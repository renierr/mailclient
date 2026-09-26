import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Recipient input with local, privacy-controlled contact suggestions. Only the
// current comma/semicolon-delimited recipient is replaced on selection.
Item {
    id: root
    implicitHeight: field.implicitHeight
    property var backend
    property bool enabledSuggestions: true
    property alias text: field.text
    property alias placeholderText: field.placeholderText
    signal edited

    function query() {
        var pieces = field.text.split(/[;,]/);
        return pieces[pieces.length - 1].trim();
    }

    function refresh() {
        if (!root.enabledSuggestions || !root.backend || root.query() === "") {
            suggestionModel.clear();
            popup.close();
            return;
        }
        var rows = [];
        rows = FeedJson.parse(root.backend.contacts_json(root.query()), []);
        suggestionModel.clear();
        for (var i = 0; i < rows.length; i++)
            suggestionModel.append(rows[i]);
        if (suggestionModel.count > 0)
            popup.open();
        else
            popup.close();
    }

    function choose(address) {
        var end = field.text.search(/[;,][^;,]*$/);
        field.text = end < 0 ? address : field.text.substring(0, end + 1) + " " + address;
        field.cursorPosition = field.text.length;
        popup.close();
        field.forceActiveFocus();
    }

    AppTextField {
        id: field
        anchors.fill: parent
        onTextEdited: {
            root.edited();
            root.refresh();
        }
        onActiveFocusChanged: if (!activeFocus)
                                  closeTimer.restart()
        Keys.onEscapePressed: popup.close()
    }

    ListModel {
        id: suggestionModel
    }
    Timer {
        id: closeTimer
        interval: 150
        onTriggered: popup.close()
    }
    Popup {
        id: popup
        x: 0
        y: root.height + 2
        // At least 360 wide for readable suggestions, but never past the
        // window's right edge: the field is indented, so a narrow window
        // would otherwise push the list off-screen.
        property real roomRight: root.width
        width: Math.max(root.width, Math.min(360, roomRight))
        onAboutToShow: {
            var win = root.Window.window;
            roomRight = win ? win.width - root.mapToItem(null, 0, 0).x - 8 : root.width;
        }
        padding: 4
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle {
            color: Theme.bgRaised
            radius: Theme.radius
            border.width: 1
            border.color: Theme.border
        }
        contentItem: ListView {
            implicitHeight: Math.min(contentHeight, 220)
            model: suggestionModel
            clip: true
            delegate: ItemDelegate {
                width: ListView.view.width
                implicitHeight: Math.round(44 * Theme.uiScale)
                padding: Theme.sm
                contentItem: RowLayout {
                    spacing: Theme.sm
                    Label {
                        text: {
                            var display = model.alias || model.name || "";
                            return display !== "" ? display : model.address;
                        }
                        color: Theme.text
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.maximumWidth: Math.round(ListView.view.width * 0.45)
                    }
                    Label {
                        text: {
                            var display = model.alias || model.name || "";
                            return display !== "" ? "<" + model.address + ">" : "";
                        }
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Label {
                        text: model.name && model.alias && model.name !== model.alias ? "(" + model.name + ")" : ""
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        elide: Text.ElideRight
                        visible: text !== ""
                    }
                }
                onClicked: {
                    var display = (model.alias || model.name || "").replace(/[;,]/g, " ").trim();
                    var formatted = display !== "" ? display + " <" + model.address + ">" : model.address;
                    root.choose(formatted);
                }
            }
        }
    }
}
