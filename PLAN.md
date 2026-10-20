# BUILD PLAN

**Status: v1 is built and live on the dev machine (2026-10-19)** — daemon as
a systemd `--user` service, widget in the bar; see README. The checklists
below are now the **remaining verification queue**: owner-assisted live
proofs (unplug/replug timing, weak charger), the settings walk, and polish.
The queue order is still read-order: README → RESEARCH → CONTRACT → SPEC →
this file.

## 0 — Prerequisites

- [x] `mise install rust@stable` + `mise use rust` — installed and
      activated this session (rustc/cargo 1.99.1)
- [x] `pkg-config --exists libudev && test -f /usr/include/libudev.h`
      (both ship with Arch's `systemd` package — yes)
- [x] `git status` clean; built on a branch (`build/nowatts-v1`)
- [x] Re-verified the RESEARCH.md data map against live sysfs (one
      correction applied: psy attr is `usb_type`, not `usb_types`)

## P1a — daemon (`nowatts/`)

- [ ] **P1a.1** Crate scaffold: `Cargo.toml` deps are already sketched —
      `udev` + `serde` + `serde_json`, **no async runtime**. `cargo build`
      must pass on the stub.
- [ ] **P1a.2** `sysfs` module: read typec / power_supply / usb / thunderbolt
      / drm into a `Snapshot`. Path list = RESEARCH.md "Data map". Build
      fixture trees under `nowatts/fixtures/<case>/` mirroring the sysfs
      layout; the reader takes a base path so tests are hermetic.
- [ ] **P1a.3** `model` module: pure `Snapshot → verdicts/warnings` per
      CONTRACT.md enums. Unit tests against fixtures (FR-004).
- [ ] **P1a.4** `status` module: CONTRACT v1 writer — atomic tmp+rename,
      rewrite-on-change only, heartbeat_unix refreshed ≤ 30 s (CONTRACT rules
      2–3).
- [ ] **P1a.5** `watch` module: blocking uevent iterator (`udev` crate) in its
      own thread → channel; main loop consumes events + 30 s heartbeat timer.
      Two threads, no runtime (FR-002).
- [ ] **P1a.6** Verify the psy↔port mapping hypothesis
      (`ucsi-source-psy-USBC000:00N` ↔ `typec/port(N-1)`) at runtime against
      udev devpaths; record `degraded.psy_mapping` honestly when it can't be
      confirmed.
- [ ] **P1a.7** `cargo test` green; live run: diff status.json vs manual
      `cat` of the psy files (FR-007, ±5 %); unplug/replug → mtime updates
      ≤ 100 ms (FR-002).
- [ ] **P1a.8** Install: `cp target/release/nowatts ~/.local/bin/`, install
      `nowatts.service` to `~/.config/systemd/user/`,
      `systemctl --user daemon-reload && systemctl --user enable --now nowatts`.

## P1b — widget (`plugin/` → `~/.config/omarchy/plugins/legend.nowatts/`)

- [ ] **P1b.1** Install the plugin folder (copy, or `omarchy plugin add` if
      the repo is on GitHub by then). *(Done: deployed to
      `~/.config/omarchy/plugins/legend.nowatts` and enabled.)*
- [ ] **P1b.2** `BarWidget.qml` for real: FileView+Timer on status.json,
      label per `format` setting, stale marker (heartbeat > 60 s), `show=auto`
      muting. Copy the panelLoader/injectPanel boilerplate from
      `io.github.nulluserid.dishy/BarWidget.qml` — it is the working
      reference on this machine.
- [ ] **P1b.3** `Panel.qml` for real: per-port cards (verdict, contract vs
      measured, battery, devices+speeds, TB), warning banner, battery line.
      Model.js-style parse helper for status.json.
- [ ] **P1b.4** `omarchy plugin validate legend.nowatts` → clean (FR-006).
- [ ] **P1b.5** Hot-reload check: edit a file under the plugin dir → change
      applies without `omarchy restart shell`.
- [ ] **P1b.6** Live cross-check: bar watts vs sysfs (FR-007); unplug → bar
      reacts within one tick; kill daemon → stale marker within 60 s.

## P2 — warnings + association

- [ ] **P2.1** Warning rules (SPEC P2) + fixture tests.
- [ ] **P2.2** Device association via typec port `usbN-portM`/`usb4_portM`
      links → bus devices; `unmapped_devices` fallback (RESEARCH.md "USB
      devices").
- [ ] **P2.3** Thunderbolt line from `/sys/bus/thunderbolt`; display line
      from `/sys/class/drm` (probe it — RESEARCH.md flags it as unprobed).
- [ ] **P2.4** Live test with a weak charger (phone charger) → slow-charge
      warning.

## P3 — polish

- [ ] **P3.1** Finalize manifest settings schema (`show`/`format`/`interval`)
      and test each from the shell settings UI.
- [ ] **P3.2** preview.png, README screenshot, install.sh (daemon binary +
      unit + plugin copy).
- [ ] **P3.3** Optional: git remote/PKGBUILD/AUR — owner's call.

## Done when

SPEC.md "Success criteria" 1–5 are all demonstrated, each with its own
evidence (timing checks, ±5 % diffs, cargo test output, validate output,
24 h journal). Tick nothing on anyone's word — reproduce it.
