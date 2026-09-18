import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Bar entry point for mailclient.unread: envelope badge with the cached
// unread count plus a popup with newest mail, settings, and actions.
// Hover shows the tooltip, left-click toggles the popup, right-click syncs
// now, middle-click opens the mail app straight away.
BarWidget {
  id: root
  moduleName: "mailclient.unread"

  readonly property color panelForeground: bar ? bar.foreground : Color.foreground
  readonly property string panelFontFamily: bar ? bar.fontFamily : Style.font.family

  Service {
    id: mail
    settings: root.settings
    bar: root.bar
  }

  // Popup state. Shape contract for shell summon/hide/toggle routing.
  property bool panelOpen: false
  readonly property bool opened: panelOpen

  function open() { panelOpen = true }
  function close() { panelOpen = false }
  function toggle() { panelOpen = !panelOpen }

  // Persist one setting to shell.json (clock pattern): applied locally
  // first so the popup reflects it instantly, then stored.
  function saveSetting(key, value) {
    var entry = { id: root.moduleName }
    for (var k in root.settings) if (k !== "id") entry[k] = root.settings[k]
    entry[key] = value
    root.settings = entry
    if (root.bar && root.bar.shell && typeof root.bar.shell.updateEntryInline === "function")
      root.bar.shell.updateEntryInline(root.moduleName, entry)
  }

  function heroMeta() {
    if (mail.syncing) return "Syncing…"
    if (mail.unread === 1) return "1 unread message"
    if (mail.unread > 1) return mail.unread + " unread messages"
    return "All caught up"
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  IpcHandler {
    target: "mailclient.unread"

    function refresh(): void { mail.refresh() }
    function sync(): void { mail.syncNow() }
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: Model.badge(mail.unread)
    tooltipText: Model.tooltip(mail.unread, mail.recent, mail.lastError, mail.syncing)
    dimmed: mail.unread === 0 && !mail.syncing
    active: mail.unread > 0
    onPressed: function(b) {
      if (b === Qt.RightButton) mail.syncNow()
      else if (b === Qt.MiddleButton) mail.openApp()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.panelOpen
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(600))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
    }

    Column {
      id: column
      width: parent ? parent.width : panel.contentWidth
      spacing: Style.space(12)

      PanelHero {
        id: hero
        width: parent.width
        title: "Mail"
        meta: root.heroMeta()
        foreground: root.panelForeground
        fontFamily: root.panelFontFamily
        iconComponent: Component {
          Text {
            textFormat: Text.PlainText
            anchors.centerIn: parent
            text: "󰇮"
            color: hero.foreground
            font.family: hero.fontFamily
            font.pixelSize: hero.iconSize
          }
        }
      }

      Text {
        textFormat: Text.PlainText
        visible: mail.lastError !== ""
        width: parent.width
        text: mail.lastError
        color: root.bar ? root.bar.urgent : Color.urgent
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
      }

      Column {
        visible: mail.recent.length > 0
        width: parent.width
        spacing: Style.space(4)

        Repeater {
          model: mail.recent.slice(0, 5)

          Button {
            required property var modelData
            width: parent.width
            leftAlign: true
            text: Model.elide(modelData.from, 28) + " — " + Model.elide(modelData.subject, 40)
            onClicked: {
              mail.openApp()
              root.close()
            }
          }
        }
      }

      Text {
        textFormat: Text.PlainText
        visible: mail.recent.length === 0 && !mail.syncing
        width: parent.width
        text: "Nothing unread. Enjoy the quiet."
        color: Qt.darker(root.panelForeground, 1.4)
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.bodySmall
      }

      PanelSeparator {
        width: parent.width
      }

      PanelSectionHeader {
        width: parent.width
        text: "Settings"
      }

      NumberField {
        width: parent.width
        label: "Sync every (minutes)"
        from: 1
        to: 1440
        stepSize: 1
        value: mail.syncIntervalMin
        onModified: function(v) { root.saveSetting("syncIntervalMin", v) }
      }

      Row {
        width: parent.width
        spacing: Style.space(8)

        Text {
          textFormat: Text.PlainText
          width: parent.width - notifySwitch.width - parent.spacing
          anchors.verticalCenter: parent.verticalCenter
          text: "Notify on new mail"
          color: root.panelForeground
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }

        ToggleSwitch {
          id: notifySwitch
          anchors.verticalCenter: parent.verticalCenter
          checked: mail.notify
          onToggled: root.saveSetting("notify", !mail.notify)
        }
      }

      TextField {
        width: parent.width
        text: mail.account
        placeholderText: "All accounts (id or address filter)"
        font.family: root.panelFontFamily
        onAccepted: root.saveSetting("account", text.trim())
      }

      Text {
        textFormat: Text.PlainText
        width: parent.width
        text: "Last sync: " + Model.lastSyncText(mail.lastSyncAt)
        color: Qt.darker(root.panelForeground, 1.4)
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.bodySmall
      }

      Row {
        width: parent.width
        spacing: Style.space(8)

        Button {
          width: (parent.width - parent.spacing) / 2
          text: "Open mail"
          onClicked: {
            mail.openApp()
            root.close()
          }
        }

        Button {
          width: (parent.width - parent.spacing) / 2
          text: mail.syncing ? "Syncing…" : "Sync now"
          onClicked: mail.syncNow()
        }
      }
    }
  }
}
