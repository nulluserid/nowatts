import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

// SKETCH (pre-build) — deliberately NOT loadable. See ../PLAN.md P1b.
// Structure mirrors io.github.nulluserid.dishy/BarWidget.qml, the working
// reference on this machine. Reads ONLY ~/.local/state/nowatts/status.json
// (../CONTRACT.md). Copy the panelLoader/injectPanel boilerplate from Dishy
// when implementing P1b — do not reinvent it.
BarWidget {
  id: root
  moduleName: "legend.nowatts"

  // TODO(P1b): pick a plug/power glyph that exists in the Omarchy nerd font.
  readonly property string glyph: ""  // e.g. nf-md-power_plug — choose in-theme
  readonly property string statusPath: Quickshell.env("HOME") + "/.local/state/nowatts/status.json"

  // Parsed CONTRACT.md v1 file; null = missing/corrupt. heartbeat_unix older
  // than 60s => stale: show marker, never keep displaying old watts silently.
  property var statusData: null
  property bool stale: true

  // TODO(P1b): label per manifest "format" setting:
  //   watts    -> glyph + measured charging W from ports[] (e.g. " 65W")
  //   contract -> glyph + contract W
  //   icon     -> glyph only
  // show=auto -> muted/hidden when every port.partner=false and !ac_online
  readonly property string label: root.glyph

  function tick() {
    // TODO(P1b): statusFile.reload(); parse; set statusData + stale.
  }

  // TODO(P1b): Timer { interval: root.setting("interval", 1000) } + FileView { path: root.statusPath }
  // TODO(P1b): panel host boilerplate (panelLoader/injectPanel/togglePanel) from Dishy.
}
