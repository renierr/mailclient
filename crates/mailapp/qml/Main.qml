import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// App shell: 3-pane mail layout (sidebar / list / reader).
// Data comes from the Rust Bridge (SQLite + IMAP); no mock models remain.
ApplicationWindow {
    id: root
    visible: true
    width: 1280
    height: 800
    minimumWidth: 760
    minimumHeight: 480
    title: qsTr("Mailclient")

    property string currentFolder: ""
    property int currentMessageIndex: 0
    property string statusText: qsTr("Starting…")

    Bridge {
        id: backend
    }

    SettingsBridge {
        id: appSettings
    }

    ListModel {
        id: folderModel
    }
    ListModel {
        id: messageModel
    }

    function reloadFolders() {
        var arr = JSON.parse(backend.folders_json)
        folderModel.clear()
        for (var i = 0; i < arr.length; i++)
            folderModel.append(arr[i])
        // Keep selection if still present, else inbox, else first.
        var found = false
        for (var j = 0; j < folderModel.count; j++) {
            if (folderModel.get(j).name === root.currentFolder) {
                found = true
                break
            }
        }
        if (!found) {
            var inbox = ""
            for (var k = 0; k < folderModel.count; k++) {
                if (folderModel.get(k).role === "inbox")
                    inbox = folderModel.get(k).name
            }
            root.currentFolder = inbox !== "" ? inbox : (folderModel.count > 0 ? folderModel.get(0).name : "")
        }
    }

    function reloadMessages() {
        var arr = JSON.parse(backend.messages_json)
        messageModel.clear()
        for (var i = 0; i < arr.length; i++)
            messageModel.append(arr[i])
        if (root.currentMessageIndex >= messageModel.count)
            root.currentMessageIndex = 0
    }

    function reloadAll() {
        var r = backend.refresh_accounts()
        reloadFolders()
        reloadMessages()
        return r
    }

    function showResult(okMessage, result) {
        root.statusText = result === "" ? okMessage : result
    }

    Component.onCompleted: {
        appSettings.load()
        var r = reloadAll()
        if (backend.account_count === 0) {
            root.statusText = qsTr("Add an account to start")
            accountSetup.open()
        } else if (r !== "") {
            root.statusText = r
        } else {
            root.statusText = qsTr("Ready")
        }
    }

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
                enabled: backend.account_count > 0
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
                enabled: backend.account_count > 0
                onClicked: {
                    root.statusText = qsTr("Syncing…")
                    // NOTE: blocking network call; async worker is a follow-up.
                    var r = backend.sync_now()
                    reloadFolders()
                    reloadMessages()
                    root.statusText = r
                }
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
            currentAccount: backend.account_count > 0 ? qsTr("%n account(s)", "", backend.account_count) : qsTr("No account")
            currentFolder: root.currentFolder
            onFolderSelected: path => {
                var r = backend.select_folder(path)
                if (r === "") {
                    root.currentFolder = path
                    root.currentMessageIndex = 0
                    reloadMessages()
                    root.statusText = qsTr("Folder: %1").arg(path)
                } else {
                    root.statusText = r
                }
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
                var m = messageModel.get(index)
                if (m !== undefined) {
                    backend.open_message(m.uid)
                    reloadFolders()
                    reloadMessages()
                }
            }
        }
        MessageView {
            id: messageView
            SplitView.fillWidth: true
            SplitView.minimumWidth: 300
            loadRemoteImages: appSettings.load_remote_images
            message: messageModel.count > 0 ? messageModel.get(Math.min(root.currentMessageIndex, messageModel.count - 1)) : undefined
            onReplyRequested: composer.openForReply(messageModel.get(root.currentMessageIndex))
            onForwardRequested: composer.openForForward(messageModel.get(root.currentMessageIndex))
            onStarRequested: {
                var m = messageModel.get(root.currentMessageIndex)
                if (m !== undefined) {
                    showResult("", backend.toggle_star(m.uid))
                    reloadMessages()
                }
            }
            onDeleteRequested: {
                var d = messageModel.get(root.currentMessageIndex)
                if (d !== undefined) {
                    var r = backend.delete_message(d.uid)
                    root.currentMessageIndex = 0
                    reloadFolders()
                    reloadMessages()
                    showResult(qsTr("Deleted"), r)
                }
            }
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
        accountEmail: backend.current_account_email
        sendFormat: appSettings.compose_send_format
        onStatusMessage: text => root.statusText = text
        onSendRequested: payload => {
            var r = backend.send_mail(payload)
            if (r === "") {
                composer.close()
                reloadFolders()
                reloadMessages()
                root.statusText = qsTr("Sent")
            } else {
                root.statusText = r
            }
        }
    }
    AccountSetup {
        id: accountSetup
        onStatusMessage: text => root.statusText = text
        onAccountSubmit: payload => {
            var r = backend.add_account(payload)
            if (r === "") {
                accountSetup.close()
                reloadAll()
                root.statusText = qsTr("Account added — press ⟳ to sync")
            } else {
                root.statusText = r
            }
        }
    }
    Settings {
        id: settingsDialog
        settingsBridge: appSettings
        onStatusMessage: text => root.statusText = text
    }
}
