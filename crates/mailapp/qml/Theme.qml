pragma Singleton

import QtQuick

// Design tokens — the single source of truth for colour, spacing, radius and
// type. Every pane reads these instead of hardcoding values, which is what
// keeps the app looking like one product (flaw F6).
//
// Why tokens rather than the platform palette: Qt Quick Controls' native
// Windows style ignores most customisation and looks nothing like the Linux
// build, so `main.rs` pins the Basic style and we paint everything ourselves.
// Identical rendering on Omarchy and Windows is the point.
QtObject {
    id: theme

    // Follows the desktop's light/dark preference; falls back to light.
    readonly property bool dark: Application.styleHints.colorScheme === Qt.Dark

    // --- surfaces ---------------------------------------------------------
    readonly property color bg: dark ? "#16181d" : "#ffffff"
    readonly property color bgAlt: dark ? "#1b1e24" : "#f7f8fa"
    readonly property color bgRaised: dark ? "#22262d" : "#ffffff"
    readonly property color border: dark ? "#2c323b" : "#e3e6ea"
    readonly property color hover: dark ? "#232830" : "#f1f3f6"
    readonly property color selected: dark ? "#25314a" : "#e7efff"

    // --- content ----------------------------------------------------------
    readonly property color text: dark ? "#e6e8eb" : "#1c1f23"
    readonly property color textMuted: dark ? "#98a1ad" : "#6b7280"
    readonly property color accent: dark ? "#5c93ff" : "#2f6fed"
    readonly property color accentText: "#ffffff"
    readonly property color star: "#f0a92c"
    readonly property color danger: dark ? "#f2777a" : "#c8342f"

    // --- metrics ----------------------------------------------------------
    readonly property int xs: 4
    readonly property int sm: 8
    readonly property int md: 12
    readonly property int lg: 16
    readonly property int xl: 24

    readonly property int radius: 6
    readonly property int radiusLg: 10

    readonly property int rowHeight: 34
    readonly property int listItemHeight: 78
    readonly property int toolbarHeight: 48

    // --- type -------------------------------------------------------------
    readonly property int fontTiny: 11
    readonly property int fontSmall: 12
    readonly property int fontBase: 13
    readonly property int fontMedium: 15
    readonly property int fontTitle: 20

    // Deterministic avatar colour per sender (same name, same colour).
    function avatarColor(seed) {
        var s = seed || "?"
        var h = 0
        for (var i = 0; i < s.length; i++)
            h = (h * 31 + s.charCodeAt(i)) % 360
        return Qt.hsla(h / 360, dark ? 0.42 : 0.5, dark ? 0.46 : 0.52, 1)
    }
}
