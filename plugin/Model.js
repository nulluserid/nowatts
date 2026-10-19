.pragma library

// Pure parsing and formatting helpers for the nowatts status.json
// (CONTRACT.md v1). Presentation only: everything degrades to placeholders,
// never throws. Consumers ignore unknown fields (CONTRACT rule 5), so every
// accessor treats missing/null as "no data".

// Status colors that have no omarchy theme token (palette only defines
// accent/urgent/muted). Red/gray map to Color.urgent/Color.muted in QML.
var OK_HEX = "#7cb342"
var WARN_HEX = "#ffa000"

// CONTRACT rule 4: stale when now - heartbeat_unix > 60 s.
var STALE_S = 60

function parse(text) {
  try {
    var d = JSON.parse(String(text))
    return d && typeof d === "object" && !Array.isArray(d) ? d : null
  } catch (e) {
    return null
  }
}

function num(v, fallback) {
  // Number(null)/Number("") are 0 in JS — guard before coercion so nulls
  // stay null (contract/measured/percent rely on that).
  if (v === null || v === undefined || v === "") return fallback === undefined ? null : fallback
  var n = Number(v)
  return isFinite(n) ? n : (fallback === undefined ? null : fallback)
}

function isStale(status, nowS) {
  if (!status) return true
  var ts = num(status.heartbeat_unix, 0)
  return ts <= 0 || nowS - ts > STALE_S
}

function portsOf(status) {
  return status && Array.isArray(status.ports) ? status.ports : []
}

// CONTRACT semantics note: degraded.typec is false when the typec class is
// missing (then ports[] is []); true = data OK. The flag name reads
// backwards; CONTRACT.md text governs.
function degradedTypec(status) {
  return !!(status && status.degraded && status.degraded.typec === false)
}

function hasWarning(status) {
  var ports = portsOf(status)
  for (var i = 0; i < ports.length; i++) {
    var p = ports[i] || {}
    if (p.warning !== null && p.warning !== undefined && String(p.warning) !== "") return true
  }
  return false
}

// good = live and all-clear, warn = warning or degraded (data hole), bad =
// stale/dead heartbeat, none = no file yet.
function level(status, stale) {
  if (!status) return "none"
  if (stale) return "bad"
  if (degradedTypec(status)) return "warn"
  if (hasWarning(status)) return "warn"
  return "good"
}

// First port (in ports[] order) whose `kind` ("measured"|"contract") carries
// a numeric watts — the bar reads only from that port. null when none.
function firstPortWith(status, kind) {
  var ports = portsOf(status)
  for (var i = 0; i < ports.length; i++) {
    var p = ports[i] || {}
    var v = p[kind]
    if (v && num(v.watts) !== null) return p
  }
  return null
}

// One decimal, trailing .0 trimmed: 65.0 → "65", 17.4 → "17.4". null passes
// through as null so callers can drop the line instead of showing "0".
function trim1(n) {
  var s = n.toFixed(1)
  return s.slice(-2) === ".0" ? s.slice(0, -2) : s
}

function fmtWatts(v) {
  var n = num(v)
  return n === null ? null : trim1(Math.round(n * 10) / 10)
}

function fmtPctInt(v) {
  var n = num(v)
  return n === null ? null : Math.round(n) + "%"
}

function fmtSpeedMbps(v) {
  var n = num(v)
  if (n === null) return "—"
  if (n >= 1000) return trim1(n / 1000) + " Gbps"
  return trim1(n) + " Mbps"
}

// "left upper" → "Left upper" (SPEC P1b shows first-letter case only; a
// word-by-word titleCase would produce "Left Upper").
function fmtLocation(loc) {
  var s = String(loc || "").trim()
  if (s === "") return ""
  return s.charAt(0).toUpperCase() + s.slice(1)
}

function humanDur(s) {
  s = Math.max(0, Math.round(num(s, 0)))
  if (s < 90) return s + "s"
  if (s < 5400) return Math.floor(s / 60) + "m " + (s % 60) + "s"
  if (s < 172800) return Math.floor(s / 3600) + "h " + (Math.floor(s / 60) % 60) + "m"
  return Math.floor(s / 86400) + "d " + (Math.floor(s / 3600) % 24) + "h"
}

// One-line bar tooltip summary.
function summary(status, stale, nowS) {
  if (!status) return "nowatts — waiting for data"
  if (stale) {
    var hb = num(status.heartbeat_unix, 0)
    var ago = hb > 0
      ? " · last update " + humanDur(Math.max(0, (nowS === undefined ? Date.now() / 1000 : nowS) - hb)) + " ago"
      : ""
    return "nowatts — not responding" + ago
  }
  if (degradedTypec(status)) return "nowatts — No USB-C data available"
  var parts = []
  var b = status.battery
  if (b && b.present !== false) {
    var bp = [String(b.status || "Battery")]
    var w = fmtWatts(b.power_w)
    if (w !== null) bp.push(w + "W")
    var pct = fmtPctInt(b.percent)
    if (pct !== null) bp.push(pct)
    parts.push(bp.join(" "))
  }
  var mp = firstPortWith(status, "measured")
  if (mp) parts.push("draw " + fmtWatts(mp.measured.watts) + "W")
  if (!parts.length) {
    var first = portsOf(status)[0] || {}
    parts.push(String(first.verdict || "no data"))
  }
  return "nowatts · " + parts.join("  ·  ")
}
