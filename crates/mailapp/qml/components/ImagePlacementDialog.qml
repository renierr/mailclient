import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// "Show these images inside the text, or attach them?" — asked when images
// are dropped onto the composer. The files ride along in `urls` and come
// back with the choice.
Dialog {
    id: root

    property var urls: []

    signal inlineChosen(var urls)
    signal attachChosen(var urls)

    function ask(urls) {
        root.urls = urls.slice();
        root.open();
    }

    title: root.urls.length === 1 ? qsTr("Add image") : qsTr("Add %1 images").arg(root.urls.length)
    modal: true
    anchors.centerIn: parent
    width: Math.min(440, (parent ? parent.width : 440) - 2 * Theme.lg)
    padding: Theme.lg

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    footer: Flow {
        spacing: Theme.sm
        layoutDirection: Qt.RightToLeft
        leftPadding: Theme.lg
        rightPadding: Theme.lg
        bottomPadding: Theme.md
        topPadding: Theme.sm

        AppButton {
            text: qsTr("Insert inline")
            intent: "primary"
            onClicked: {
                root.close();
                root.inlineChosen(root.urls);
            }
        }
        AppButton {
            text: qsTr("Attach")
            onClicked: {
                root.close();
                root.attachChosen(root.urls);
            }
        }
        AppButton {
            text: qsTr("Cancel")
            onClicked: root.close()
        }
    }

    Label {
        width: parent.width
        wrapMode: Text.Wrap
        color: Theme.text
        font.pixelSize: Theme.fontBase
        text: qsTr("Inline images show inside the message text. Attached ones arrive as separate files.")
    }
}
