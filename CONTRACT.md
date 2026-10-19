# CONTRACT — `~/.local/state/nowatts/status.json` (v1)

`nowatts` (the daemon) writes it; `legend.nowatts` (the widget) reads it.
Nothing else touches it. This file is the whole interface between the two
halves of the project — change it only with a `version` bump.

## Rules

1. The daemon is the only writer; the widget is the only reader.
2. Writes are atomic: write to a tmp file in the same directory, fsync, rename
   over the target.
3. Rewrite only when content changes; refresh `heartbeat_unix` at least every
   30 s (a heartbeat rewrite that changes nothing else is fine).
4. **Stale rule:** the widget treats the file as stale when
   `now - heartbeat_unix > 60` → show a stale/timeout marker; never silently
   keep displaying old numbers.
5. `version` comes first. v1 consumers ignore unknown fields; v2 bumps the
   field and documents itself.
6. Every emitted number is traceable to a sysfs file (see RESEARCH.md "Data
   map"). Watts = volts × amps, rounded to one decimal.

## Schema (v1)

```jsonc
{
  "version": 1,
  "heartbeat_unix": 1760868000,

  "battery": {
    "present": true,
    "status": "Charging",            // power_supply BAT0 status, verbatim
    "power_w": 17.4,                 // BAT0 power_now, µW → W; negative = discharging
    "percent": 18.9                  // energy_now / energy_full × 100
  },
  "ac_online": true,                 // power_supply AC online

  "ports": [
    {
      "id": "port1",                 // typec port dir name
      "psy": "ucsi-source-psy-USBC000:002",
      "location": "left upper",      // from physical_location/*
      "partner": true,               // portN-partner exists
      "power_role": "sink",
      "data_role": "device",
      "usb_capability": "usb3",
      "pd_revision": "2.0",
      "supports_pd": true,

      "contract": {                  // negotiated PD contract; null when absent
        "volts": 20.0, "amps": 5.0, "watts": 100.0
      },
      "measured": {                  // live draw; null when not measurable
        "volts": 20.0, "amps": 3.25, "watts": 65.0
      },
      "usb_types": ["C", "PD", "PD_PPS"],   // supported set, from psy usb_types
      "active_usb_type": "PD",              // bracketed entry in usb_types

      "devices": [                   // USB devices associated to this port
        { "name": "…", "vendor": "…", "product": "…", "speed_mbps": 10000 }
      ],
      "thunderbolt": {               // present only when a TB/USB4 link exists
        "generation": 4, "authorized": true, "name": "Gen16"
      },
      "display": {
        "connectors": ["DP-1 connected"],   // verbatim from /sys/class/drm
        "dp_alt_mode": false
      },
      "identity": null,              // partner SOP identity if ever exposed

      "verdict": "…",                // plain English, enum below
      "warning": null                // plain English, enum below; null = fine
    }
  ],

  "unmapped_devices": [],            // USB devices we couldn't tie to a port
  "degraded": {
    "typec": true,                   // false when /sys/class/typec is missing
    "psy_mapping": "assumed"         // "assumed" | "verified" | "unknown"
  }
}
```

## Verdict enum (v1)

The widget renders these verbatim; the daemon owns the wording.

- `"Nothing connected"`
- `"Charging at {w}W of a {cw}W contract — battery taking {b}W"`
- `"Charging at {w}W — battery taking {b}W"` (contract not readable)
- `"Battery full — plugged in, not drawing"` (AC online + status *Not charging*)
- `"Supplying power to a device"` (power_role source)
- `"Device connected (USB {speed})"` (partner, no power contract)
- `"Display connected"`
- `"No USB-C data available"` (degraded.typec = false; then `ports` is `[]`)

Verdicts compose the highest-priority applicable line; the panel can show the
full detail regardless. Priority: charging > display > device > nothing.

## Warning enum (v1)

- `"On AC but the battery is draining — charger or cable is too weak"`
  (ac_online && battery.power_w < 0)
- `"Slow charge: contract limited to {cw}W"` (contract.watts ≤ 15 while charging)
- Warnings are additive to the verdict; `null` means all-clear.

## Example (observed live 2026-10-19, charger on port1)

```json
{
  "version": 1,
  "heartbeat_unix": 1760868000,
  "battery": { "present": true, "status": "Charging", "power_w": 17.4, "percent": 18.9 },
  "ac_online": true,
  "ports": [
    {
      "id": "port0", "psy": "ucsi-source-psy-USBC000:001",
      "location": "left upper", "partner": false,
      "power_role": "sink", "data_role": "device",
      "usb_capability": "usb2", "pd_revision": "2.0", "supports_pd": true,
      "contract": null, "measured": null,
      "usb_types": ["C", "PD", "PD_PPS"], "active_usb_type": "C",
      "devices": [], "display": { "connectors": [], "dp_alt_mode": false },
      "identity": null,
      "verdict": "Nothing connected", "warning": null
    },
    {
      "id": "port1", "psy": "ucsi-source-psy-USBC000:002",
      "location": "left upper", "partner": true,
      "power_role": "sink", "data_role": "device",
      "usb_capability": "usb3", "pd_revision": "2.0", "supports_pd": true,
      "contract": { "volts": 20.0, "amps": 5.0, "watts": 100.0 },
      "measured": { "volts": 20.0, "amps": 3.25, "watts": 65.0 },
      "usb_types": ["C", "PD", "PD_PPS"], "active_usb_type": "PD",
      "devices": [],
      "thunderbolt": { "generation": 4, "authorized": true, "name": "Gen16" },
      "display": { "connectors": [], "dp_alt_mode": false },
      "identity": null,
      "verdict": "Charging at 65W of a 100W contract — battery taking 17.4W",
      "warning": null
    }
  ],
  "unmapped_devices": [],
  "degraded": { "typec": true, "psy_mapping": "assumed" }
}
```
