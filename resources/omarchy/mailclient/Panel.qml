import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Bar entry point for mailclient.unread: envelope badge with the cached
// unread count. Left-click opens the mail app, right-click syncs now,
// the tooltip lists the newest unread senders.
BarWidget {
  id: root
  moduleName: "mailclient.unread"

  Service {
    id: mail
    settings: root.settings
    bar: root.bar
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  IpcHandler {
    target: "mailclient.unread"

    function refresh(): void { mail.refresh() }
    function sync(): void { mail.syncNow() }
    function open(): void { mail.openApp() }
    function show(): void { mail.openApp() }
    function toggle(): void { mail.openApp() }
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
      else mail.openApp()
    }
  }
}
