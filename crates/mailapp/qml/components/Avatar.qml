import QtQuick

// Circle with the sender's initial.
Rectangle {
    id: root
    property string initials: "?"

    width: 36
    height: 36
    radius: 18
    opacity: 1
    color: Qt.hsla(((initials.length > 0 ? initials.charCodeAt(0) : 63) * 47 % 360) / 360, 0.45, 0.55, 1)

    Text {
        anchors.centerIn: parent
        text: root.initials
        color: "white"
        font.bold: true
        font.pixelSize: 16
    }
}
