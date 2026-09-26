pragma Singleton

import QtQuick

// Real vector icons, one definition. The bundled Material Icons font
// (see fonts/ATTRIBUTION.md, loaded once by Main.qml) renders these
// codepoints — no text-symbol idiom, no emoji-font roulette, identical on
// every machine. Every entry was verified against the bundled TTF
// (upstream codepoints can be newer than a given font cut).
//
// Usage: `AppIcon { glyph: Icons.replyAll }`, or `IconButton {
// text: Icons.sync; iconFont: true }`.
QtObject {
    id: icons

    /// The family name the loader registers. Anything rendering a glyph
    /// below must use it, or the codepoint shows whatever the UI font has
    /// (usually nothing).
    readonly property string fontFamily: "Material Icons"

    readonly property string add: "\ue145"
    readonly property string archive: "\ue149"
    readonly property string arrowBack: "\ue5c4"
    readonly property string attachFile: "\ue226"
    readonly property string block: "\ue14b"
    readonly property string checkBox: "\ue834"
    readonly property string checkBoxBlank: "\ue835"
    readonly property string chevronRight: "\ue5cc"
    readonly property string clear: "\ue14c"
    readonly property string close: "\ue5cd"
    readonly property string closeFullscreen: "\uf1cf"
    readonly property string contacts: "\ue0ba"
    // `delete` alone is an ECMAScript reserved word and cannot be a
    // property name; the trash-can icon lives under `trash`.
    readonly property string trash: "\ue872"
    readonly property string deleteForever: "\ue92b"
    readonly property string deleteSweep: "\ue16c"
    readonly property string deselect: "\uebb6"
    readonly property string done: "\ue876"
    readonly property string drafts: "\ue151"
    readonly property string driveFileMove: "\ue675"
    readonly property string edit: "\ue3c9"
    readonly property string editNote: "\ue745"
    readonly property string expandLess: "\ue5ce"
    readonly property string expandMore: "\ue5cf"
    readonly property string fileDownload: "\ue2c4"
    readonly property string flag: "\ue153"
    readonly property string folder: "\ue2c7"
    readonly property string formatClear: "\ue239"
    readonly property string formatListBulleted: "\ue241"
    readonly property string formatQuote: "\ue244"
    readonly property string forward: "\ue154"
    readonly property string imageBlocked: "\uf116"
    readonly property string inbox: "\ue156"
    readonly property string info: "\ue88e"
    readonly property string link: "\ue157"
    readonly property string mail: "\ue0e1"
    readonly property string markRead: "\uf18c"
    readonly property string markUnread: "\uf18a"
    readonly property string markUnreadAlt: "\ue159"
    readonly property string menu: "\ue5d2"
    readonly property string moreVert: "\ue5d4"
    readonly property string openFullscreen: "\uf1ce"
    readonly property string openInNew: "\ue89e"
    readonly property string outbox: "\uef5f"
    readonly property string personAdd: "\ue7fe"
    readonly property string person: "\ue7ff"
    // `print` is reserved (the logging function); unused, kept for
    // completeness under a safe name.
    readonly property string printer: "\ue8ad"
    readonly property string refresh: "\ue5d5"
    readonly property string reply: "\ue15e"
    readonly property string replyAll: "\ue15f"
    readonly property string save: "\ue161"
    readonly property string saveAlt: "\ue171"
    readonly property string schedule: "\ue8b5"
    readonly property string search: "\ue8b6"
    readonly property string selectAll: "\ue162"
    readonly property string send: "\ue163"
    readonly property string settings: "\ue8b8"
    readonly property string sort: "\ue164"
    readonly property string star: "\ue838"
    readonly property string starBorder: "\ue83a"
    readonly property string swapHoriz: "\ue8d4"
    readonly property string sync: "\ue627"
    readonly property string unfoldMore: "\ue5d7"

    /// Current-account dot in the account switcher. A drawn shape, not an
    /// icon — it stays a text glyph on purpose.
    readonly property string currentDot: "●"
}
