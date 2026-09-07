import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Folder/account sidebar. Expects `folders` ListModel with
// {name, role, unread}; emits folderSelected(path).
Pane {
    id: root

    property var folders
    property string currentAccount: ""
    property string currentFolder: ""
    signal folderSelected(string path)
    signal addAccountRequested()

    padding: 0

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ComboBox {
            id: accountBox
            Layout.fillWidth: true
            Layout.margins: 8
            model: [root.currentAccount === "" ? qsTr("No account") : root.currentAccount, qsTr("+ Add account…")]
            onActivated: index => {
                if (index === 1)
                    root.addAccountRequested()
            }
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.folders
            delegate: ItemDelegate {
                width: folderList.width
                highlighted: model.name === root.currentFolder
                onClicked: root.folderSelected(model.name)
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    Label {
                        text: {
                            switch (model.role) {
                            case "inbox": return "📥"
                            case "drafts": return "📝"
                            case "sent": return "📤"
                            case "archive": return "🗄"
                            case "junk": return "🚫"
                            case "trash": return "🗑"
                            default: return "📁"
                            }
                        }
                    }
                    Label {
                        text: model.name
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        font.bold: model.unread > 0
                    }
                    Label {
                        text: model.unread > 0 ? model.unread : ""
                        font.bold: true
                        opacity: 0.8
                    }
                }
            }
        }
    }
}
