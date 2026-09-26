import QtQuick
import QtQuick.Controls

import Mailclient

// Themed popup menu. Items are `AppMenuItem`s, which style themselves (a
// Menu only instantiates its `delegate` for model-driven items - static
// children render their own visuals, so item styling lives in AppMenuItem
// and this file is just the popup chrome).
Menu {
    id: control

    implicitWidth: 240
    padding: Theme.xs

    background: Rectangle {
        radius: Theme.radius
        color: Theme.bgRaised
        border.width: 1
        border.color: Theme.border
    }
}
