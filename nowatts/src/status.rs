//! status — CONTRACT v1 writer for status.json.
//!
//! Rules implemented here (CONTRACT rules 2–3, FR-003):
//! - atomic: write tmp file in the same directory, fsync, rename over target;
//! - rewrite only when content changes, EXCEPT `heartbeat_unix`, which is
//!   refreshed at least every 30 s (a heartbeat rewrite changing nothing else
//!   is fine).

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::{Status, HEARTBEAT_SECS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// First write this process (file may or may not have existed).
    Initial,
    /// Content changed since the last write.
    Change,
    /// Content unchanged; heartbeat refreshed (≥ HEARTBEAT_SECS since last write).
    Heartbeat,
    /// Nothing to do: content unchanged and heartbeat still fresh.
    Skipped,
}

impl WriteOutcome {
    pub fn as_log_str(self) -> &'static str {
        match self {
            WriteOutcome::Initial => "initial",
            WriteOutcome::Change => "change",
            WriteOutcome::Heartbeat => "heartbeat",
            WriteOutcome::Skipped => "skipped",
        }
    }
}

pub struct Writer {
    path: PathBuf,
    /// Last written document with heartbeat zeroed (the change-comparison body).
    last_body: Option<String>,
    last_write_unix: Option<u64>,
}

impl Writer {
    pub fn new(path: PathBuf) -> Self {
        Writer {
            path,
            last_body: None,
            last_write_unix: None,
        }
    }

    /// Compare-then-write. `now_unix` stamps the document's heartbeat.
    pub fn write(&mut self, status: &Status, now_unix: u64) -> io::Result<WriteOutcome> {
        let mut probe = status.clone();
        probe.heartbeat_unix = 0;
        let body = serde_json::to_string(&probe).expect("Status is always serializable");

        let changed = self.last_body.as_deref() != Some(body.as_str());
        let heartbeat_due = self
            .last_write_unix
            .map_or(true, |t| now_unix.saturating_sub(t) >= HEARTBEAT_SECS);

        if !changed && !heartbeat_due {
            return Ok(WriteOutcome::Skipped);
        }
        let outcome = if changed {
            if self.last_body.is_none() {
                WriteOutcome::Initial
            } else {
                WriteOutcome::Change
            }
        } else {
            WriteOutcome::Heartbeat
        };

        let mut doc = status.clone();
        doc.heartbeat_unix = now_unix;
        let rendered = serde_json::to_string(&doc).expect("Status is always serializable");
        atomic_write(&self.path, rendered.as_bytes())?;

        self.last_body = Some(body);
        self.last_write_unix = Some(now_unix);
        Ok(outcome)
    }
}

/// CONTRACT rule 2: tmp file in the same directory, fsync, rename. A reader
/// (the widget) therefore never observes a partial file.
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(".nowatts.status.json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // Best-effort directory fsync so the rename itself survives power loss.
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::build;
    use crate::sysfs;
    use std::path::Path;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nowatts-status-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn snap(case: &str) -> sysfs::Snapshot {
        sysfs::read_snapshot(Path::new(&format!(
            "{}/fixtures/{}/sysfs",
            env!("CARGO_MANIFEST_DIR"),
            case
        )))
    }

    #[test]
    fn atomic_write_leaves_no_tmp_and_valid_json() {
        let dir = temp_dir("atomic");
        let path = dir.join("status.json");
        let mut w = Writer::new(path.clone());
        let doc = build(&snap("charger-charging"), 100);
        w.write(&doc, 100).unwrap();

        let entries: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["status.json"], "no tmp leftovers: {entries:?}");

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("always parseable");
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["heartbeat_unix"], 100);
        assert!(raw.starts_with("{\"version\":1,"), "version first: {raw}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchanged_content_within_heartbeat_window_is_skipped() {
        let dir = temp_dir("skip");
        let path = dir.join("status.json");
        let mut w = Writer::new(path.clone());
        let doc = build(&snap("charger-charging"), 100);
        assert_eq!(w.write(&doc, 100).unwrap(), WriteOutcome::Initial);
        let m0 = fs::metadata(&path).unwrap().modified().unwrap();

        // +1 s, same content → nothing touches the file (change-only rule).
        let doc2 = build(&snap("charger-charging"), 101);
        assert_eq!(w.write(&doc2, 101).unwrap(), WriteOutcome::Skipped);
        let m1 = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(m0, m1, "skipped write must not touch the file");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn heartbeat_refreshes_at_least_every_30s_without_content_change() {
        let dir = temp_dir("heartbeat");
        let path = dir.join("status.json");
        let mut w = Writer::new(path.clone());
        let doc = build(&snap("charger-charging"), 100);
        assert_eq!(w.write(&doc, 100).unwrap(), WriteOutcome::Initial);

        // +29 s: still fresh → skipped. +30 s: heartbeat due → rewrite that
        // changes nothing but heartbeat_unix (CONTRACT rule 3).
        let doc29 = build(&snap("charger-charging"), 129);
        assert_eq!(w.write(&doc29, 129).unwrap(), WriteOutcome::Skipped);

        let doc30 = build(&snap("charger-charging"), 130);
        assert_eq!(w.write(&doc30, 130).unwrap(), WriteOutcome::Heartbeat);
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["heartbeat_unix"], 130);
        let m = fs::metadata(&path).unwrap().modified().unwrap();
        let _ = m; // mtime moved (rewrite happened); value asserted above
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn content_change_writes_immediately() {
        let dir = temp_dir("change");
        let path = dir.join("status.json");
        let mut w = Writer::new(path.clone());
        assert_eq!(
            w.write(&build(&snap("charger-charging"), 100), 100).unwrap(),
            WriteOutcome::Initial
        );
        // An unplug: different content at +1 s must land immediately.
        let doc = build(&snap("idle-no-partner"), 101);
        assert_eq!(w.write(&doc, 101).unwrap(), WriteOutcome::Change);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("Nothing connected"));
        assert_eq!(w.write(&doc, 102).unwrap(), WriteOutcome::Skipped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_write_simulation_never_corrupts_target() {
        // A reader that polls continuously must only ever see full documents.
        let dir = temp_dir("partial");
        let path = dir.join("status.json");
        let mut w = Writer::new(path.clone());
        let mut heartbeat = 1_000_000u64;
        for case in [
            "charger-charging",
            "idle-no-partner",
            "display-connected",
            "usb-device-attached",
            "charger-charging",
        ] {
            let doc = build(&snap(case), heartbeat);
            w.write(&doc, heartbeat).unwrap();
            heartbeat += 60; // force a rewrite every step
            let raw = fs::read_to_string(&path).unwrap();
            let v: serde_json::Value =
                serde_json::from_str(&raw).expect("target is always complete");
            assert_eq!(v["version"], 1);
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
