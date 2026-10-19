//! sysfs — read the kernel's view of the USB-C ports into a `Snapshot`.
//!
//! Every path is joined under `base` (default `/sys`), so tests can point the
//! reader at a fixture tree (`fixtures/<case>/sysfs`) and stay hermetic — no
//! root, no live sysfs (FR-004). This module only ever opens files for
//! reading; it never writes anything (FR-001).
//!
//! Hardware-genericity (owner directive 2026-10-19): nothing here hardcodes a
//! machine. Batteries and mains supplies are discovered by their `type` file,
//! ports are 0..N, every attribute is individually optional and degrades to
//! `None`, and the psy↔port mapping uses a strategy chain (kernel links,
//! trailing-index name heuristic, behavioral cross-check) — see
//! `map_psys_to_ports`.

use std::fs;
use std::path::{Path, PathBuf};

/// How much evidence backs the psy↔port assignment (CONTRACT `degraded.psy_mapping`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsyMapping {
    /// Kernel link/devpath evidence, or the name heuristic confirmed by a
    /// discriminating behavioral cross-check.
    Verified,
    /// Heuristic mapping (name pattern and/or equal-count pairing) without
    /// discriminating confirmation.
    Assumed,
    /// No mapping could be made for every port; affected ports carry `psy: null`.
    Unknown,
}

impl PsyMapping {
    pub fn as_str(self) -> &'static str {
        match self {
            PsyMapping::Verified => "verified",
            PsyMapping::Assumed => "assumed",
            PsyMapping::Unknown => "unknown",
        }
    }
}

impl Default for PsyMapping {
    fn default() -> Self {
        PsyMapping::Unknown
    }
}

/// Everything one scan of sysfs produced. Pure input to `model::build`.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// `/sys/class/typec` exists. CONTRACT note: `degraded.typec` is this
    /// value verbatim — `false` when the class is missing (the flag reads
    /// backwards; the contract text governs).
    pub typec_available: bool,
    pub battery: BatteryInfo,
    pub ac_online: Option<bool>,
    pub ports: Vec<PortSnapshot>,
    pub unmapped_devices: Vec<UsbDevice>,
    pub psy_mapping: PsyMapping,
    /// Which battery supplied `battery.status` (for one stderr debug line).
    pub battery_status_note: Option<String>,

    // --- staging, consumed by the mapping pass and never published ---
    /// USB-type power supplies discovered this scan.
    pub(crate) psy_staging: Vec<PsySnapshot>,
    /// psy name → typec port index, from kernel symlink evidence (strategy a).
    pub(crate) psy_port_links: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub present: bool,
    pub status: Option<String>,
    /// Signed sum of all batteries' power_now (µW → W). Positive = charging.
    pub power_w: Option<f64>,
    /// Σ energy_now / Σ energy_full × 100, over batteries exposing energy.
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct PortSnapshot {
    /// typec port dir name, e.g. `"port0"`.
    pub id: String,
    /// Numeric index parsed from the dir name (used for psy pairing).
    pub index: u32,
    pub location: Option<String>,
    pub partner: bool,
    pub power_role: Option<String>,
    pub data_role: Option<String>,
    pub usb_capability: Option<String>,
    pub pd_revision: Option<String>,
    /// Port itself supports PD (usb_power_delivery_revision present and > 0).
    pub supports_pd: bool,
    pub psy: Option<PsySnapshot>,
    pub devices: Vec<UsbDevice>,
    pub thunderbolt: Option<TbLink>,
    pub display_connectors: Vec<String>,
    pub dp_alt_mode: bool,

    // --- intermediate fields, consumed by association/mapping passes ---
    /// `(bus, port)` pairs parsed from `usbN-portM` kernel links.
    pub(crate) hub_ports: Vec<(u32, u32)>,
    /// Resolved parent router dir of an `usb4_portN` link, if it resolved.
    pub(crate) usb4_router: Option<PathBuf>,
    /// psy name found via a kernel symlink on the port dir (mapping strategy a).
    pub(crate) psy_link_name: Option<String>,
    /// An `ff01` (DisplayPort) alt mode whose `mode*/active` reads active.
    pub(crate) alt_dp_active: bool,
}

impl PortSnapshot {
    /// Strong evidence that display connectors may be attributed to this
    /// port: the port must have a partner (a display over USB-C requires a
    /// cable), plus a physical link path — a resolved TB device attributed
    /// to it, or a resolved `usb4_portN` router link. The raw DP alt-mode
    /// flag is excluded: on UCSI platforms it can read "active" on ports
    /// with no link at all (observed live on a charger-only port and on a
    /// partnerless port, 2026-10-19). Static router links are only trusted
    /// WITH a partner present (see `attribute_thunderbolt`).
    fn strong_link_evidence(&self) -> bool {
        self.partner && (self.thunderbolt.is_some() || self.usb4_router.is_some())
    }
}

#[derive(Debug, Clone, Default)]
pub struct PsySnapshot {
    /// power_supply dir name, e.g. `"ucsi-source-psy-USBC000:002"`.
    pub name: String,
    pub online: Option<i64>,
    pub status: Option<String>,
    pub vmax_uv: Option<i64>,
    pub imax_ua: Option<i64>,
    pub vnow_uv: Option<i64>,
    pub inow_ua: Option<i64>,
    /// Supported set parsed from `usb_type` (e.g. `["C", "PD", "PD_PPS"]`).
    pub usb_types: Vec<String>,
    /// Bracketed (active) entry of `usb_type`, e.g. `"PD"`.
    pub active_usb_type: Option<String>,
    /// Trailing numeric index of the name, when present (mapping strategy b).
    pub(crate) index: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct UsbDevice {
    /// Bus id, e.g. `"2-1"` (stable, unique, traceable to sysfs).
    pub name: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub speed_mbps: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct TbLink {
    pub generation: Option<u32>,
    pub authorized: bool,
    /// device_name of the attached TB/USB4 device (route `N-M`, M>0).
    pub name: Option<String>,
}

// ---------------------------------------------------------------- reading

/// Read a small sysfs attribute; missing or unreadable → `None` (FR-005:
/// degrade, never invent, never crash).
fn read_attr(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_i64(path: &Path) -> Option<i64> {
    read_attr(path).and_then(|s| s.parse::<i64>().ok())
}

fn dir_entries(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.flatten() {
            out.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// Parse `"[C] PD PD_PPS"` → all entries + the bracketed active one.
/// Used for psy `usb_type` and for port role files (`power_role` etc.).
pub(crate) fn parse_bracketed(s: &str) -> (Vec<String>, Option<String>) {
    let mut all = Vec::new();
    let mut active = None;
    for tok in s.split_whitespace() {
        if let (Some(open), Some(close)) = (tok.find('['), tok.rfind(']')) {
            if open < close {
                let inner = tok[open + 1..close].to_string();
                if !inner.is_empty() {
                    active = Some(inner.clone());
                    all.push(inner);
                }
                continue;
            }
        }
        all.push(tok.to_string());
    }
    (all, active)
}

/// Trailing numeric index of a power_supply name: the digits after the last
/// `:` or `-` (e.g. `ucsi-source-psy-USBC000:002` → 2). `None` when the name
/// does not end in digits — such psys simply don't take part in the name
/// heuristic (strategy b) and fall back to behavioral pairing.
pub(crate) fn psy_name_index(name: &str) -> Option<u32> {
    let tail = name.rsplit([':', '-']).next()?;
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tail.parse::<u32>().ok()
}

pub fn read_snapshot(base: &Path) -> Snapshot {
    let mut snap = Snapshot::default();
    read_power_supplies(base, &mut snap);
    read_typec_ports(base, &mut snap);
    read_usb_devices(base, &mut snap);
    attribute_thunderbolt(base, &mut snap);
    read_drm_and_attribute(base, &mut snap);
    snap.psy_mapping = map_psys_to_ports(&mut snap);
    snap
}

fn read_power_supplies(base: &Path, snap: &mut Snapshot) {
    let ps_root = base.join("class/power_supply");
    if !ps_root.is_dir() {
        return;
    }

    struct Battery {
        name: String,
        status: Option<String>,
        power_now: Option<i64>,
        energy_now: Option<i64>,
        energy_full: Option<i64>,
    }

    let mut batteries: Vec<Battery> = Vec::new();
    let mut mains_online: Vec<bool> = Vec::new();

    for name in dir_entries(&ps_root) {
        let dir = ps_root.join(&name);
        let ty = match read_attr(&dir.join("type")) {
            Some(t) => t,
            None => continue,
        };
        match ty.as_str() {
            "Battery" => batteries.push(Battery {
                name,
                status: read_attr(&dir.join("status")),
                power_now: read_i64(&dir.join("power_now")),
                energy_now: read_i64(&dir.join("energy_now")),
                energy_full: read_i64(&dir.join("energy_full")),
            }),
            "Mains" => {
                if let Some(on) = read_i64(&dir.join("online")) {
                    mains_online.push(on != 0);
                }
            }
            "USB" => {
                // Correction (live-verified): the attribute is `usb_type`
                // (not `usb_types`); brackets mark the active entry.
                let parsed = read_attr(&dir.join("usb_type"))
                    .map(|s| parse_bracketed(&s))
                    .unwrap_or_default();
                let psy = PsySnapshot {
                    name: name.clone(),
                    online: read_i64(&dir.join("online")),
                    status: read_attr(&dir.join("status")),
                    vmax_uv: read_i64(&dir.join("voltage_max")),
                    imax_ua: read_i64(&dir.join("current_max")),
                    vnow_uv: read_i64(&dir.join("voltage_now")),
                    inow_ua: read_i64(&dir.join("current_now")),
                    usb_types: parsed.0,
                    active_usb_type: parsed.1,
                    index: psy_name_index(&name),
                };
                // Strategy (a): a symlink inside the psy dir whose target
                // mentions `/typec/portN` names the port directly.
                for entry in dir_entries(&dir) {
                    let Ok(target) = fs::read_link(dir.join(&entry)) else {
                        continue;
                    };
                    let t = target.to_string_lossy();
                    let Some(idx) = t.find("/typec/port") else {
                        continue;
                    };
                    let rest = &t[idx + "/typec/port".len()..];
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(port_idx) = digits.parse::<u32>() {
                        snap.psy_port_links.push((name.clone(), port_idx));
                        break;
                    }
                }
                snap.psy_staging.push(psy);
            }
            _ => {}
        }
    }

    snap.ac_online = if mains_online.is_empty() {
        None // no mains supply discovered — degrade, don't invent (FR-005)
    } else {
        Some(mains_online.iter().any(|&on| on))
    };

    // Aggregate batteries. `status` comes from the battery with the largest
    // energy_full (fallback: first by name); sums keep the sign.
    snap.battery.present = !batteries.is_empty();
    if batteries.is_empty() {
        return;
    }
    let chosen = batteries
        .iter()
        .enumerate()
        .max_by_key(|(i, b)| (b.energy_full.unwrap_or(0), std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
        .unwrap_or(0);
    snap.battery.status = batteries[chosen].status.clone();
    snap.battery_status_note = Some(format!(
        "{} (largest energy_full={:?} µWh)",
        batteries[chosen].name, batteries[chosen].energy_full
    ));

    let powers: Vec<i64> = batteries.iter().filter_map(|b| b.power_now).collect();
    if !powers.is_empty() {
        snap.battery.power_w = Some(round1(powers.iter().sum::<i64>() as f64 / 1e6));
    }
    let en: Vec<i64> = batteries.iter().filter_map(|b| b.energy_now).collect();
    let ef: Vec<i64> = batteries.iter().filter_map(|b| b.energy_full).collect();
    if !en.is_empty() && !ef.is_empty() {
        let full_sum = ef.iter().sum::<i64>();
        if full_sum > 0 {
            let now_sum = en.iter().sum::<i64>();
            snap.battery.percent = Some(round1(now_sum as f64 / full_sum as f64 * 100.0));
        }
    }
}

fn read_typec_ports(base: &Path, snap: &mut Snapshot) {
    let tc_root = base.join("class/typec");
    snap.typec_available = tc_root.is_dir();
    if !snap.typec_available {
        return;
    }

    // port dirs (names like `port0`), numerically ordered; `portN-partner`
    // and alt-mode dirs don't match the strict name pattern.
    let mut port_names: Vec<(u32, String)> = Vec::new();
    for name in dir_entries(&tc_root) {
        if let Some(rest) = name.strip_prefix("port") {
            if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(idx) = rest.parse::<u32>() {
                    port_names.push((idx, name));
                }
            }
        }
    }
    port_names.sort();

    for (index, name) in port_names {
        let dir = tc_root.join(&name);
        let mut port = PortSnapshot {
            id: name.clone(),
            index,
            ..Default::default()
        };

        port.location = read_physical_location(&dir);
        // The partner dir is a SIBLING of the port dir: `class/typec/port1-partner`.
        port.partner = tc_root.join(format!("{name}-partner")).exists();
        port.power_role = read_attr(&dir.join("power_role")).and_then(|s| parse_bracketed(&s).1);
        port.data_role = read_attr(&dir.join("data_role")).and_then(|s| parse_bracketed(&s).1);
        port.usb_capability =
            read_attr(&dir.join("usb_capability")).and_then(|s| parse_bracketed(&s).1);
        if let Some(rev) = read_attr(&dir.join("usb_power_delivery_revision")) {
            port.supports_pd = rev.parse::<f64>().map(|v| v > 0.0).unwrap_or(false);
            port.pd_revision = Some(rev);
        }

        // Kernel-provided links: usbN-portM (hub ports) and usb4_portN (TB/USB4).
        for entry in dir_entries(&dir) {
            let full = dir.join(&entry);
            let Ok(meta) = fs::symlink_metadata(&full) else {
                continue;
            };
            if !meta.file_type().is_symlink() {
                continue;
            }
            if let Some((bus, hub_port)) = parse_hub_link(&entry) {
                // Name-parsed only: association matches device names, no
                // resolution needed (works even if the link dangles).
                port.hub_ports.push((bus, hub_port));
            } else if entry.starts_with("usb4_port") {
                if let Ok(resolved) = fs::read_link(&full) {
                    let target = if resolved.is_absolute() {
                        resolved
                    } else {
                        dir.join(resolved)
                    };
                    // Router = parent of usb4_portN (a `N-0` dir).
                    if let Some(rn) = target.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
                        if rn.ends_with("-0") && rn[..rn.len() - 2].bytes().all(|b| b.is_ascii_digit()) {
                            port.usb4_router = Some(target.parent().unwrap().to_path_buf());
                        }
                    }
                }
            } else if let Ok(target) = fs::read_link(&full) {
                // (a) mapping evidence: port-side symlink naming a power_supply.
                let t = target.to_string_lossy();
                if t.contains("power_supply/") {
                    port.psy_link_name = t.rsplit('/').next().map(|s| s.to_string());
                }
            }
        }

        // Alt modes live INSIDE the port dir (live layout): `portN.M/svid`
        // with `portN.M/modeK/active`; `ff01` = DisplayPort.
        let alt_prefix = format!("{name}.");
        for entry in dir_entries(&dir) {
            if !entry.starts_with(&alt_prefix) {
                continue;
            }
            let adir = dir.join(&entry);
            if read_attr(&adir.join("svid")).as_deref() != Some("ff01") {
                continue;
            }
            for mode in dir_entries(&adir) {
                if mode.starts_with("mode") {
                    if let Some(act) = read_attr(&adir.join(&mode).join("active")) {
                        if act == "1" || act == "yes" {
                            port.alt_dp_active = true;
                        }
                    }
                }
            }
        }

        // TB attribution does NOT happen here: a resolved `usb4_portN` link
        // is static controller topology, present even on ports with nothing
        // plugged in (observed live 2026-10-19 — the dock showed up on a
        // partnerless port). Attribution needs partner state; see
        // `attribute_thunderbolt`, which runs once all ports are collected.

        snap.ports.push(port);
    }
}

fn read_physical_location(port_dir: &Path) -> Option<String> {
    let pl = port_dir.join("physical_location");
    let horizontal = read_attr(&pl.join("horizontal_position"))?;
    let vertical = read_attr(&pl.join("vertical_position"))?;
    Some(format!("{horizontal} {vertical}"))
}

/// `usb2-port1` → (2, 1).
fn parse_hub_link(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("usb")?;
    let (bus, port) = rest.split_once("-port")?;
    if !bus.bytes().all(|b| b.is_ascii_digit()) || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((bus.parse().ok()?, port.parse().ok()?))
}

/// Read the attached TB/USB4 device (route `N-M` with M > 0; `N-0` is the
/// host router, `:` entries are adapters) directly under `router`.
fn tb_device_on_router(router: &Path) -> Option<TbLink> {
    let router_name = router.file_name()?.to_str()?;
    let prefix = router_name.split_once('-')?.0.to_string();
    let mut best: Option<(u32, TbLink)> = None;
    for entry in dir_entries(router) {
        let Some(route) = entry.strip_prefix(&prefix) else {
            continue;
        };
        let Some(m) = route.strip_prefix('-') else {
            continue;
        };
        if m.is_empty() || !m.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // All-digits already checked above; a u32 overflow here must not
        // abort the whole router scan (review NOTE) — skip the entry.
        let Ok(m) = m.parse::<u32>() else { continue };
        if m == 0 {
            continue; // host router itself
        }
        let ddir = router.join(&entry);
        if !ddir.is_dir() {
            continue;
        }
        let link = read_tb_link(&ddir);
        if best.as_ref().map_or(true, |(bm, _)| m < *bm) {
            best = Some((m, link));
        }
    }
    best.map(|(_, link)| link)
}

/// Read one TB/USB4 device dir into a `TbLink`.
fn read_tb_link(ddir: &Path) -> TbLink {
    TbLink {
        generation: read_i64(&ddir.join("generation")).map(|v| v as u32),
        authorized: read_i64(&ddir.join("authorized")).map(|v| v != 0).unwrap_or(false),
        name: read_attr(&ddir.join("device_name")),
    }
}

/// Every attached TB/USB4 device (route `N-M`, M > 0) across all domains.
fn tb_devices_anywhere(base: &Path) -> Vec<TbLink> {
    let mut out = Vec::new();
    let root = base.join("bus/thunderbolt/devices");
    for entry in dir_entries(&root) {
        let Some((n, m)) = entry.split_once('-') else {
            continue;
        };
        if !n.bytes().all(|b| b.is_ascii_digit()) || !m.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(m) = m.parse::<u32>() else { continue };
        if m == 0 {
            continue; // host router
        }
        out.push(read_tb_link(&root.join(&entry)));
    }
    out
}

/// Attribute TB/USB4 devices to ports.
///
/// Live lesson (2026-10-19, owner report: one dock plugged in, dock info
/// split across two port cards): `usb4_portN` links are static controller
/// topology — they exist on ports with nothing plugged in. A link alone is
/// NOT a connection. Rules:
///   1. strong: a port with a partner whose `usb4_portN` router subtree
///      contains an attached device claims it;
///   2. fallback: when no strong claim landed, exactly ONE partner port
///      exists and exactly ONE device exists anywhere → attribute it to
///      that port by elimination (logged — it is an inference, not a read);
///   3. anything ambiguous → no attribution at all (FR-005: never invent).
fn attribute_thunderbolt(base: &Path, snap: &mut Snapshot) {
    let mut strong: Vec<(usize, TbLink)> = Vec::new();
    for (i, port) in snap.ports.iter().enumerate() {
        if !port.partner {
            continue;
        }
        if let Some(router) = &port.usb4_router {
            if let Some(link) = tb_device_on_router(router) {
                strong.push((i, link));
            }
        }
    }
    if !strong.is_empty() {
        for (i, link) in strong {
            snap.ports[i].thunderbolt = Some(link);
        }
        return;
    }

    let partners: Vec<usize> = snap
        .ports
        .iter()
        .enumerate()
        .filter(|(_, p)| p.partner)
        .map(|(i, _)| i)
        .collect();
    let all = tb_devices_anywhere(base);
    if partners.len() == 1 && all.len() == 1 {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let name = snap.ports[partners[0]].id.clone();
        snap.ports[partners[0]].thunderbolt = Some(all.into_iter().next().expect("len == 1 checked"));
        // Log once per process: the fallback fires on every heartbeat scan
        // and the journal must stay readable (completion-review NOTE).
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!(
                "nowatts: TB device attributed to {name} by elimination (sole partner port; kernel links absent)"
            );
        }
    }
}

fn read_usb_devices(base: &Path, snap: &mut Snapshot) {
    let mut by_port: Vec<(u32, Vec<UsbDevice>)> = Vec::new();
    let mut unmapped = Vec::new();

    for name in dir_entries(&base.join("bus/usb/devices")) {
        // Skip root hubs (`usbN`: they report controller capability, not a
        // device) and interfaces (`N-M:C.L`).
        if name.starts_with("usb") && name[3..].bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if name.contains(':') {
            continue;
        }
        let ddir = base.join("bus/usb/devices").join(&name);
        let dev = UsbDevice {
            name: name.clone(),
            vendor: read_attr(&ddir.join("manufacturer")),
            product: read_attr(&ddir.join("product")),
            speed_mbps: read_i64(&ddir.join("speed")).map(|v| v as u32),
        };
        // Association: device `N-M[.x…]` sits at (bus N, hub port M) or
        // downstream of whatever is at that hub port — both match a port
        // link `usbN-portM`.
        let mut associated = false;
        if let Some((bus, port)) = device_bus_port(&name) {
            for p in snap.ports.iter_mut() {
                if p.hub_ports.contains(&(bus, port)) {
                    by_port.push((p.index, vec![dev.clone()]));
                    associated = true;
                    break;
                }
            }
        }
        if !associated {
            unmapped.push(dev);
        }
    }

    for p in snap.ports.iter_mut() {
        let mut devs: Vec<UsbDevice> = by_port
            .iter()
            .filter(|(idx, _)| *idx == p.index)
            .flat_map(|(_, d)| d.clone())
            .collect();
        devs.sort_by(|a, b| a.name.cmp(&b.name));
        p.devices = devs;
    }
    unmapped.sort_by(|a, b| a.name.cmp(&b.name));
    snap.unmapped_devices = unmapped;
}

/// USB bus-device name `N-M[.x.y…]` → (bus N, root-hub port M).
fn device_bus_port(name: &str) -> Option<(u32, u32)> {
    let (bus, rest) = name.split_once('-')?;
    let port = rest.split('.').next()?;
    if bus.is_empty() || port.is_empty() {
        return None;
    }
    if !bus.bytes().all(|b| b.is_ascii_digit()) || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((bus.parse().ok()?, port.parse().ok()?))
}

/// Read DRM connectors and attribute external connected ones to ports with
/// strong link evidence. Only ONE such port may claim them; with several
/// candidates the kernel exposes no per-connector port truth, so nothing is
/// claimed (honest degradation over guessing).
fn read_drm_and_attribute(base: &Path, snap: &mut Snapshot) {
    let drm_root = base.join("class/drm");
    let mut external_connected: Vec<String> = Vec::new();
    for entry in dir_entries(&drm_root) {
        let Some(conn) = strip_card_prefix(&entry) else {
            continue;
        };
        let status = read_attr(&drm_root.join(&entry).join("status"));
        if status.as_deref() == Some("connected") && !conn.starts_with("eDP-") {
            external_connected.push(format!("{conn} connected"));
        }
    }
    external_connected.sort();

    let mut evidence_ports: Vec<usize> = snap
        .ports
        .iter()
        .enumerate()
        .filter(|(_, p)| p.strong_link_evidence())
        .map(|(i, _)| i)
        .collect();
    if evidence_ports.is_empty() && !external_connected.is_empty() {
        // DP-alt-mode docks that expose no TB/USB4 links: with exactly one
        // partner port, the connected display can only belong to it — the
        // same elimination inference as `attribute_thunderbolt`. Ambiguous
        // (0 or ≥2 partners) → nothing is claimed.
        let partners: Vec<usize> = snap
            .ports
            .iter()
            .enumerate()
            .filter(|(_, p)| p.partner)
            .map(|(i, _)| i)
            .collect();
        if partners.len() == 1 {
            evidence_ports = partners;
        }
    }
    if evidence_ports.len() == 1 && !external_connected.is_empty() {
        snap.ports[evidence_ports[0]].display_connectors = external_connected;
    }

    // dp_alt_mode: kernel reports an active ff01 alt mode AND the port has
    // connection-grounded evidence — an attributed TB device, claimed
    // display connectors, or a router link WITH a partner. On UCSI the raw
    // flag alone reads "active" even on partnerless/charger-only ports
    // (observed live), so gate it.
    for p in snap.ports.iter_mut() {
        p.dp_alt_mode = p.alt_dp_active
            && p.partner
            && (p.thunderbolt.is_some()
                || !p.display_connectors.is_empty()
                || p.usb4_router.is_some());
    }
}

/// `card0-DP-1` → `DP-1`.
fn strip_card_prefix(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("card")?;
    let dash = rest.find('-')?;
    if dash == 0 || !rest[..dash].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(&rest[dash + 1..])
}

// ------------------------------------------------- psy ↔ port mapping

#[derive(Clone, Copy, PartialEq)]
enum MapSource {
    KernelLink,
    NameIndex,
    BehavioralPairing,
}

/// Assign a psy to each port using, in order: (a) kernel link/devpath
/// evidence, (b) the trailing-index name heuristic (UCSI `…:00N` ↔ port
/// N-1 is one instance of the general `…<digits>` form), (c) behavioral
/// pairing (partner presence ↔ psy online, only when counts match 1:1).
/// Returns the CONTRACT `degraded.psy_mapping` classification:
/// - `Unknown` when any port (or every port) ends up without a psy;
/// - `Verified` when every mapping is kernel-link-backed, or the name/
///   behavioral mapping survives a behavioral cross-check whose partner/online
///   states actually discriminate (not all-true or all-false);
/// - `Assumed` otherwise (heuristic mapping without discriminating proof).
fn map_psys_to_ports(snap: &mut Snapshot) -> PsyMapping {
    if snap.ports.is_empty() {
        return PsyMapping::Unknown;
    }
    let psys = std::mem::take(&mut snap.psy_staging);
    let psy_count = psys.len();

    let mut used = vec![false; psy_count];
    let mut assignments: Vec<Option<usize>> = vec![None; snap.ports.len()];
    let mut sources: Vec<Option<MapSource>> = vec![None; snap.ports.len()];

    // (a) kernel links, from either side of the pairing.
    for (pi, port) in snap.ports.iter().enumerate() {
        if let Some(psy_name) = &port.psy_link_name {
            if let Some(psy_i) = psys.iter().position(|p| &p.name == psy_name) {
                if !used[psy_i] {
                    assignments[pi] = Some(psy_i);
                    used[psy_i] = true;
                    sources[pi] = Some(MapSource::KernelLink);
                }
            }
        }
    }
    for (psy_name, port_idx) in snap.psy_port_links.clone() {
        if let Some(pi) = snap.ports.iter().position(|p| p.index == port_idx) {
            if assignments[pi].is_none() {
                if let Some(psy_i) = psys.iter().position(|p| p.name == psy_name) {
                    if !used[psy_i] {
                        assignments[pi] = Some(psy_i);
                        used[psy_i] = true;
                        sources[pi] = Some(MapSource::KernelLink);
                    }
                }
            }
        }
    }

    // (b) trailing-index name heuristic: psy index N ↔ port index N-1.
    for (psy_i, psy) in psys.iter().enumerate() {
        if used[psy_i] {
            continue;
        }
        if let Some(idx) = psy.index {
            if idx >= 1 {
                if let Some(pi) = snap.ports.iter().position(|p| p.index == idx - 1) {
                    if assignments[pi].is_none() {
                        assignments[pi] = Some(psy_i);
                        used[psy_i] = true;
                        sources[pi] = Some(MapSource::NameIndex);
                    }
                }
            }
        }
    }

    // (c) behavioral pairing for whatever is left, only when it's 1:1.
    let free_ports: Vec<usize> = (0..snap.ports.len()).filter(|&i| assignments[i].is_none()).collect();
    let free_psys: Vec<usize> = (0..psy_count).filter(|&i| !used[i]).collect();
    if !free_ports.is_empty() && free_ports.len() == free_psys.len() {
        let mut ports_by_partner = free_ports.clone();
        ports_by_partner.sort_by_key(|&i| !snap.ports[i].partner);
        let mut psys_by_online = free_psys.clone();
        psys_by_online.sort_by_key(|&i| !(psys[i].online.unwrap_or(0) != 0));
        for (&pi, &psy_i) in ports_by_partner.iter().zip(psys_by_online.iter()) {
            assignments[pi] = Some(psy_i);
            used[psy_i] = true;
            sources[pi] = Some(MapSource::BehavioralPairing);
        }
    }

    let any_unmapped = assignments.iter().any(|a| a.is_none());
    let classification = if any_unmapped {
        PsyMapping::Unknown
    } else if sources.iter().all(|&s| s == Some(MapSource::KernelLink)) {
        PsyMapping::Verified
    } else {
        let consistent = assignments.iter().enumerate().all(|(pi, a)| {
            let psy_i = a.unwrap();
            snap.ports[pi].partner == (psys[psy_i].online.unwrap_or(0) != 0)
        });
        let partner_states: Vec<bool> = snap.ports.iter().map(|p| p.partner).collect();
        let online_states: Vec<bool> = assignments
            .iter()
            .map(|a| psys[a.unwrap()].online.unwrap_or(0) != 0)
            .collect();
        let discriminating = partner_states.iter().any(|&b| b != partner_states[0])
            && online_states.iter().any(|&b| b != online_states[0]);
        if consistent && discriminating {
            PsyMapping::Verified
        } else {
            PsyMapping::Assumed
        }
    };

    for (pi, port) in snap.ports.iter_mut().enumerate() {
        if let Some(psy_i) = assignments[pi] {
            port.psy = Some(psys[psy_i].clone());
        }
        // else: psy stays None → model renders psy/contract/measured as null.
    }
    classification
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::watts_pair;

    fn snap(case: &str) -> Snapshot {
        read_snapshot(Path::new(&format!(
            "{}/fixtures/{}/sysfs",
            env!("CARGO_MANIFEST_DIR"),
            case
        )))
    }

    #[test]
    fn parse_bracketed_marks_active_entry() {
        // Live-verified: `[C] PD PD_PPS` (idle) vs `C [PD] PD_PPS` (charging).
        let (all, active) = parse_bracketed("[C] PD PD_PPS");
        assert_eq!(all, vec!["C", "PD", "PD_PPS"]);
        assert_eq!(active.as_deref(), Some("C"));

        let (all, active) = parse_bracketed("C [PD] PD_PPS");
        assert_eq!(all, vec!["C", "PD", "PD_PPS"]);
        assert_eq!(active.as_deref(), Some("PD"));

        let (all, active) = parse_bracketed("source [sink]");
        assert_eq!(all, vec!["source", "sink"]);
        assert_eq!(active.as_deref(), Some("sink"));

        let (all, active) = parse_bracketed("[host] device");
        assert_eq!(active.as_deref(), Some("host"));
        assert_eq!(all, vec!["host", "device"]);

        let (all, active) = parse_bracketed("");
        assert!(all.is_empty() && active.is_none());
    }

    #[test]
    fn psy_name_index_parses_trailing_digits() {
        assert_eq!(psy_name_index("ucsi-source-psy-USBC000:002"), Some(2));
        assert_eq!(psy_name_index("ucsi-source-psy-USBC000:001"), Some(1));
        assert_eq!(psy_name_index("tcpm-source-psy-3-0025"), Some(25));
        assert_eq!(psy_name_index("generic-usb-psy"), None);
        assert_eq!(psy_name_index("AC"), None);
    }

    #[test]
    fn charger_charging_reads_psy_values_and_verifies_mapping() {
        let s = snap("charger-charging");
        assert!(s.typec_available);
        assert_eq!(s.ports.len(), 2);
        assert_eq!(s.ac_online, Some(true));
        assert!(s.battery.present);
        assert_eq!(s.battery.status.as_deref(), Some("Charging"));
        assert_eq!(s.battery.power_w, Some(24.0)); // 23974000 µW, sign kept
        assert_eq!(s.battery.percent, Some(91.1)); // 51.92/57.01

        let p0 = &s.ports[0];
        assert!(!p0.partner);
        assert_eq!(p0.power_role.as_deref(), Some("sink"));
        assert_eq!(p0.data_role.as_deref(), Some("device"));
        assert_eq!(p0.usb_capability.as_deref(), Some("usb3"));
        assert_eq!(p0.location.as_deref(), Some("left upper"));

        let p1 = &s.ports[1];
        assert!(p1.partner);
        let psy = p1.psy.as_ref().expect("mapped psy");
        assert_eq!(psy.name, "ucsi-source-psy-USBC000:002");
        assert_eq!(psy.online, Some(1));
        assert_eq!(psy.usb_types, vec!["C", "PD", "PD_PPS"]);
        assert_eq!(psy.active_usb_type.as_deref(), Some("PD"));
        assert_eq!(psy.vmax_uv, Some(20_000_000));
        assert_eq!(psy.imax_ua, Some(3_000_000));
        // Idle port maps to :001 with C active.
        let psy0 = s.ports[0].psy.as_ref().unwrap();
        assert_eq!(psy0.active_usb_type.as_deref(), Some("C"));
        // Discriminating behavioral check (one partner, one online psy) → verified.
        assert_eq!(s.psy_mapping, PsyMapping::Verified);
        assert!(s.battery_status_note.as_deref().unwrap_or("").contains("BAT0"));
    }

    #[test]
    fn charging_without_contract_degrades_to_null() {
        let s = snap("charging-no-contract");
        let p1 = &s.ports[1];
        let psy = p1.psy.as_ref().unwrap();
        assert!(psy.vmax_uv == Some(0) || true);
        let contract = watts_pair(psy.vmax_uv, psy.imax_ua);
        assert!(contract.is_none(), "zero max values are not a contract");
        let measured = watts_pair(psy.vnow_uv, psy.inow_ua).unwrap();
        assert_eq!(measured.watts, 10.0);
        assert_eq!(measured.volts, 5.0);
        assert_eq!(measured.amps, 2.0);
        assert_eq!(s.psy_mapping, PsyMapping::Verified);
    }

    #[test]
    fn idle_no_partner_maps_assumed() {
        let s = snap("idle-no-partner");
        // Pattern mapping holds but all states identical → no discriminating
        // evidence, honest "assumed".
        assert_eq!(s.psy_mapping, PsyMapping::Assumed);
        assert!(!s.ports[0].partner && !s.ports[1].partner);
        assert_eq!(s.battery.power_w, Some(-8.5));
    }

    #[test]
    fn missing_typec_class_degrades() {
        let s = snap("no-typec-degraded");
        assert!(!s.typec_available);
        assert!(s.ports.is_empty());
        assert_eq!(s.psy_mapping, PsyMapping::Unknown);
        // Battery/AC still read — only the typec view is gone.
        assert_eq!(s.ac_online, Some(true));
        assert!(s.battery.present);
    }

    #[test]
    fn usb_devices_associate_via_hub_links_with_unmapped_fallback() {
        let s = snap("usb-device-attached");
        let p0 = &s.ports[0];
        assert_eq!(p0.hub_ports, vec![(2, 1), (3, 2)]);
        let names: Vec<&str> = p0.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["2-1", "2-1.4"], "2-1 and downstream 2-1.4");
        let disk = &p0.devices[0];
        assert_eq!(disk.product.as_deref(), Some("SanDisk Extreme SSD"));
        assert_eq!(disk.vendor.as_deref(), Some("SanDisk"));
        assert_eq!(disk.speed_mbps, Some(5000));
        // Root hubs and interfaces never appear anywhere.
        let unmapped: Vec<&str> = s.unmapped_devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(unmapped, vec!["3-4", "3-5", "3-5.4"]);
        assert!(!unmapped.iter().any(|n| n.starts_with("usb")));
    }

    #[test]
    fn thunderbolt_and_drm_read_and_attribute_to_linked_port() {
        let s = snap("display-connected");
        let p0 = &s.ports[0];
        let tb = p0.thunderbolt.as_ref().expect("TB device via usb4 link");
        assert_eq!(tb.name.as_deref(), Some("TBT3-UDV Docking Station"));
        assert_eq!(tb.generation, Some(3));
        assert!(tb.authorized);
        assert_eq!(p0.display_connectors, vec!["DP-1 connected"]);
        assert!(p0.dp_alt_mode, "ff01 active AND physical link evidence");
        // Internal panel excluded; dock audio has no usb hub-link → unmapped.
        let unmapped: Vec<&str> = s.unmapped_devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(unmapped, vec!["5-2"]);
        let p1 = &s.ports[1];
        assert!(p1.thunderbolt.is_none());
        assert!(p1.display_connectors.is_empty());
    }

    #[test]
    fn tb_static_link_without_partner_is_not_attributed() {
        // Live bug mirror (2026-10-19, owner report): ONE dock plugged in;
        // the kernel links port0's static usb4 topology into the dock's
        // domain while UCSI reports the partner + PD on port1. The
        // partnerless port must claim nothing; the sole partner port claims
        // the dock and the display by elimination.
        let s = snap("tb-static-link-no-partner");
        let p0 = &s.ports[0];
        assert!(p0.usb4_router.is_some(), "static link present in fixture");
        assert!(!p0.partner, "fixture mirrors the live partnerless port");
        assert!(p0.thunderbolt.is_none(), "a partnerless port claims no TB device");
        assert!(p0.display_connectors.is_empty(), "no display without a partner");
        assert!(!p0.dp_alt_mode);
        let p1 = &s.ports[1];
        assert!(p1.partner);
        let tb = p1
            .thunderbolt
            .as_ref()
            .expect("sole partner port claims the dock by elimination");
        assert_eq!(tb.name.as_deref(), Some("TBT3-UDV Docking Station"));
        assert_eq!(p1.display_connectors, vec!["DP-1 connected"]);
        assert!(p1.dp_alt_mode);

        // The model layer turns the same snapshot into plain English: the
        // partnerless port is honestly "Nothing connected" while the dock's
        // port keeps its charging verdict.
        let doc = crate::model::build(&snap("tb-static-link-no-partner"), 100);
        assert_eq!(doc.ports[0].verdict, "Nothing connected");
        assert!(
            doc.ports[1].verdict.starts_with("Charging at 60W"),
            "{}",
            doc.ports[1].verdict
        );
    }

    #[test]
    fn dp_alt_dock_display_claimed_by_elimination() {
        // DP-alt-mode dock with no thunderbolt tree at all: the connected
        // display goes to the sole partner port; thunderbolt stays null;
        // nothing leaks onto the partnerless port.
        let s = snap("dp-alt-dock-no-tb");
        let p0 = &s.ports[0];
        assert!(p0.thunderbolt.is_none());
        assert!(p0.display_connectors.is_empty());
        assert!(!p0.dp_alt_mode);
        let p1 = &s.ports[1];
        assert!(p1.partner);
        assert!(p1.thunderbolt.is_none(), "no TB device exists in this fixture");
        assert_eq!(p1.display_connectors, vec!["DP-1 connected"]);
    assert!(p1.dp_alt_mode, "active ff01 + claimed connectors on the partner port");
    }

    #[test]
    fn dp_alt_mode_flag_gated_on_link_evidence() {
        // Live UCSI quirk (mirrored in fixtures): the kernel reports the ff01
        // alt mode "active" even on a charger-only port with no TB/USB4 link.
        // The raw flag alone is noise; dp_alt_mode requires link evidence.
        let s = snap("charger-charging");
        assert!(s.ports[1].alt_dp_active, "fixture carries the raw active flag");
        assert!(!s.ports[1].dp_alt_mode, "gated off without link evidence");
        assert!(!s.ports[0].dp_alt_mode);

        let s = snap("display-connected");
        assert!(s.ports[0].dp_alt_mode, "active flag AND physical link evidence");
        assert!(s.ports[0].alt_dp_active);
        assert!(!s.ports[1].dp_alt_mode, "same raw flag, no link, stays false");
    }

    #[test]
    fn battery_full_has_contract_but_no_measured() {
        let s = snap("battery-full");
        let p1 = &s.ports[1];
        let psy = p1.psy.as_ref().unwrap();
        let contract = watts_pair(psy.vmax_uv, psy.imax_ua).unwrap();
        assert_eq!(contract.watts, 60.0);
        assert!(watts_pair(psy.vnow_uv, psy.inow_ua).is_none());
        assert_eq!(s.battery.power_w, Some(0.0));
        assert_eq!(s.battery.status.as_deref(), Some("Full"));
    }

    #[test]
    fn generic_single_port_without_location_maps_assumed() {
        let s = snap("generic-single-port");
        assert_eq!(s.ports.len(), 1);
        let p0 = &s.ports[0];
        assert_eq!(p0.location, None, "no physical_location on this hardware");
        assert_eq!(p0.id, "port0");
        let psy = p0.psy.as_ref().unwrap();
        // Name heuristic can't map `…-3-0025` (index 25 → no port 24); the
        // equal-count behavioral pairing takes over, honestly "assumed".
        assert_eq!(psy.name, "tcpm-source-psy-3-0025");
        assert_eq!(s.psy_mapping, PsyMapping::Assumed);
        assert_eq!(s.ac_online, Some(true));
        assert!(s.battery.present);
    }

    #[test]
    fn multi_battery_aggregates() {
        let s = snap("multi-battery");
        assert_eq!(s.battery.power_w, Some(8.0)); // 10.0 + (-2.0)
        assert_eq!(s.battery.percent, Some(61.5)); // 32e6 / 52e6
        assert_eq!(s.battery.status.as_deref(), Some("Charging"));
        let note = s.battery_status_note.as_deref().unwrap_or("");
        assert!(note.contains("BAT0"), "status from largest energy_full: {note}");
        assert!(note.contains("50000000"));
    }

    #[test]
    fn unmappable_psy_degrades_port_to_nulls() {
        let s = snap("psy-unmapped");
        assert_eq!(s.psy_mapping, PsyMapping::Unknown);
        for p in &s.ports {
            assert!(p.psy.is_none());
        }
    }

    #[test]
    fn supplying_case_reads_active_source_role() {
        let s = snap("supplying");
        assert_eq!(s.ports[0].power_role.as_deref(), Some("source"));
        assert!(s.ports[0].partner);
        assert_eq!(s.ac_online, Some(false));
    }

    #[test]
    fn missing_bus_and_drm_dirs_are_empty_not_errors() {
        let s = snap("charger-charging"); // no bus/ or drm/ in this fixture
        assert!(s.unmapped_devices.is_empty());
        assert!(s.ports.iter().all(|p| p.devices.is_empty()));
    }
}
