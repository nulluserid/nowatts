//! nowatts — plain-English USB-C power status for Omarchy.
//!
//! Settled architecture (2026-10-19 — see ../SPEC.md, ../RESEARCH.md):
//!   - no async runtime; two threads:
//!       1. blocking uevent watcher (udev crate) → channel
//!       2. main loop: recompute on event + 30s heartbeat, write on change
//!   - output: ~/.local/state/nowatts/status.json, CONTRACT.md v1
//!     (atomic tmp+rename, change-only writes, heartbeat_unix ≤ 30s)
//!   - reads only world-readable sysfs; NEVER writes sysfs (FR-001)
//!   - single instance (FR-001); RSS target ≤ 10 MB (success criteria)
//!
//! `--base` / `--status` are internal/testing overrides so runs can be
//! pointed at a fixture sysfs tree and a scratch output path (hermetic,
//! never touching the deployed state file).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod model;
mod status;
mod sysfs;
mod watch;

use status::Writer;

struct Args {
    base: PathBuf,
    status: PathBuf,
}

const USAGE: &str = "nowatts — plain-English USB-C power status for Omarchy

Usage: nowatts [--base <PATH>] [--status <PATH>]
       nowatts --help | --version

Options:
  --base <PATH>     sysfs base path (default: /sys; internal/testing override)
  --status <PATH>   status.json output path
                    (default: ~/.local/state/nowatts/status.json;
                    internal/testing override)
  -h, --help        show this help
  -V, --version     print version";

fn default_status_path() -> PathBuf {
    let state_dir = match std::env::var_os("XDG_STATE_HOME") {
        Some(v) => PathBuf::from(v),
        None => match std::env::home_dir() {
            Some(h) => h.join(".local/state"),
            None => return PathBuf::from("status.json"),
        },
    };
    state_dir.join("nowatts/status.json")
}

fn parse_args() -> Result<Args, String> {
    let mut base: Option<PathBuf> = None;
    let mut status: Option<PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("nowatts {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--base" => {
                base = Some(PathBuf::from(
                    it.next().ok_or("--base requires a PATH argument")?,
                ));
            }
            "--status" => {
                status = Some(PathBuf::from(
                    it.next().ok_or("--status requires a PATH argument")?,
                ));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        base: base.unwrap_or_else(|| PathBuf::from("/sys")),
        status: status.unwrap_or_else(default_status_path),
    })
}

/// FR-001: single instance. std file lock on `<status dir>/nowatts.lock` —
/// the lock lives with the output file, so a testing run pointed at /tmp
/// neither blocks nor gets blocked by the deployed daemon, while production
/// (default path) stays exclusive. The lock file is held for process
/// lifetime; the OS releases it on exit (clean or not).
fn acquire_single_instance(lock_path: &PathBuf) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(lock_path)?;
    file.try_lock().map_err(std::io::Error::from)?;
    Ok(file)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nowatts: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let status_dir = args
        .status
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = std::fs::create_dir_all(&status_dir) {
        eprintln!("nowatts: cannot create state dir {}: {e}", status_dir.display());
        return ExitCode::FAILURE;
    }

    let lock_path = status_dir.join("nowatts.lock");
    let _instance_lock = match acquire_single_instance(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "nowatts: another instance appears to hold {} ({e}); refusing to start",
                lock_path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "nowatts {} starting: base={} status={}",
        env!("CARGO_PKG_VERSION"),
        args.base.display(),
        args.status.display()
    );

    // One writer, one reader channel: watcher thread → main loop.
    let (tx, rx) = mpsc::channel::<watch::WatchMsg>();
    let _watcher = match watch::spawn_watcher(tx) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("nowatts: cannot start uevent watcher: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut writer = Writer::new(args.status.clone());
    let mut last_battery_note: Option<String> = None;

    // Initial write before blocking: the widget must not wait up to 30 s for
    // the first status.json after daemon start.
    recompute_and_write(&args.base, &mut writer, &mut last_battery_note, "startup");

    loop {
        let reason = match rx.recv_timeout(Duration::from_secs(model::HEARTBEAT_SECS)) {
            Ok(watch::WatchMsg::WatcherError(e)) => {
                eprintln!("nowatts: uevent watcher failed: {e}; shutting down");
                return ExitCode::FAILURE;
            }
            Ok(watch::WatchMsg::Uevent(first)) => {
                // FR-002: the first event triggers an immediate recompute —
                // waiting out a debounce window first would push the write
                // past the 100 ms uevent→status.json budget. The rest of the
                // burst is then coalesced into one follow-up pass, which the
                // change-only writer turns into a no-op unless the late
                // events actually shifted the data.
                recompute_and_write(&args.base, &mut writer, &mut last_battery_note, &format!("event ({first})"));
                let mut burst: Vec<String> = Vec::new();
                for msg in watch::drain_burst(&rx) {
                    match msg {
                        watch::WatchMsg::Uevent(line) => burst.push(line),
                        watch::WatchMsg::WatcherError(e) => {
                            eprintln!("nowatts: uevent watcher failed: {e}; shutting down");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                if !burst.is_empty() {
                    recompute_and_write(
                        &args.base,
                        &mut writer,
                        &mut last_battery_note,
                        &format!("event (burst: {})", burst.join(", ")),
                    );
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => "heartbeat".to_string(),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("nowatts: uevent watcher exited; shutting down");
                return ExitCode::FAILURE;
            }
        };

        recompute_and_write(&args.base, &mut writer, &mut last_battery_note, &reason);
    }
}

/// One scan + build + write cycle, shared by the startup write and the loop.
fn recompute_and_write(
    base: &std::path::Path,
    writer: &mut Writer,
    last_battery_note: &mut Option<String>,
    reason: &str,
) {
    let now = unix_now();
    let snap = sysfs::read_snapshot(base);
    if snap.battery_status_note != *last_battery_note {
        if let Some(note) = &snap.battery_status_note {
            eprintln!("nowatts: battery status from {note}");
        }
        *last_battery_note = snap.battery_status_note.clone();
    }
    let doc = model::build(&snap, now);
    match writer.write(&doc, now) {
        Ok(outcome) => {
            if outcome != status::WriteOutcome::Skipped {
                eprintln!(
                    "nowatts: status.json written ({}) on {reason}",
                    outcome.as_log_str()
                );
            }
        }
        Err(e) => eprintln!("nowatts: writing status.json failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!(
            "nowatts-main-test-{}-lock",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("nowatts.lock");

        let _first = acquire_single_instance(&lock).expect("first holder succeeds");
        let second = acquire_single_instance(&lock);
        assert!(second.is_err(), "second instance must be refused");

        // Lock lives with the output file: a testing run in another dir
        // doesn't collide with the deployed daemon's lock (by design).
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_status_path_uses_state_dir() {
        // No panic, sane shape; honors XDG_STATE_HOME when set.
        std::env::set_var("XDG_STATE_HOME", "/tmp/opencode/nowatts-xdg-test");
        let p = default_status_path();
        assert_eq!(
            p,
            PathBuf::from("/tmp/opencode/nowatts-xdg-test/nowatts/status.json")
        );
        std::env::remove_var("XDG_STATE_HOME");
        let p = default_status_path();
        assert!(p.ends_with("nowatts/status.json"), "{p:?}");
    }
}
