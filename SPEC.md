# SPEC — nowatts v1

Plain-English USB-C power status for Omarchy: a Rust daemon (`nowatts`) that
watches the kernel's view of the USB-C ports and writes
`~/.local/state/nowatts/status.json` (see CONTRACT.md), plus an Omarchy shell
bar widget (`legend.nowatts`) that renders it.

**Scope ruling (owner, 2026-10-19):** bar widget + panel only. No CLI, no
notifications in v1. Core is Rust. Read-only. The name is `nowatts`.

## User stories

### P1a — Daemon produces status.json

**Given** this X1C with a charger negotiating 20 V / 5 A on port1
(observed live 2026-10-19: contract 100 W, measured ~65 W),
**when** `nowatts` runs,
**then** `~/.local/state/nowatts/status.json` matches CONTRACT.md v1 and
reports the contract, the measured draw, the battery watts, and the verdict
line — updated within 100 ms of a kernel uevent, with a heartbeat at most
30 s old.

**Independent test:** `cargo test` over fixture sysfs trees, plus a live diff
of status.json numbers against manual `cat` of the psy files (±5 %), plus an
unplug/replug timing check (`stat` mtime deltas).

### P1b — Bar widget + panel

**Given** a live status.json,
**then** the bar shows the plug glyph + live watts per the `format` setting
(`watts` → measured W, `contract` → contract W, `icon` → glyph only), and
clicking opens the panel: one plain-English card per port — location
("Left upper"), verdict, contract vs measured, battery rate, attached devices
with speeds, Thunderbolt line — plus a stale marker when the daemon is dead.

**Independent test:** `omarchy plugin validate legend.nowatts` passes; widget
numbers match sysfs within one tick (~1 s); unplug → bar reacts ≤ ~1.2 s;
kill the daemon → stale marker within 60 s.

### P2 — Warnings + device association

**Given** AC online but `BAT0/power_now` negative, **then** the port card
shows `"On AC but the battery is draining — charger or cable is too weak"`.
**Given** a contract ≤ 15 W while charging, **then**
`"Slow charge: contract limited to {cw}W"`.
**Given** a USB device attached to a port, **then** it is listed under that
port via the typec port's `usbN-portM` / `usb4_portM` links, with an
`unmapped_devices` fallback.

**Independent test:** fixture-tree unit tests for both warning rules and the
association resolver; a real weak charger for the live warning.

### P3 — Settings + polish

The manifest schema drives: `show` (`auto` hides/mutes the bar button when
every port has no partner and AC is offline / `always`), `format`
(`watts|contract|icon`), `interval` (500–10000 ms). Preview image; README
screenshot; optional install script.

**Independent test:** change each setting in the shell's widget settings UI →
behavior changes without a restart (hot-reload).

## Requirements (MUST)

1. **FR-001** Read-only daemon: never writes sysfs, no privileged calls,
   single instance.
2. **FR-002** Event-driven: uevent → status.json ≤ 100 ms; heartbeat ≤ 30 s;
   no busy polling.
3. **FR-003** Atomic status.json writes (tmp+rename, CONTRACT rule 2); widget
   tolerates missing/stale files (stale = heartbeat older than 60 s).
4. **FR-004** Verdict/warning logic is a pure, fixture-tested function
   (`cargo test`), independent of live hardware.
5. **FR-005** Missing or partial data degrades explicitly ("No USB-C data
   available", `degraded.*` flags) — never a crash, never silent zeros.
6. **FR-006** Widget passes `omarchy plugin validate`, hot-reloads on save,
   and spawns no subprocesses.
7. **FR-007** Displayed watts within 5 % of sysfs truth; watts = V × A.
8. **FR-008** Daemon runs as a systemd `--user` service, independent of the
   shell (`systemctl --user enable --now nowatts` survives shell restarts).

## Out of scope (v1)

- Cable e-marker display and charger PDO list — impossible on this hardware
  (RESEARCH.md "Constraints"; owner accepted)
- CLI tool, notifications, charge limiting / threshold control
- Multi-language UI
- Non-UCSI platforms (design degrades gracefully, but nothing is promised)

## Success criteria

1. Plug/unplug lands in status.json ≤ 100 ms and on screen ≤ ~1.2 s.
2. Displayed watts within 5 % of sysfs values.
3. Daemon RSS ≤ 10 MB and flat over 24 h; clean `journalctl --user -u nowatts`.
4. `cargo test` green (fixtures); `omarchy plugin validate` clean; hot-reload
   applies widget edits without restart.
5. On hardware without a typec class: explicit degraded verdict, no crash.

## Assumptions

- English-only, single user, read-only (FR-001), plugin id `legend.nowatts`.
- Repo directory stays `~/Projects/omarchy_usb_power`; renaming to `nowatts/`
  is the owner's call.
- Omarchy 4.0.4 shell conventions (manifest schemaVersion 1, Dishy-style
  plugin layout) — revisit on major Omarchy updates.
- UCSI data quality is what it is: contract + measured only. The daemon must
  not invent charger capabilities it cannot read.
