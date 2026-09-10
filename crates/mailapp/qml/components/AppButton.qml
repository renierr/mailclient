import QtQuick
import QtQuick.Controls

import Mailclient

// The one button in the app. Three intents, one radius, one type size.
//
// Plain `Button` picks up the Basic style's own look -- light background,
// square corners -- which is what made dialogs read as a different app than
// the panes behind them.
Button {
    id: control

    // "primary" = the affirmative action, "danger" = destructive,
    // "quiet" = secondary/cancel.
    property string intent: "quiet"

    implicitHeight: 32
    hoverEnabled: true
    font.pixelSize: Theme.fontBase
    font.bold: intent === "primary"

    readonly property color baseColor: intent === "primary" ? Theme.accent
                                     : intent === "danger" ? Theme.danger
                                     : Theme.bgRaised

    background: Rectangle {
        radius: Theme.radius
        color: !control.enabled ? Theme.border
             : control.pressed ? Qt.darker(control.baseColor, 1.25)
             : control.hovered ? Qt.lighter(control.baseColor, control.intent === "quiet" ? 1.08 : 1.12)
             : control.baseColor
        border.width: control.intent === "quiet" ? 1 : 0
        border.color: Theme.border
    }

    contentItem: Text {
        text: control.text
        font: control.font
        color: !control.enabled ? Theme.textMuted
             : control.intent === "quiet" ? Theme.text
             : Theme.accentText
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        leftPadding: Theme.md
        rightPadding: Theme.md
        elide: Text.ElideRight
    }
}
