import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Bar readout (plug glyph + live watts per the "format" setting) and host
// for the detail panel. Reads ONLY ~/.local/state/nowatts/status.json
// (CONTRACT.md v1) — read-only, no subprocesses (FR-006). A missing or
// corrupt file behaves like stale-with-no-data, never a crash (FR-003);
// stale = heartbeat_unix older than 60 s (CONTRACT rule 4).
BarWidget {
  id: root
  moduleName: "legend.nowatts"

  readonly property string glyph: "\udb81\udee5"  // nf-md-power_plug — verified present (U+F06E5) in JetBrainsMono Nerd Font; same nf-md set as Dishy's nf-md-satellite_uplink
  readonly property string stateDir: Quickshell.env("HOME") + "/.local/state/nowatts"

  // Parsed status.json (null = missing/corrupt) and staleness flag.
  // (Not named `data`: that would shadow Item.data.)
  property var statusData: null
  property bool stale: true

  readonly property string level: Model.level(root.statusData, root.stale)
  readonly property color stateColor: level === "good" ? Model.OK_HEX
    : level === "warn" ? Model.WARN_HEX
    : level === "bad" ? Color.urgent : Color.muted

  // Bar value per the manifest "format" setting: watts → measured W from
  // ports[], contract → contract W, icon → glyph only. Numbers are NEVER
  // shown while stale (CONTRACT rule 4: don't keep displaying old watts as
  // if live) — the value collapses to glyph-only and the state dot carries
  // the stale marker instead.
  readonly property string value: {
    if (!root.statusData || root.stale) return ""
    var format = String(root.setting("format", "watts"))
    if (format === "icon") return ""
    var kind = format === "contract" ? "contract" : "measured"
    var port = Model.firstPortWith(root.statusData, kind)
    if (!port) return ""
    var w = Model.fmtWatts(port[kind].watts)
    return w === null ? "" : w + "W"
  }

  readonly property string label: root.glyph + (root.value !== "" ? " " + root.value : "")

  // show=auto: mute (dim, not remove — the panel stays reachable) when
  // nothing is connected: every port says partner === false AND AC offline.
  // A degraded readout (typec data missing, ports[]) never counts as idle —
  // explicit degradation must stay visible (FR-005) — and stale/no-data
  // stays visible so a dead daemon is noticeable.
  readonly property bool idleWhenAuto: {
    if (!root.statusData || root.stale) return false
    if (Model.degradedTypec(root.statusData)) return false
    if (root.statusData.ac_online !== false) return false
    var ports = Model.portsOf(root.statusData)
    for (var i = 0; i < ports.length; i++) {
      var p = ports[i] || {}
      if (p.partner !== false) return false
    }
    return true
  }
  readonly property bool muted: String(root.setting("show", "auto")) === "auto" && root.idleWhenAuto

  function tick() {
    var parsed = null
    try {
      statusFile.reload()
      parsed = Model.parse(statusFile.text())
    } catch (e) {
      parsed = null
    }
    root.statusData = parsed
    root.stale = Model.isStale(parsed, Date.now() / 1000)
  }

  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false

  function open() { if (panelLoader.item) panelLoader.item.open() }
  function close() { if (panelLoader.item) panelLoader.item.close() }
  function togglePanel() { if (panelLoader.item) panelLoader.item.toggle() }

  readonly property bool popoutSwitchClosing: panelLoader.item ? panelLoader.item.popoutSwitchClosing === true : false
  function closeForPopoutSwitch() { if (panelLoader.item) panelLoader.item.closeForPopoutSwitch() }

  function injectPanel() {
    var target = panelLoader.item
    if (!target) return
    if ("bar" in target) target.bar = root.bar
    if ("settings" in target) target.settings = root.settings
    if ("anchorItem" in target) target.anchorItem = button
    if ("hostWidget" in target) target.hostWidget = root
    if ("status" in target) target.status = root
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Timer {
    // Manifest clamps interval to 500–10000 ms; the clamp keeps a bad
    // settings value from turning into a busy loop.
    interval: Math.min(10000, Math.max(500, Number(root.setting("interval", 1000)) || 1000))
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.tick()
  }

  FileView { id: statusFile; path: root.stateDir + "/status.json"; blockLoading: true }

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("Panel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  IpcHandler {
    target: "nowatts"

    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.togglePanel() }

    // Vision-free introspection: exactly what this widget renders right
    // now, as JSON. The panel/binder tests call this over the shell IPC.
    function status(): string {
      return JSON.stringify({
        label: String(root.label),
        value: String(root.value),
        muted: !!root.muted,
        stale: !!root.stale,
        level: String(root.level),
        opened: !!root.opened,
        ports: root.statusData && Array.isArray(root.statusData.ports)
          ? root.statusData.ports.length : -1,
        verdict0: (root.statusData && Array.isArray(root.statusData.ports)
            && root.statusData.ports.length > 0 && (root.statusData.ports[0] || {}).verdict)
          ? String(root.statusData.ports[0].verdict) : "",
        unmapped: root.statusData && Array.isArray(root.statusData.unmapped_devices)
          ? root.statusData.unmapped_devices.length : -1
      })
    }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.label
    dimmed: root.muted
    tooltipText: Model.summary(root.statusData, root.stale, Date.now() / 1000)
    horizontalMargin: 8.75
    onPressed: function(b) {
      if (b === Qt.LeftButton) root.togglePanel()
    }
  }

  // State dot: green live, amber warning/degraded, red stale (daemon not
  // responding or no heartbeat), gray waiting for the first file.
  Rectangle {
    id: stateDot
    visible: true
    anchors.top: button.top
    anchors.right: button.right
    anchors.margins: 2
    z: 10

    implicitWidth: Style.space(8)
    implicitHeight: Style.space(8)
    radius: height / 2
    color: root.stateColor
    border.width: 1
    border.color: root.bar ? root.bar.background : Color.background
  }
}
