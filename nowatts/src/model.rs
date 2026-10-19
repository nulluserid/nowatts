//! model — pure `Snapshot → CONTRACT v1 status`. No I/O anywhere (FR-004).
//!
//! Verdict and warning strings are CONTRACT.md enums, rendered verbatim —
//! the widget prints them as-is. Priority per contract: charging > battery
//! full > display > device > supplying > nothing.

use serde::Serialize;

use crate::sysfs::{PortSnapshot, PsySnapshot, Snapshot, UsbDevice};

pub const CONTRACT_VERSION: i64 = 1;

/// Contract v1 says "at least every 30 s".
pub const HEARTBEAT_SECS: u64 = 30;

/// Warning rule 1 threshold: this exact string, verbatim.
pub const WARN_ON_AC_DRAINING: &str = "On AC but the battery is draining — charger or cable is too weak";

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: i64,
    pub heartbeat_unix: u64,
    pub battery: BatteryStatus,
    pub ac_online: Option<bool>,
    pub ports: Vec<PortStatus>,
    /// Top-level line for the degraded case only (typec class missing): the
    /// enum line `"No USB-C data available"` must reach the widget even though
    /// `ports` is `[]`. v1 consumers ignore unknown fields (contract rule 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    pub unmapped_devices: Vec<DeviceStatus>,
    pub degraded: Degraded,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryStatus {
    pub present: bool,
    pub status: Option<String>,
    pub power_w: Option<f64>,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortStatus {
    pub id: String,
    pub psy: Option<String>,
    pub location: Option<String>,
    pub partner: bool,
    pub power_role: Option<String>,
    pub data_role: Option<String>,
    pub usb_capability: Option<String>,
    pub pd_revision: Option<String>,
    pub supports_pd: bool,
    pub contract: Option<Watts>,
    pub measured: Option<Watts>,
    pub usb_types: Vec<String>,
    pub active_usb_type: Option<String>,
    pub devices: Vec<DeviceStatus>,
    /// Present only when a TB/USB4 link exists (contract: skip otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thunderbolt: Option<TbStatus>,
    pub display: DisplayStatus,
    /// SOP partner identity; not exposed by this UCSI implementation — v1
    /// always renders null, never invented.
    pub identity: Option<serde_json::Value>,
    pub verdict: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Watts {
    pub volts: f64,
    pub amps: f64,
    pub watts: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TbStatus {
    pub generation: Option<u32>,
    pub authorized: bool,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisplayStatus {
    pub connectors: Vec<String>,
    pub dp_alt_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub name: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub speed_mbps: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Degraded {
    /// CONTRACT semantics verbatim: `false` when /sys/class/typec is missing
    /// (true = data available). The flag name reads backwards; the contract
    /// text governs.
    pub typec: bool,
    /// "assumed" | "verified" | "unknown".
    pub psy_mapping: String,
}

/// Verdict line: `"Charging at 65W of a 100W contract — battery taking 17.4W"`.
pub fn fmt_watts(w: f64) -> String {
    let r = (w * 10.0).round() / 10.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Build the CONTRACT v1 document. `now_unix` stamps `heartbeat_unix`.
pub fn build(snap: &Snapshot, now_unix: u64) -> Status {
    let battery = BatteryStatus {
        present: snap.battery.present,
        status: snap.battery.status.clone(),
        power_w: snap.battery.power_w,
        percent: snap.battery.percent,
    };

    let ports: Vec<PortStatus> = if snap.typec_available {
        snap.ports.iter().map(|p| build_port(p, snap, &battery)).collect()
    } else {
        // Degraded: explicit "No USB-C data available", never invented ports.
        Vec::new()
    };

    let top_verdict = if snap.typec_available {
        None
    } else {
        Some("No USB-C data available".to_string())
    };

    Status {
        version: CONTRACT_VERSION,
        heartbeat_unix: now_unix,
        battery,
        ac_online: snap.ac_online,
        ports,
        verdict: top_verdict,
        unmapped_devices: snap.unmapped_devices.iter().map(device_status).collect(),
        degraded: Degraded {
            typec: snap.typec_available,
            psy_mapping: snap.psy_mapping.as_str().to_string(),
        },
    }
}

fn build_port(p: &PortSnapshot, snap: &Snapshot, battery: &BatteryStatus) -> PortStatus {
    let (contract, measured, psy_name, usb_types, active) = match &p.psy {
        Some(psy) => (psy_contract(psy), psy_measured(psy), Some(psy.name.clone()), psy.usb_types.clone(), psy.active_usb_type.clone()),
        None => (None, None, None, Vec::new(), None),
    };

    let verdict = verdict_for(p, snap, battery, &contract, &measured);
    let warning = warning_for(p, snap, battery, &contract);

    PortStatus {
        id: p.id.clone(),
        psy: psy_name,
        location: p.location.clone(),
        partner: p.partner,
        power_role: p.power_role.clone(),
        data_role: p.data_role.clone(),
        usb_capability: p.usb_capability.clone(),
        pd_revision: p.pd_revision.clone(),
        supports_pd: p.supports_pd,
        contract,
        measured,
        usb_types,
        active_usb_type: active,
        devices: p.devices.iter().map(device_status).collect(),
        thunderbolt: p.thunderbolt.as_ref().map(|tb| TbStatus {
            generation: tb.generation,
            authorized: tb.authorized,
            name: tb.name.clone(),
        }),
        display: DisplayStatus {
            connectors: p.display_connectors.clone(),
            dp_alt_mode: p.dp_alt_mode,
        },
        identity: None,
        verdict,
        warning,
    }
}

/// Negotiated contract: `voltage_max × current_max`. Zero or missing values
/// → not measurable → `None` (never silent zeros, FR-005).
fn psy_contract(psy: &PsySnapshot) -> Option<Watts> {
    watts_pair(psy.vmax_uv, psy.imax_ua)
}

/// Live draw: `voltage_now × current_now`.
fn psy_measured(psy: &PsySnapshot) -> Option<Watts> {
    watts_pair(psy.vnow_uv, psy.inow_ua)
}

pub(crate) fn watts_pair(v_uv: Option<i64>, i_ua: Option<i64>) -> Option<Watts> {
    let (v, i) = match (v_uv, i_ua) {
        (Some(v), Some(i)) if v > 0 && i > 0 => (v, i),
        _ => return None,
    };
    let volts = round1(v as f64 / 1e6);
    let amps = round1(i as f64 / 1e6);
    let watts = round1(v as f64 * i as f64 / 1e12);
    Some(Watts { volts, amps, watts })
}

/// Priority (contract): charging > battery full > display > device >
/// supplying > nothing. Device needs an enumerated speed to render its
/// `(USB {speed})` slot; without one it degrades to the next line.
fn verdict_for(
    p: &PortSnapshot,
    snap: &Snapshot,
    battery: &BatteryStatus,
    contract: &Option<Watts>,
    measured: &Option<Watts>,
) -> String {
    let psy = p.psy.as_ref();
    let online = psy.map(|s| s.online.unwrap_or(0) == 1).unwrap_or(false);
    let status = psy.and_then(|s| s.status.as_deref());

    if online && status == Some("Charging") {
        if let Some(m) = measured {
            let w = fmt_watts(m.watts);
            let b = fmt_watts(battery.power_w.map(f64::abs).unwrap_or(0.0));
            return match contract {
                Some(c) => format!("Charging at {w}W of a {}W contract — battery taking {b}W", fmt_watts(c.watts)),
                None => format!("Charging at {w}W — battery taking {b}W"),
            };
        }
        // Charging flag but no measurable draw: fall through to lower lines.
    }

    if online && status == Some("Not charging") && snap.ac_online == Some(true) {
        return "Battery full — plugged in, not drawing".to_string();
    }

    if !p.display_connectors.is_empty() {
        return "Display connected".to_string();
    }

    if let Some(speed) = p.devices.iter().filter_map(|d| d.speed_mbps).max() {
        return format!("Device connected (USB {speed})");
    }

    if p.power_role.as_deref() == Some("source") {
        return "Supplying power to a device".to_string();
    }

    "Nothing connected".to_string()
}

/// Warning rules (contract), additive to the verdict, one slot per port:
/// 1. `"On AC but the battery is draining — charger or cable is too weak"`
///    (ac_online && battery.power_w < 0) — beats slow-charge when both apply.
/// 2. `"Slow charge: contract limited to {cw}W"` (contract.watts ≤ 15 while
///    charging).
fn warning_for(
    p: &PortSnapshot,
    snap: &Snapshot,
    battery: &BatteryStatus,
    contract: &Option<Watts>,
) -> Option<String> {
    let psy = p.psy.as_ref()?;
    let online = psy.online.unwrap_or(0) == 1;
    let charging = online && psy.status.as_deref() == Some("Charging");
    if !charging {
        return None;
    }

    if snap.ac_online == Some(true) && battery.power_w.map_or(false, |w| w < 0.0) {
        return Some(WARN_ON_AC_DRAINING.to_string());
    }
    if let Some(c) = contract {
        if c.watts <= 15.0 {
            return Some(format!("Slow charge: contract limited to {}W", fmt_watts(c.watts)));
        }
    }
    None
}

fn device_status(d: &UsbDevice) -> DeviceStatus {
    DeviceStatus {
        name: d.name.clone(),
        vendor: d.vendor.clone(),
        product: d.product.clone(),
        speed_mbps: d.speed_mbps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn snap(case: &str) -> Snapshot {
        crate::sysfs::read_snapshot(Path::new(&format!(
            "{}/fixtures/{}/sysfs",
            env!("CARGO_MANIFEST_DIR"),
            case
        )))
    }

    fn build_case(case: &str) -> Status {
        build(&snap(case), 1_760_868_000)
    }

    #[test]
    fn fmt_watts_matches_contract_examples() {
        assert_eq!(fmt_watts(65.0), "65");
        assert_eq!(fmt_watts(17.4), "17.4");
        assert_eq!(fmt_watts(100.0), "100");
        assert_eq!(fmt_watts(12.5), "12.5");
        assert_eq!(fmt_watts(0.0), "0");
        assert_eq!(fmt_watts(-2.0), "-2");
    }

    #[test]
    fn charger_charging_verdict_with_contract() {
        let st = build_case("charger-charging");
        assert_eq!(st.version, 1);
        assert_eq!(st.ports.len(), 2);
        let p0 = &st.ports[0];
        assert_eq!(p0.verdict, "Nothing connected");
        assert_eq!(p0.warning, None);
        assert_eq!(p0.identity, None);
        assert_eq!(p0.usb_types, vec!["C", "PD", "PD_PPS"]);
        assert_eq!(p0.active_usb_type.as_deref(), Some("C"));
        assert_eq!(p0.psy.as_deref(), Some("ucsi-source-psy-USBC000:001"));

        let p1 = &st.ports[1];
        let contract = p1.contract.as_ref().unwrap();
        assert_eq!(
            (contract.volts, contract.amps, contract.watts),
            (20.0, 3.0, 60.0)
        );
        let measured = p1.measured.as_ref().unwrap();
        assert_eq!((measured.volts, measured.amps, measured.watts), (20.0, 3.0, 60.0));
        assert_eq!(
            p1.verdict,
            "Charging at 60W of a 60W contract — battery taking 24W"
        );
        assert_eq!(p1.warning, None, "battery is charging, no warning fires");
        assert_eq!(st.battery.power_w, Some(24.0));
        assert_eq!(st.ac_online, Some(true));
        assert_eq!(st.degraded.typec, true);
        assert_eq!(st.degraded.psy_mapping, "verified");
        assert!(st.verdict.is_none(), "top-level verdict only in degraded case");
    }

    #[test]
    fn charging_without_contract_short_verdict() {
        let st = build_case("charging-no-contract");
        let p1 = &st.ports[1];
        assert!(p1.contract.is_none());
        assert_eq!(p1.verdict, "Charging at 10W — battery taking 24W");
    }

    #[test]
    fn idle_ports_say_nothing_connected() {
        let st = build_case("idle-no-partner");
        for p in &st.ports {
            assert_eq!(p.verdict, "Nothing connected");
            assert_eq!(p.warning, None);
        }
        assert_eq!(st.degraded.psy_mapping, "assumed");
    }

    #[test]
    fn degraded_typec_missing_emits_enum_line_and_empty_ports() {
        let st = build_case("no-typec-degraded");
        assert!(st.ports.is_empty());
        assert_eq!(st.verdict.as_deref(), Some("No USB-C data available"));
        assert_eq!(st.degraded.typec, false, "CONTRACT lines 76-77/94: false = missing");
        assert_eq!(st.degraded.psy_mapping, "unknown");
    }

    #[test]
    fn weak_charger_with_draining_battery_fires_drain_warning() {
        let st = build_case("weak-charger");
        let p1 = &st.ports[1];
        assert_eq!(
            p1.verdict,
            "Charging at 12.5W of a 15W contract — battery taking 3W"
        );
        // Both rules apply (drain + ≤15W contract); the drain alarm wins the
        // single warning slot and names the actual problem.
        assert_eq!(p1.warning.as_deref(), Some(WARN_ON_AC_DRAINING));
    }

    #[test]
    fn slow_charge_warning_at_and_below_15w() {
        let st = build_case("weak-charger-slow-only");
        let p1 = &st.ports[1];
        assert_eq!(
            p1.warning.as_deref(),
            Some("Slow charge: contract limited to 15W")
        );
        // Boundary: exactly 15 W is included (contract: `≤ 15`).
        assert_eq!(p1.contract.as_ref().unwrap().watts, 15.0);
    }

    #[test]
    fn device_verdict_shows_fastest_speed_and_unmapped_fallback() {
        let st = build_case("usb-device-attached");
        let p0 = &st.ports[0];
        assert_eq!(p0.verdict, "Device connected (USB 5000)");
        assert_eq!(p0.devices.len(), 2);
        assert_eq!(p0.devices[0].product.as_deref(), Some("SanDisk Extreme SSD"));
        assert_eq!(st.unmapped_devices.len(), 3);
    }

    #[test]
    fn display_verdict_with_tb_and_connectors() {
        let st = build_case("display-connected");
        let p0 = &st.ports[0];
        assert_eq!(p0.verdict, "Display connected");
        assert_eq!(p0.display.connectors, vec!["DP-1 connected"]);
        assert!(p0.display.dp_alt_mode);
        let tb = p0.thunderbolt.as_ref().unwrap();
        assert_eq!(tb.name.as_deref(), Some("TBT3-UDV Docking Station"));
        assert_eq!(tb.generation, Some(3));
        assert!(tb.authorized);
        // No thunderbolt key on the port without a link (skip_if none).
        let raw = serde_json::to_string(&st).unwrap();
        assert_eq!(
            raw.matches("\"thunderbolt\"").count(),
            1,
            "thunderbolt object only on the port with a TB/USB4 link"
        );
    }

    #[test]
    fn battery_full_verdict() {
        let st = build_case("battery-full");
        let p1 = &st.ports[1];
        assert_eq!(p1.verdict, "Battery full — plugged in, not drawing");
        assert!(p1.measured.is_none());
        assert!(p1.contract.is_some());
        assert_eq!(p1.warning, None, "0 W draw is not a drain");
        assert_eq!(st.battery.status.as_deref(), Some("Full"));
        assert_eq!(st.battery.power_w, Some(0.0));
    }

    #[test]
    fn supplying_verdict_for_active_source() {
        let st = build_case("supplying");
        assert_eq!(st.ports[0].verdict, "Supplying power to a device");
        assert_eq!(st.ports[1].verdict, "Nothing connected");
    }

    #[test]
    fn generic_single_port_renders_from_any_names() {
        let st = build_case("generic-single-port");
        assert_eq!(st.ports.len(), 1);
        let p0 = &st.ports[0];
        assert_eq!(
            p0.verdict,
            "Charging at 30W of a 45W contract — battery taking 10.5W"
        );
        assert_eq!(p0.psy.as_deref(), Some("tcpm-source-psy-3-0025"));
        assert_eq!(p0.location, None, "widget hides null location");
        assert_eq!(st.battery.percent, Some(65.0));
        assert_eq!(st.degraded.psy_mapping, "assumed");
    }

    #[test]
    fn unmapped_psy_renders_nulls_but_port_card_survives() {
        let st = build_case("psy-unmapped");
        let p0 = &st.ports[0];
        assert_eq!(p0.psy, None);
        assert!(p0.contract.is_none() && p0.measured.is_none());
        assert!(p0.usb_types.is_empty() && p0.active_usb_type.is_none());
        assert_eq!(st.degraded.psy_mapping, "unknown");
        // Documented enum gap: partner with no usable data falls to the
        // lowest line; the card still shows partner/roles/attrs.
        assert_eq!(p0.verdict, "Nothing connected");
    }

    #[test]
    fn serialized_field_order_puts_version_first() {
        let st = build_case("charger-charging");
        let raw = serde_json::to_string(&st).unwrap();
        assert!(raw.starts_with("{\"version\":1,\"heartbeat_unix\":"), "{raw}");
        // No thunderbolt key on link-less ports (charger fixture has none).
        assert!(!raw.contains("\"thunderbolt\""), "{raw}");

        let st = build_case("display-connected");
        let raw = serde_json::to_string(&st).unwrap();
        assert!(raw.contains("\"thunderbolt\":{\"generation\":3"), "{raw}");
    }
}
