import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// "Moved 3 to Trash — Undo" for the grace period of an undoable action.
// View-only: the bridge queues the action and pushes it later; this only
// shows the label and hands the batch back when Undo is pressed.
Rectangle {
    id: root

    property string batch: ""
    property string label: ""
    property int durationMs: 8000

    signal undoRequested(string batch)

    function show(batch, label) {
        root.batch = batch;
        root.label = label;
        hideTimer.restart();
    }

    function dismiss() {
        hideTimer.stop();
        root.batch = "";
    }

    function undo() {
        if (root.batch === "")
            return;
        var b = root.batch;
        root.dismiss();
        root.undoRequested(b);
    }

    visible: root.batch !== ""
    width: Math.min(row.implicitWidth + 2 * Theme.md, parent ? parent.width - 2 * Theme.md : row.implicitWidth)
    height: Math.max(Theme.controlHeight, row.implicitHeight) + 2 * Theme.sm
    radius: Theme.radiusLg
    color: Theme.bgRaised
    border.color: Theme.border
    border.width: 1

    Accessible.role: Accessible.AlertMessage
    Accessible.name: root.label

    Timer {
        id: hideTimer
        interval: root.durationMs
        onTriggered: root.batch = ""
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: Theme.md
        anchors.rightMargin: Theme.sm
        spacing: Theme.md

        Label {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            text: root.label
            color: Theme.text
            font.pixelSize: Theme.fontBase
            elide: Text.ElideRight
        }
        AppButton {
            text: qsTr("Undo")
            intent: "primary"
            onClicked: root.undo()
        }
        IconButton {
            implicitWidth: Theme.miniButton
            implicitHeight: Theme.miniButton
            fontSize: Theme.fontSmall
            text: Icons.close
            iconFont: true
            tooltip: qsTr("Dismiss")
            onClicked: root.dismiss()
        }
    }
}
