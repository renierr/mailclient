import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Settings dialog, Roundcube-style: section navigation on the left, a
// scrollable detail pane on the right, Cancel/Save at the bottom.
// Explicit sync on open/save: controls bind to LOCAL copies, Save commits
// them to the shared SettingsBridge (+ sort via Bridge) and persists — so
// Cancel truly reverts instead of leaving half-applied in-memory state.
Dialog {
    id: root
    title: qsTr("Settings")
    modal: true
    width: Math.min(parent ? parent.width - 40 : 780, 780)
    height: Math.min(parent ? parent.height - 60 : 620, 620)
    anchors.centerIn: parent
    padding: Theme.lg

    signal statusMessage(string text)

    // Shared bridges owned by Main (single source of truth).
    property var settingsBridge
    property var backend
    // Passed in from Main: the DB path lives on Bridge, not SettingsBridge.
    property string dbPath: ""

    // Local edit copies (committed on Save only).
    property bool localSentCopy: true
    property bool localRemoteImages: false
    property string localSendFormat: "auto"
    property bool localIncludePlain: true
    property bool localAutoMark: true
    property int localMarkDelay: 0
    property bool localCollectContacts: true
    property bool localConfirmDelete: true
    property string localDensity: "comfortable"
    property string localReaderFont: "normal"
    property int localSyncInterval: 0
    property bool localSigEnabled: false
    property string localSigText: ""
    property bool localReplyBelow: false
    property bool localRequestMdn: false
    property string localSortField: "date"
    property bool localSortDesc: true

    // Caption + control + optional help, stacked full-width so nothing can
    // overflow on narrow panes.
    component ChoiceRow: ColumnLayout {
        property string caption: ""
        property string help: ""
        property alias model: combo.model
        property alias currentIndex: combo.currentIndex
        signal chosen(int index)
        spacing: Theme.xs
        Layout.fillWidth: true
        Layout.topMargin: Theme.sm
        Label {
            text: caption
            color: Theme.text
            font.pixelSize: Theme.fontBase
        }
        AppComboBox {
            id: combo
            Layout.fillWidth: true
            onActivated: index => chosen(index)
        }
        Label {
            visible: help !== ""
            text: help
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    component HintLabel: Label {
        color: Theme.textMuted
        font.pixelSize: Theme.fontSmall
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    component SectionCaption: Label {
        color: Theme.textMuted
        font.pixelSize: Theme.fontTiny
        font.bold: true
        font.letterSpacing: 1
        Layout.topMargin: Theme.sm
    }

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        implicitHeight: 48
        color: "transparent"
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: Theme.lg
            text: root.title
            color: Theme.text
            font.pixelSize: Theme.fontMedium
            font.bold: true
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    footer: RowLayout {
        spacing: Theme.sm
        Item { Layout.fillWidth: true }
        AppButton {
            text: qsTr("Cancel")
            onClicked: root.reject()
        }
        AppButton {
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            Layout.topMargin: Theme.sm
            text: qsTr("Save")
            intent: "primary"
            onClicked: root.accept()
        }
    }

    function formatIndex(v) {
        if (v === "plain")
            return 1
        if (v === "multipart")
            return 2
        if (v === "html")
            return 3
        return 0
    }

    function delayIndex(secs) {
        var steps = [0, 3, 5, 10, 30]
        var idx = steps.indexOf(secs)
        return idx >= 0 ? idx : 0
    }

    function delaySecs(idx) {
        return [0, 3, 5, 10, 30][idx] || 0
    }

    function syncIndex(mins) {
        var steps = [0, 5, 10, 15, 30, 60]
        var idx = steps.indexOf(mins)
        return idx >= 0 ? idx : 0
    }

    function syncMins(idx) {
        return [0, 5, 10, 15, 30, 60][idx] || 0
    }

    function indexOr(list, value, fallback) {
        var idx = list.indexOf(value)
        return idx >= 0 ? idx : fallback
    }

    onOpened: {
        settingsBridge.load()
        root.localSentCopy = settingsBridge.sent_copy_enabled
        root.localRemoteImages = settingsBridge.load_remote_images
        root.localSendFormat = settingsBridge.compose_send_format
        root.localIncludePlain = settingsBridge.compose_include_plain
        root.localAutoMark = settingsBridge.auto_mark_read
        root.localMarkDelay = settingsBridge.mark_read_delay_secs
        root.localCollectContacts = settingsBridge.collect_sent_contacts
        root.localConfirmDelete = settingsBridge.confirm_delete
        root.localDensity = settingsBridge.list_density
        root.localReaderFont = settingsBridge.reader_font_size
        root.localSyncInterval = settingsBridge.sync_interval_minutes
        root.localSigEnabled = settingsBridge.signature_enabled
        root.localSigText = settingsBridge.signature_text
        root.localReplyBelow = settingsBridge.reply_below_quote
        root.localRequestMdn = settingsBridge.request_mdn
        root.localSortField = root.backend ? root.backend.sort_field : "date"
        root.localSortDesc = root.backend ? root.backend.sort_descending : true
    }

    RowLayout {
        anchors.fill: parent
        spacing: Theme.md

        // --- section navigation --------------------------------------
        ListView {
            id: nav
            Layout.preferredWidth: 168
            Layout.fillHeight: true
            clip: true
            spacing: 2
            model: ListModel {
                ListElement { icon: "📥"; label: qsTr("Mailbox") }
                ListElement { icon: "📖"; label: qsTr("Reading") }
                ListElement { icon: "✏️"; label: qsTr("Composing") }
                ListElement { icon: "☁️"; label: qsTr("Accounts & sync") }
            }
            delegate: Item {
                id: navItem
                required property string icon
                required property string label
                required property int index
                width: nav.width
                height: 38
                Rectangle {
                    anchors.fill: parent
                    radius: Theme.radius
                    color: nav.currentIndex === navItem.index ? Theme.selected : "transparent"
                }
                Rectangle {
                    width: 3
                    height: parent.height - 10
                    anchors.left: parent.left
                    anchors.leftMargin: 4
                    anchors.verticalCenter: parent.verticalCenter
                    radius: 2
                    color: Theme.accent
                    visible: nav.currentIndex === navItem.index
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    spacing: Theme.sm
                    Label { text: navItem.icon }
                    Label {
                        Layout.fillWidth: true
                        text: navItem.label
                        color: nav.currentIndex === navItem.index ? Theme.text : Theme.textMuted
                        font.pixelSize: Theme.fontBase
                        font.bold: nav.currentIndex === navItem.index
                        elide: Text.ElideRight
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: nav.currentIndex = navItem.index
                }
            }
        }

        Rectangle {
            Layout.preferredWidth: 1
            Layout.fillHeight: true
            color: Theme.border
        }

        // --- scrollable detail panes ----------------------------------
        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: nav.currentIndex

            // Mailbox view.
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ColumnLayout {
                    width: parent.availableWidth
                    spacing: Theme.sm

                    SectionCaption { text: qsTr("MAILBOX VIEW") }

                    ChoiceRow {
                        caption: qsTr("Sort messages by")
                        model: [qsTr("Date"), qsTr("Sender"), qsTr("Subject")]
                        currentIndex: indexOr(["date", "from", "subject"], root.localSortField, 0)
                        onChosen: index => {
                            root.localSortField = ["date", "from", "subject"][index]
                        }
                    }
                    ChoiceRow {
                        caption: qsTr("Order")
                        model: [qsTr("Newest first"), qsTr("Oldest first")]
                        currentIndex: root.localSortDesc ? 0 : 1
                        onChosen: index => {
                            root.localSortDesc = index === 0
                        }
                    }
                    ChoiceRow {
                        caption: qsTr("Density")
                        model: [qsTr("Comfortable"), qsTr("Compact")]
                        currentIndex: root.localDensity === "compact" ? 1 : 0
                        help: qsTr("Compact hides the preview line and tightens the rows.")
                        onChosen: index => {
                            root.localDensity = index === 1 ? "compact" : "comfortable"
                        }
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.sm
                        checked: root.localConfirmDelete
                        text: qsTr("Confirm before moving mail to Trash")
                        onToggled: root.localConfirmDelete = checked
                    }
                    HintLabel {
                        text: qsTr("Single deletes, bulk deletes and the Delete key ask first. Permanent deletes always ask.")
                    }
                }
            }

            // Reading mail.
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ColumnLayout {
                    width: parent.availableWidth
                    spacing: Theme.sm

                    SectionCaption { text: qsTr("READING MAIL") }

                    AppCheckBox {
                        Layout.fillWidth: true
                        checked: root.localAutoMark
                        text: qsTr("Automatically mark messages as read when viewed")
                        onToggled: root.localAutoMark = checked
                    }
                    ChoiceRow {
                        caption: qsTr("Mark as read")
                        enabled: root.localAutoMark
                        model: [qsTr("Immediately"), qsTr("After 3 seconds"), qsTr("After 5 seconds"), qsTr("After 10 seconds"), qsTr("After 30 seconds")]
                        currentIndex: delayIndex(root.localMarkDelay)
                        help: qsTr("With a delay, only messages still open when the timer elapses count as read. Right-click any message to mark it read or unread manually.")
                        onChosen: index => {
                            root.localMarkDelay = delaySecs(index)
                        }
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.sm
                        checked: root.localRemoteImages
                        text: qsTr("Load remote images in HTML mail (not recommended)")
                        onToggled: root.localRemoteImages = checked
                    }
                    HintLabel {
                        text: qsTr("Remote images can track opens. Blocked images still offer a one-click “Show once” banner per message.")
                    }
                    ChoiceRow {
                        caption: qsTr("Text size")
                        model: [qsTr("Small"), qsTr("Normal"), qsTr("Large")]
                        currentIndex: indexOr(["small", "normal", "large"], root.localReaderFont, 1)
                        help: qsTr("Applies to plain-text mail; HTML mail brings its own sizes.")
                        onChosen: index => {
                            root.localReaderFont = ["small", "normal", "large"][index]
                        }
                    }
                }
            }

            // Composing mail.
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ColumnLayout {
                    width: parent.availableWidth
                    spacing: Theme.sm

                    SectionCaption { text: qsTr("COMPOSING MAIL") }

                    ChoiceRow {
                        caption: qsTr("Send mail as")
                        model: [qsTr("Automatic (recommended)"), qsTr("Plain text (safest)"), qsTr("Multipart plain + HTML"), qsTr("HTML only")]
                        currentIndex: formatIndex(root.localSendFormat)
                        onChosen: index => {
                            root.localSendFormat = ["auto", "plain", "multipart", "html"][index]
                        }
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        checked: root.localIncludePlain
                        text: qsTr("Always include a plain-text version alongside HTML")
                        onToggled: root.localIncludePlain = checked
                    }
                    HintLabel {
                        text: qsTr("Automatic sends plain text unless the message uses formatting (bold, links, lists, quotes); attachments always travel as multipart. The plain-text twin keeps every client readable.")
                    }
                    ChoiceRow {
                        caption: qsTr("Replies start")
                        model: [qsTr("Above the quote"), qsTr("Below the quote")]
                        currentIndex: root.localReplyBelow ? 1 : 0
                        onChosen: index => {
                            root.localReplyBelow = index === 1
                        }
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.sm
                        checked: root.localSigEnabled
                        text: qsTr("Use a signature")
                        onToggled: root.localSigEnabled = checked
                    }
                    TextArea {
                        id: sigArea
                        Layout.fillWidth: true
                        Layout.preferredHeight: 96
                        enabled: root.localSigEnabled
                        text: root.localSigText
                        wrapMode: TextArea.Wrap
                        selectByMouse: true
                        color: Theme.text
                        placeholderText: qsTr("Kind regards, …")
                        placeholderTextColor: Theme.textMuted
                        font.pixelSize: Theme.fontBase
                        onTextChanged: root.localSigText = text
                        background: Rectangle {
                            radius: Theme.radius
                            color: Theme.bg
                            border.width: 1
                            border.color: sigArea.activeFocus ? Theme.accent : Theme.border
                        }
                    }
                    HintLabel {
                        text: qsTr("Added to new mail, replies and forwards, separated by “-- ”.")
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.sm
                        checked: root.localRequestMdn
                        text: qsTr("Request a read receipt")
                        onToggled: root.localRequestMdn = checked
                    }
                    HintLabel {
                        text: qsTr("Adds a receipt-request header to sent mail. Recipients may ignore it; it only asks.")
                    }
                }
            }

            // Accounts & sync.
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ColumnLayout {
                    width: parent.availableWidth
                    spacing: Theme.sm

                    SectionCaption { text: qsTr("ACCOUNTS & SYNC") }

                    AppCheckBox {
                        Layout.fillWidth: true
                        checked: root.localSentCopy
                        text: qsTr("Save a copy of sent mail in Sent")
                        onToggled: root.localSentCopy = checked
                    }
                    AppCheckBox {
                        Layout.fillWidth: true
                        checked: root.localCollectContacts
                        text: qsTr("Suggest recipients from sent mail")
                        onToggled: root.localCollectContacts = checked
                    }
                    HintLabel {
                        text: qsTr("Addresses you sent to are suggested while composing.")
                    }
                    ChoiceRow {
                        caption: qsTr("Check for new mail")
                        model: [qsTr("Manually"), qsTr("Every 5 minutes"), qsTr("Every 10 minutes"), qsTr("Every 15 minutes"), qsTr("Every 30 minutes"), qsTr("Every hour")]
                        currentIndex: syncIndex(root.localSyncInterval)
                        help: qsTr("Automatic checks only run while the app is idle, never mid-action.")
                        onChosen: index => {
                            root.localSyncInterval = syncMins(index)
                        }
                    }

                    SectionCaption { text: qsTr("ABOUT") }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Database: %1").arg(root.dbPath)
                        color: Theme.textMuted
                        elide: Text.ElideLeft
                        font.pixelSize: Theme.fontSmall
                    }
                }
            }
        }
    }

    onAccepted: {
        settingsBridge.sent_copy_enabled = root.localSentCopy
        settingsBridge.load_remote_images = root.localRemoteImages
        settingsBridge.compose_send_format = root.localSendFormat
        settingsBridge.compose_include_plain = root.localIncludePlain
        settingsBridge.auto_mark_read = root.localAutoMark
        settingsBridge.mark_read_delay_secs = root.localMarkDelay
        settingsBridge.collect_sent_contacts = root.localCollectContacts
        settingsBridge.confirm_delete = root.localConfirmDelete
        settingsBridge.list_density = root.localDensity
        settingsBridge.reader_font_size = root.localReaderFont
        settingsBridge.sync_interval_minutes = root.localSyncInterval
        settingsBridge.signature_enabled = root.localSigEnabled
        settingsBridge.signature_text = root.localSigText
        settingsBridge.reply_below_quote = root.localReplyBelow
        settingsBridge.request_mdn = root.localRequestMdn
        settingsBridge.save()
        // Sort lives on Bridge (shared with the list header menu).
        if (root.backend && root.backend.set_sort
                && (root.localSortField !== root.backend.sort_field
                    || root.localSortDesc !== root.backend.sort_descending))
            root.backend.set_sort(root.localSortField, root.localSortDesc)
        root.statusMessage(qsTr("Settings saved"))
    }
}
