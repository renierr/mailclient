import QtQuick
import QtQuick.Controls

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
    signal edited()

    function query() {
        var pieces = field.text.split(/[;,]/)
        return pieces[pieces.length - 1].trim()
    }

    function refresh() {
        if (!root.enabledSuggestions || !root.backend || root.query() === "") {
            suggestionModel.clear()
            popup.close()
            return
        }
        var rows = []
        try { rows = JSON.parse(root.backend.contacts_json(root.query())) } catch (e) {}
        suggestionModel.clear()
        for (var i = 0; i < rows.length; i++)
            suggestionModel.append(rows[i])
        if (suggestionModel.count > 0)
            popup.open()
        else
            popup.close()
    }

    function choose(address) {
        var end = field.text.search(/[;,][^;,]*$/)
        field.text = end < 0 ? address : field.text.substring(0, end + 1) + " " + address
        field.cursorPosition = field.text.length
        popup.close()
        field.forceActiveFocus()
    }

    AppTextField {
        id: field
        anchors.fill: parent
        onTextEdited: {
            root.edited()
            root.refresh()
        }
        onActiveFocusChanged: if (!activeFocus) closeTimer.restart()
        Keys.onEscapePressed: popup.close()
    }

    ListModel { id: suggestionModel }
    Timer { id: closeTimer; interval: 150; onTriggered: popup.close() }
    Popup {
        id: popup
        x: 0
        y: root.height + 2
        width: root.width
        padding: 4
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: Theme.bgRaised; radius: Theme.radius; border.width: 1; border.color: Theme.border }
        contentItem: ListView {
            implicitHeight: Math.min(contentHeight, 192)
            model: suggestionModel
            clip: true
            delegate: ItemDelegate {
                width: ListView.view.width
                text: (model.name ? model.name + " <" + model.address + ">" : model.address)
                onClicked: root.choose(model.address)
            }
        }
    }
}
