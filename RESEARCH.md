# RESEARCH — verified on this machine, 2026-10-19

Everything below was probed live during the planning session. Paths are exact.
Sample values were observed while a charger was attached to port1, so they
double as a worked example. A building session should re-verify rather than
trust blindly — but this is a real map, not a guess.

## Machine

- ThinkPad X1 Carbon Gen 13 (21NS0014US), Core Ultra 7 258V (Lunar Lake)
- Omarchy 4.0.4, kernel `7.2.5-3-omarchy`, Hyprland + omarchy-shell (Quickshell)
- 2× USB-C via **UCSI**: platform device `USBC000:00`, driver `ucsi_acpi`
- Not a TCPM platform: no `/sys/kernel/debug/tcpm` (and debugfs needs root).
  Do not design anything around TCPM.

## Data map (all world-readable, no root needed)

### Type-C ports — `/sys/class/typec/port{0,1}`

- `power_role`: `source [sink]` · `data_role`: `host [device]` (brackets = active)
- `usb_power_delivery_revision`: `2.0`
- `usb_capability` (port1): `usb2 [usb3]`
- `physical_location/{panel,vertical_position,horizontal_position,dock,lid}`:
  both ports read `top / upper / left / no / no` → display as **"Left upper"**
- Also: `vconn_source`, `supported_accessory_modes`, `usb_typec_revision`
- **Partner:** the `portN-partner` directory appears on attach — its existence
  *is* the connect event. Inside:
  `supports_usb_power_delivery` (yes), `usb_power_delivery_revision`,
  `number_of_alternate_modes`, `accessory_mode`, and a `pd2/` dir
  - **No `identity/` dir** — SOP Discover Identity is not exposed by this UCSI
    implementation (observed with a charger attached; may appear for other
    partners — display when present, never depend on it)
- **No `portN-plug` dirs** — cable e-marker (SOP′) is not exposed at all on
  this platform
- Port alt modes: `port1.{0,1,2}/mode1/{active,vdo}` exist; `svid` files read
  back empty during planning — resolve at build time. P2 display detection
  should lean on DRM (below) instead.

### Kernel PD class — `/sys/class/usb_power_delivery/pd{0,1,2}`

- `pd0`/`pd1` = the two ports, `pd2` = port1's partner; `revision` = 2.0
- `source-capabilities` / `sink-capabilities` exist but are **EMPTY under UCSI**
  → the full charger PDO list is not available. We get the *negotiated
  contract* only. (WhatCable's "charger can do up to 96W" feature is out of
  scope on this hardware — owner accepted.)

### Power supplies — `/sys/class/power_supply/`

- `AC` (Mains) · `BAT0` (Battery) · `ucsi-source-psy-USBC000:00{1,2}` (USB)
- Observed live with charger on port1:

  | file | :001 (idle) | :002 (charging) |
  |---|---|---|
  | `online` | 0 | 1 |
  | `status` | Not charging | Charging |
  | `voltage_max` | 0 | 20000000 (20 V) |
  | `current_max` | 0 | 5000000 (5 A) |
  | `voltage_now` | 0 | 20000000 |
  | `current_now` | 0 | 3250000 (3.25 A) |
  | `usb_type` | `[C] PD PD_PPS` | `C [PD] PD_PPS` |

  → contract = 20 V × 5 A = **100 W**; measured = 20 V × 3.25 A = **65 W**.
  `charge_type` was `Standard`.
  *(Correction, build session 2026-10-19: the psy attribute is named
  `usb_type`, not `usb_types` — re-verified live the same day; brackets
  still mark the active entry.)*
- `BAT0`: `status=Charging`, `power_now=17368000` (17.4 W into the battery),
  `energy_now`/`energy_full` for percent
- **Port↔PSY mapping hypothesis (verify at build time, PLAN §P1a.6):**
  `ucsi-source-psy-USBC000:00N` ↔ `typec/port(N-1)`. Evidence: `:002` was
  online exactly when the partner was present on port1. Cross-check at runtime
  via udev devpath; degrade honestly (`degraded.psy_mapping`) if it doesn't hold.

### USB devices — `/sys/bus/usb/devices/`

- `speed`: 1.5 / 12 / 480 / 5000 / 10000 / 20000 Mbps; plus `product`,
  `manufacturer`, `idVendor`, `idProduct`
- Exclude root hubs (`usbN`) — they report the controller's capability
  (usb2/usb4 hubs showed 20000), not a connected device
- **Port association:** typec port dirs contain kernel-provided links — port0
  had `usb2-port1`, `usb3-port2`, `usb4_port1`. Resolve these to bus devices
  to list devices under their physical port. Fallback: an `unmapped_devices`
  list. (No macOS-style XHCI walking needed — the kernel did the work.)

### Thunderbolt — `/sys/bus/thunderbolt/devices/`

- `domain{0,1}`, `0-0`/`1-0` = host routers, `N-M` = attached gear
- Router attrs read live: `device_name` (Gen16), `vendor_name` (Intel),
  `generation` (4), `authorized` (1)

### Display — `/sys/class/drm/`

- **Not probed during planning.** At build time: `card*-*/status`
  (`connected`/`disconnected`), `enabled`, `modes`. DP alt-mode truth should
  come from here, not the typec alt-mode svid files.

### Events

- udev uevents fire on partner attach/remove, power_supply property changes,
  and USB add/remove — enough to drive the daemon event-first.

## Constraints that shaped the spec (owner accepted 2026-10-19)

1. **No cable e-marker data** → "your cable is 60 W / USB 2.0" is impossible
   here. Slow-charge detection is *inferred* from the negotiated contract plus
   real battery behavior.
2. **No charger PDO list** → we show the negotiated contract, not the
   charger's full menu.
3. **Charger identity absent today** (no SOP identity dir) → show when
   present, never depend on it.

## Desktop surface (Omarchy 4.0.4)

- The bar runs inside `omarchy-shell`, a single Quickshell process. Bar
  widgets are plugins under `~/.config/omarchy/plugins/<id>/` —
  `manifest.json` + QML, hot-reloading on save.
- **Working template on this machine:** `io.github.nulluserid.dishy`
  (bar widget + Panel + Model.js). Mirror its structure:
  - `manifest.json`: `schemaVersion 1`, `kinds: ["bar-widget"]`,
    `entryPoints.barWidget: "BarWidget.qml"`, `barWidget.{displayName, category,
    allowMultiple, defaultSection, defaults, schema}`; schema entries are
    `enum` (options, defaultValue) or `integer` (min/max/step/defaultValue)
  - `BarWidget.qml`: `BarWidget` base; `moduleName`; `setting(key, default)`;
    a `label` property; `Timer` + `FileView { path; reload(); text() }` from
    `Quickshell.Io` — Dishy polls `~/.local/state/starlinkd/status.json`,
    exactly our pattern
  - `Panel.qml`: `Panel` base; `moduleName`, `ipcTarget`, `manageIpc: false`;
    the host injects `bar`/`settings`/`anchorItem`/`hostWidget`/`status`;
    `open()`/`close()`/`toggle()`/`opened`
- Useful commands: `omarchy plugin validate <dir>`, `omarchy plugin
  clone|add|enable|list`, `omarchy restart shell`, `omarchy refresh shell`
- **Daemon precedent:** `starlinkd` runs as a systemd **--user** unit at
  `~/.config/systemd/user/starlinkd.service` (`Restart=on-failure`,
  `RestartSec=5`, `WantedBy=default.target`). Mirror it for `nowatts.service`.

## Toolchain (as of 2026-10-19)

- go 1.27.2 via mise (installed; **not needed** — Rust was chosen)
- **Rust: NOT installed** → `mise install rust@stable` is prerequisite #0
- pkg-config + libudev headers ship with Arch's `systemd` package; verify at
  build: `pkg-config --exists libudev && test -f /usr/include/libudev.h`
- python3, upower, lsusb present (verification aids)

## Name

- **`nowatts`** — clean on web search (2026-10-19) in the linux/power-tool space
- Rejected on collision: `wattbar` (WattBar, a Mac menu bar power monitor —
  same concept), `wattson` (Dr. Wattson energy-monitoring hardware; WattsOn
  academic power models), `plugd`, `pdpeek`
- Reads both ways on purpose: **"now: watts"** (the bar readout) and
  **"no watts?!"** (the slow-charge warning)
