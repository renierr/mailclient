import QtQuick
import QtQuick.Controls

import Mailclient

// A MenuItem with a real vector icon that styles itself.
//
// Why here and not in AppMenu's `delegate`: a Menu instantiates its
// delegate only for model-driven items — statically declared children are
// used directly with their own visuals (verified headless: the delegate
// never runs for them, so delegate tricks silently do nothing). Each item
// therefore carries its own contentItem and background; AppMenu only
// provides the popup chrome.
MenuItem {
    id: root

    /// An `Icons.*` glyph, or "" for no icon.
    property string glyph: ""
    /// The visible label, usually a `qsTr()` (dynamic conditions allowed).
    property string label: ""

    // The accessible name. The visible rows below render `label` themselves.
    text: label
    implicitHeight: 32

    contentItem: Row {
        spacing: Theme.sm

        // Fixed cell so icons line up down the menu. Collapses when the
        // item carries no glyph.
        Text {
            visible: root.glyph !== ""
            width: visible ? 20 : 0
            text: root.glyph
            font.family: Icons.fontFamily
            font.pixelSize: Theme.fontMedium
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textMuted
            opacity: root.enabled ? 1.0 : 0.5
            elide: Text.ElideRight
        }
        Text {
            text: root.label
            width: root.availableWidth - (root.glyph !== "" ? 20 + parent.spacing : 0)
            color: root.enabled ? Theme.text : Theme.textMuted
            font.pixelSize: Theme.fontBase
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }

    background: Rectangle {
        radius: Theme.radius
        color: root.highlighted ? Theme.hover : "transparent"
    }
}
