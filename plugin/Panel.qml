import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// nowatts dashboard. status.json flows in from the bar widget (injected as
// `status`); this file reads nothing itself — presentation only, no writes,
// no subprocesses (FR-006). Anything missing/null/unknown in the CONTRACT
// v1 shape collapses to an omitted line or "—", never an error (FR-003).
Panel {
  id: root
  moduleName: "legend.nowatts"
  ipcTarget: "nowatts"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property var status: null
  readonly property var barIdentity: hostWidget || root

  readonly property color fg: root.barForeground
  readonly property color dim: Color.muted
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // status.json mirror (kept fresh by the bar widget's own timer).
  readonly property var s: status ? status.statusData : null
  readonly property bool sStale: status ? status.stale : true
  readonly property bool hasData: s !== null && !sStale
  readonly property var ports: hasData ? Model.portsOf(s) : []
  readonly property bool degradedTypec: hasData && Model.degradedTypec(s)
  readonly property var unmapped: hasData && Array.isArray(s.unmapped_devices) ? s.unmapped_devices : []

  // Global battery line (top-level CONTRACT field, not per-port — the same
  // numbers must not repeat on every card; the verdicts already carry them).
  // Hidden entirely when battery.present is false; percent/power_w null drop
  // their segment ("Discharging  ·  -8.1W" with no %).
  readonly property var battery: hasData && s.battery && s.battery.present !== false ? s.battery : null
  readonly property string batteryText: {
    if (!root.battery) return ""
    var parts = [String(root.battery.status || "Battery")]
    var w = Model.fmtWatts(root.battery.power_w)
    if (w !== null) parts.push(w + "W")
    var pct = Model.fmtPctInt(root.battery.percent)
    if (pct !== null) parts.push(pct)
    return parts.join("  ·  ")
  }

  readonly property color stateColor: status ? status.stateColor : Color.muted

  // Recomputes on every tick: statusData is reassigned each interval, so the
  // "ago" text stays live without its own timer.
  readonly property string lastUpdateAgo: {
    var hb = root.s ? Model.num(root.s.heartbeat_unix, 0) : 0
    return hb > 0 ? Model.humanDur(Math.max(0, Date.now() / 1000 - hb)) : null
  }

  function open() { root.controller.show() }
  function close() { root.controller.hide() }
  function toggle() { root.opened ? root.close() : root.open() }

  function scrollBy(dy) {
    var max = Math.max(0, dashScroll.contentHeight - dashScroll.height)
    dashScroll.contentY = Math.max(0, Math.min(max, dashScroll.contentY + dy * 80))
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    centerOnBar: true
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(620))
    contentHeight: panel.fittedContentHeight(Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) { if (dy !== 0) root.scrollBy(dy) }
      onCloseRequested: root.close()
      onTabRequested: function(direction) { if (root.bar) root.bar.switchPanelFrom(root.barIdentity, direction) }
      onTextKey: function(t) { if (t === "q" || t === "Q") root.close() }
    }

    // Waiting / not-responding states — global, and no numbers are shown in
    // either (CONTRACT rule 4: never keep displaying old values as live).
    Column {
      anchors.centerIn: parent
      visible: !root.hasData
      spacing: Style.space(8)

      Text {
        anchors.horizontalCenter: parent.horizontalCenter
        text: root.s === null ? "Waiting for nowatts data…" : "nowatts is not responding"
        textFormat: Text.PlainText
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.subtitle
      }

      Text {
        anchors.horizontalCenter: parent.horizontalCenter
        text: root.s === null
          ? "systemctl --user start nowatts"
          : "last update " + (root.lastUpdateAgo || "unknown") + " ago  ·  systemctl --user status nowatts"
        textFormat: Text.PlainText
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
      }
    }

    // Dashboard.
    Flickable {
      id: dashScroll
      anchors.fill: parent
      visible: root.hasData
      contentWidth: width
      contentHeight: dashColumn.implicitHeight
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      interactive: contentHeight > height

      Column {
        id: dashColumn
        width: dashScroll.width
        spacing: Style.space(12)

        // ---- Header: state dot + name + global battery line ----
        // Wrapper Item: Row children may not use anchors (Dishy KVRow pattern).
        Item {
          width: parent.width
          height: Math.max(headerDot.height, headerTitle.implicitHeight, headerBattery.implicitHeight)

          Rectangle {
            id: headerDot
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(10)
            height: width
            radius: height / 2
            color: root.stateColor
          }

          Text {
            id: headerTitle
            anchors.left: headerDot.right
            anchors.leftMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            text: "nowatts"
            textFormat: Text.PlainText
            color: root.fg
            font.family: root.fontFamily
            font.pixelSize: Style.font.heading
            font.bold: true
          }

          Text {
            id: headerBattery
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: headerTitle.right
            anchors.leftMargin: Style.space(10)
            visible: root.batteryText !== ""
            horizontalAlignment: Text.AlignRight
            text: root.batteryText
            textFormat: Text.PlainText
            elide: Text.ElideMiddle
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
          }
        }

        // ---- Degraded banner: typec class missing (CONTRACT verdict
        //      "No USB-C data available"; ports[] is [] in this state) ----
        Rectangle {
          visible: root.degradedTypec
          width: parent.width
          implicitHeight: degradedColumn.implicitHeight + Style.space(16)
          radius: Style.space(6)
          color: "transparent"
          border.width: 1
          border.color: Model.WARN_HEX

          Column {
            id: degradedColumn
            anchors.centerIn: parent
            width: parent.width - Style.space(20)
            spacing: Style.space(2)

            Text {
              text: "No USB-C data available"
              textFormat: Text.PlainText
              color: Model.WARN_HEX
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              font.bold: true
            }

            Text {
              text: "nowatts could not read USB-C port state — battery is still reported above"
              textFormat: Text.PlainText
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              wrapMode: Text.Wrap
              width: parent.width
            }
          }
        }

        // ---- Empty state: live data but zero ports (and not degraded) ----
        Text {
          visible: root.ports.length === 0 && !root.degradedTypec
          width: parent.width
          text: "No USB-C ports reported."
          textFormat: Text.PlainText
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }

        // ---- One card per ports[] entry (0..N) ----
        Repeater {
          model: root.ports

          Rectangle {
            id: card
            required property var modelData
            readonly property var port: modelData || {}
            readonly property string location: String(port.location || "")
            readonly property var contractWatts: port.contract ? Model.num(port.contract.watts) : null
            readonly property var measuredWatts: port.measured ? Model.num(port.measured.watts) : null
            readonly property string warning: port.warning === null || port.warning === undefined ? "" : String(port.warning)
            readonly property bool hasDevices: Array.isArray(port.devices) && port.devices.length > 0
            readonly property var tb: port.thunderbolt || null
            readonly property string rolesText: {
              var parts = []
              var pr = String(port.power_role || "")
              var dr = String(port.data_role || "")
              if (pr !== "") parts.push(pr)
              if (dr !== "") parts.push(dr)
              return parts.join("  ·  ")
            }
            readonly property string tbText: {
              if (!card.tb) return ""
              var parts = []
              var gen = Model.num(card.tb.generation)
              if (gen !== null) parts.push("gen " + gen)
              parts.push(card.tb.authorized ? "authorized" : "not authorized")
              var nm = String(card.tb.name || "")
              if (nm !== "") parts.push(nm)
              return "Thunderbolt  ·  " + parts.join("  ·  ")
            }

            width: parent.width
            implicitHeight: cardColumn.implicitHeight + Style.space(20)
            radius: Style.space(6)
            color: Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.05)
            border.width: card.warning !== "" ? 1 : 0
            border.color: Model.WARN_HEX

            Column {
              id: cardColumn
              anchors.fill: parent
              anchors.margins: Style.space(10)
              spacing: Style.space(6)

              // Title: port id + location ("left upper" → "Left upper").
              // Location omitted entirely when null/missing.
              Text {
                text: String(card.port.id || "port")
                  + (card.location !== "" ? "  ·  " + Model.fmtLocation(card.location) : "")
                textFormat: Text.PlainText
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                font.bold: true
              }

              // Verdict, verbatim from status.json (the daemon owns wording).
              Text {
                width: parent.width
                text: String(card.port.verdict || "")
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                color: root.fg
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
              }

              // Warning banner (CONTRACT warning enum, verbatim; additive
              // to the verdict, null = fine).
              Rectangle {
                visible: card.warning !== ""
                width: parent.width
                implicitHeight: warningLabel.implicitHeight + Style.space(12)
                radius: Style.space(4)
                color: "transparent"
                border.width: 1
                border.color: Model.WARN_HEX

                Text {
                  id: warningLabel
                  anchors.centerIn: parent
                  width: parent.width - Style.space(16)
                  text: card.warning
                  textFormat: Text.PlainText
                  wrapMode: Text.Wrap
                  color: Model.WARN_HEX
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                }
              }

              // Contract vs measured. "—" per value when null.
              Text {
                visible: card.contractWatts !== null || card.measuredWatts !== null
                text: "Contract " + (card.contractWatts !== null ? Model.fmtWatts(card.contractWatts) + "W" : "—")
                  + "  ·  Measured " + (card.measuredWatts !== null ? Model.fmtWatts(card.measuredWatts) + "W" : "—")
                textFormat: Text.PlainText
                color: root.fg
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
              }

              // Power/data roles (omitted when both absent).
              Text {
                visible: card.rolesText !== ""
                text: card.rolesText
                textFormat: Text.PlainText
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
              }

              // Attached devices with speeds.
              Column {
                visible: card.hasDevices
                width: parent.width
                spacing: Style.space(2)

                Repeater {
                  model: card.hasDevices ? card.port.devices : []

                  Text {
                    required property var modelData
                    readonly property var dev: modelData || {}
                    readonly property string name: String(dev.name || dev.product || dev.vendor || "device")

                    width: parent.width
                    text: "·  " + name + "  ·  " + Model.fmtSpeedMbps(dev.speed_mbps)
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: root.fg
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.bodySmall
                  }
                }
              }

              // Thunderbolt line, only when a TB/USB4 link exists.
              Text {
                visible: card.tbText !== ""
                text: card.tbText
                textFormat: Text.PlainText
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
              }

              // psy id footer, only when present.
              Text {
                visible: String(card.port.psy || "") !== ""
                text: String(card.port.psy)
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                width: parent.width
                color: Qt.rgba(root.dim.r, root.dim.g, root.dim.b, 0.7)
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
              }
            }
          }
        }

        // ---- Unassociated devices (ports where association failed) ----
        Column {
          visible: root.unmapped.length > 0
          width: parent.width
          spacing: Style.space(4)

          Text {
            text: "UNASSOCIATED DEVICES"
            textFormat: Text.PlainText
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            font.bold: true
          }

          Repeater {
            model: root.unmapped

            Text {
              required property var modelData
              readonly property var dev: modelData || {}
              readonly property string name: String(dev.name || dev.product || dev.vendor || "device")

              width: parent.width
              text: "·  " + name + "  ·  " + Model.fmtSpeedMbps(dev.speed_mbps)
              textFormat: Text.PlainText
              elide: Text.ElideRight
              color: root.fg
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
            }
          }
        }

        PanelSeparator { foreground: root.fg }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          text: "Esc  close   ·   ↑ ↓  scroll"
          textFormat: Text.PlainText
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
        }
      }
    }
  }
}
