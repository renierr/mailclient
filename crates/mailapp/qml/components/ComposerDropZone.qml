import QtQuick
import QtQuick.Controls

import Mailclient

// Files dragged onto the composer. Sorts them into images that could go
// inline and everything else; the composer decides what to do with each.
// Sits above the editor so the WebEngine page never receives the drop (it
// would otherwise insert the raw file or try to open it).
DropArea {
    id: root

    // Asked per file; the bridge knows which types can be shown inline.
    property var isInlineImage: function (url) {
        return false;
    }

    signal filesDropped(var images, var others)

    keys: ["text/uri-list"]

    onEntered: drag => drag.accepted = drag.hasUrls
    onDropped: drop => {
        if (!drop.hasUrls)
            return;
        var images = [];
        var others = [];
        for (var i = 0; i < drop.urls.length; i++) {
            var u = drop.urls[i].toString();
            if (root.isInlineImage(u))
                images.push(u);
            else
                others.push(u);
        }
        drop.accept(Qt.CopyAction);
        root.filesDropped(images, others);
    }

    Rectangle {
        anchors.fill: parent
        visible: root.containsDrag
        radius: Theme.radius
        color: Theme.selected
        opacity: 0.9
        border.width: 2
        border.color: Theme.accent

        Label {
            anchors.centerIn: parent
            width: parent.width - 2 * Theme.lg
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            text: qsTr("Drop files to attach them — images can go inline")
            color: Theme.text
            font.pixelSize: Theme.fontMedium
        }
    }
}
