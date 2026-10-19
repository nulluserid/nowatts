import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

// SKETCH (pre-build) — deliberately NOT loadable. See ../PLAN.md P1b.
// Modeled on io.github.nulluserid.dishy/Panel.qml (working reference on this
// machine): Panel base, moduleName, host injects bar/settings/anchorItem/
// hostWidget/status; open()/close()/toggle()/opened.
Panel {
  id: root
  moduleName: "legend.nowatts"
  // TODO(P1b): ipcTarget + manageIpc:false + injected properties, per Dishy.

  // TODO(P1b): one plain-English card per ports[] (CONTRACT.md):
  //   verdict (verbatim from status.json), contract vs measured,
  //   battery line (rate + %), devices + speeds, thunderbolt line,
  //   warning banner (P2) — plus a "daemon stale" state.
}
