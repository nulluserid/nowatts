//! nowatts — plain-English USB-C power status for Omarchy.
//!
//! STATUS: SKETCH (pre-build). A building session starts at ../PLAN.md.
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
//! Live reference data (paths + observed values): ../RESEARCH.md "Data map".
//! Verdict/warning wording: ../CONTRACT.md enums — the widget renders verbatim.

// Module map to implement (each has fixture-testable parts — see PLAN.md):
//
// mod sysfs;  // read typec / power_supply / usb / thunderbolt / drm → Snapshot
//             //   base-path parameterized so fixtures under fixtures/ are hermetic
// mod model;  // pure: Snapshot → verdicts + warnings (CONTRACT enums)
//             //   slow-charge heuristics: SPEC P2 — unit tests, no hardware needed
// mod status; // status.json v1 writer: atomic tmp+rename, change-only + heartbeat
// mod watch;  // blocking udev uevent iterator thread → Sender<Event>

fn main() {
    // TODO(P1a.5, PLAN.md): parse args (--help/--version only for v1),
    // enforce single instance, spawn watch thread, run heartbeat loop,
    // write CONTRACT.md v1 status.json on change.
    todo!("P1a — see PLAN.md; nothing here is implemented yet")
}
