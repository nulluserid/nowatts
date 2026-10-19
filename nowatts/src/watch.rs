//! watch — blocking uevent iterator (udev crate) in its own thread → channel.
//!
//! FR-002: event-driven, no async runtime, no busy polling. The thread blocks
//! in libudev's receive loop; the main loop times out on the channel after
//! 30 s to refresh the heartbeat. The monitor is constructed *inside* the
//! thread (libudev handles are not `Send`), so the only thing crossing the
//! channel is a message enum.

use std::io;
use std::mem::ManuallyDrop;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use std::sync::mpsc::{Receiver, Sender};

/// Subsystems that can change what we report. Anything else is ignored by
/// the kernel-side filter (no busy work for unrelated hotplug noise).
const SUBSYSTEMS: [&str; 5] = ["typec", "power_supply", "usb", "thunderbolt", "drm"];

/// Debounce window: udev often delivers bursts (one unplug → add/remove/
/// change across subsystems); recompute once per burst.
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// What the watcher tells the main loop.
#[derive(Debug)]
pub enum WatchMsg {
    /// One uevent, described for the stderr log.
    Uevent(String),
    /// The watcher could not start or its socket failed fatally.
    WatcherError(String),
}

/// Spawn the blocking watcher thread. The thread ends when the socket fails
/// or the receiver hangs up.
pub fn spawn_watcher(sender: Sender<WatchMsg>) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("nowatts-watch".to_string())
        .spawn(move || {
            let run = || -> io::Result<()> {
                let mut builder = udev::MonitorBuilder::new()?;
                for sub in SUBSYSTEMS {
                    builder = builder.match_subsystem(sub)?;
                }
                let socket = builder.listen()?;
                // The udev crate documents its iterator as non-blocking ("a
                // variant of poll() should be used on the file descriptor").
                // We clear O_NONBLOCK through std's UnixStream instead, so
                // the receive below genuinely blocks — no busy polling
                // (FR-002), no extra dependency.
                //
                // SAFETY: we alias the udev-owned fd read-only to flip its
                // O_NONBLOCK flag; ManuallyDrop ensures std never closes the
                // fd out from under the monitor. Same thread, fd valid for
                // the socket's lifetime.
                let stream =
                    ManuallyDrop::new(unsafe { UnixStream::from_raw_fd(socket.as_raw_fd()) });
                stream.set_nonblocking(false)?;

                for event in socket.iter() {
                    let line = format!(
                        "{:?} {}",
                        event.event_type(),
                        event.device().sysname().to_string_lossy()
                    );
                    if sender.send(WatchMsg::Uevent(line)).is_err() {
                        return Ok(()); // main loop gone — nothing left to watch for
                    }
                }
                Ok(())
            };
            if let Err(e) = run() {
                let _ = sender.send(WatchMsg::WatcherError(e.to_string()));
            }
        })
}

/// After the first received event, collect everything already queued within
/// the debounce window so one burst triggers one recompute.
pub fn drain_burst(rx: &Receiver<WatchMsg>) -> Vec<WatchMsg> {
    let mut rest = Vec::new();
    while let Ok(msg) = rx.recv_timeout(DEBOUNCE) {
        rest.push(msg);
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn drain_burst_collects_queued_events_without_blocking_on_empty() {
        let (tx, rx) = mpsc::channel::<WatchMsg>();
        for i in 0..3 {
            tx.send(WatchMsg::Uevent(format!("burst-{i}"))).unwrap();
        }
        let rest = drain_burst(&rx);
        assert_eq!(rest.len(), 3);
        assert!(matches!(&rest[0], WatchMsg::Uevent(s) if s == "burst-0"));

        // Empty queue: returns after the debounce window, no hang, no error.
        let start = std::time::Instant::now();
        let rest = drain_burst(&rx);
        assert!(rest.is_empty());
        assert!(start.elapsed() >= DEBOUNCE);
    }

    // spawn_watcher itself needs a live libudev socket; the main-loop and
    // burst behavior around it are exercised by drain_burst above and the
    // live run against this machine.
}
