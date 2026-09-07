import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// App shell: 3-pane mail layout (sidebar / list / reader).
// M0 runs on mock data so `qml6 qml/Main.qml` works without Rust.
// Rust list models (M1-M3) will replace the ListModels below 1:1.
ApplicationWindow {
    id: root
    visible: true
    width: 1280
    height: 800
    minimumWidth: 760
    minimumHeight: 480
    title: qsTr("Mailclient")

    property string currentAccount: "Work"
    property string currentFolder: "INBOX"
    property int currentMessageIndex: 0
    property string statusText: qsTr("Ready — add an account to start syncing")

    // --- mock data (replaced by Rust models in M1-M3) -------------------
    ListModel {
        id: folderModel
        ListElement { name: "INBOX";  role: "inbox";   unread: 2 }
        ListElement { name: "Drafts"; role: "drafts";  unread: 0 }
        ListElement { name: "Sent";   role: "sent";    unread: 0 }
        ListElement { name: "Archive"; role: "archive"; unread: 0 }
        ListElement { name: "Spam";   role: "junk";    unread: 5 }
        ListElement { name: "Trash";  role: "trash";   unread: 0 }
    }
    ListModel {
        id: messageModel
        ListElement {
            subject: "Welcome to Mailclient"; from: "alice@example.com"
            date: "09:12"; snippet: "This is a local mock message…"
            unread: true; starred: false
            body: "<h2>Welcome!</h2><p>This is a <b>mock</b> message rendered as rich text. The Rust/IMAP sync (M1) will replace it with real mail.</p>"
        }
        ListElement {
            subject: "Design notes: QML reader"; from: "bob@example.com"
            date: "08:03"; snippet: "WebEngine sandboxing for untrusted HTML…"
            unread: true; starred: true
            body: "<p>Reminder: render untrusted HTML in <b>QtWebEngine</b> with remote content blocked (M3). Plain rich text is fine for the M0 shell.</p>"
        }
        ListElement {
            subject: "Lunch?"; from: "carol@example.com"
            date: "Yesterday"; snippet: "Are you free at noon tomorrow?"
            unread: false; starred: false
            body: "<p>Are you free at noon tomorrow?</p><p>— Carol</p>"
        }
    }
    // --------------------------------------------------------------------

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 8

            ToolButton {
                text: qsTr("☰")
                Accessible.name: qsTr("Toggle sidebar")
                onClicked: sidebar.visible = !sidebar.visible
            }
            Button {
                text: qsTr("Compose")
                highlighted: true
                onClicked: composer.open()
            }
            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: qsTr("Search mail… (FTS in M3)")
                onAccepted: root.statusText = qsTr("Search is wired to SQLite FTS in M3")
            }
            ToolButton {
                text: qsTr("⟳")
                Accessible.name: qsTr("Sync now")
                onClicked: root.statusText = qsTr("IMAP sync lands in M1 — nothing to do yet")
            }
            ToolButton {
                text: qsTr("✉ Account")
                onClicked: accountSetup.open()
            }
            ToolButton {
                text: qsTr("⚙")
                Accessible.name: qsTr("Settings")
                onClicked: settingsDialog.open()
            }
        }
    }

    SplitView {
        anchors.fill: parent

        Sidebar {
            id: sidebar
            SplitView.preferredWidth: 240
            SplitView.minimumWidth: 160
            folders: folderModel
            currentAccount: root.currentAccount
            currentFolder: root.currentFolder
            onFolderSelected: path => {
                root.currentFolder = path
                root.statusText = qsTr("Folder: %1").arg(path)
            }
            onAddAccountRequested: accountSetup.open()
        }
        MessageList {
            id: messageList
            SplitView.preferredWidth: 340
            SplitView.minimumWidth: 220
            messages: messageModel
            currentIndex: root.currentMessageIndex
            onMessageSelected: index => {
                root.currentMessageIndex = index
            }
        }
        MessageView {
            id: messageView
            SplitView.fillWidth: true
            SplitView.minimumWidth: 300
            message: messageModel.get(root.currentMessageIndex)
            onReplyRequested: composer.openForReply(messageModel.get(root.currentMessageIndex))
            onStatusMessage: text => root.statusText = text
        }
    }

    footer: ToolBar {
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: 12
            text: root.statusText
            elide: Text.ElideRight
        }
    }

    Composer {
        id: composer
        onStatusMessage: text => root.statusText = text
    }
    AccountSetup {
        id: accountSetup
        onStatusMessage: text => root.statusText = text
    }
    Settings {
        id: settingsDialog
        onStatusMessage: text => root.statusText = text
    }
}
