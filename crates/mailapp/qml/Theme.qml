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

    // Interface scale (Settings → Interface). Qt already applies the
    // compositor scale; this multiplies on top for users who want the app
    // itself larger (desktop text-size settings don't reach Qt). Type,
    // text-bearing heights and fixed control boxes scale — spacing, radii
    // and dialog widths stay put so layouts never clip. Supported steps:
    // 100% | 110% | 125% | 150% (see `normalize_ui_scale`).
    property real uiScale: 1.0

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

    readonly property int rowHeight: Math.round(34 * uiScale)
    readonly property int listItemHeight: Math.round(78 * uiScale)
    readonly property int toolbarHeight: Math.round(48 * uiScale)

    // Fixed control boxes holding scaled text (buttons, inputs, combos,
    // icon buttons, checkboxes, pills). Everything text-bearing must read
    // one of these instead of a literal, or 150% clips.
    readonly property int controlHeight: Math.round(32 * uiScale)
    readonly property int miniButton: Math.round(24 * uiScale)
    readonly property int checkSize: Math.round(18 * uiScale)
    readonly property int pillHeight: Math.round(20 * uiScale)

    // --- type -------------------------------------------------------------
    readonly property int fontTiny: Math.round(11 * uiScale)
    readonly property int fontSmall: Math.round(12 * uiScale)
    readonly property int fontBase: Math.round(13 * uiScale)
    readonly property int fontMedium: Math.round(15 * uiScale)
    readonly property int fontTitle: Math.round(20 * uiScale)

    // Deterministic avatar colour per sender (same name, same colour).
    function avatarColor(seed) {
        var s = seed || "?"
        var h = 0
        for (var i = 0; i < s.length; i++)
            h = (h * 31 + s.charCodeAt(i)) % 360
        return Qt.hsla(h / 360, dark ? 0.42 : 0.5, dark ? 0.46 : 0.52, 1)
    }
}
