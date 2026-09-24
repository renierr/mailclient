import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

import Mailclient
import "components"

// Compose dialog: real WYSIWYG body (components/EditorFrame.qml) with an
// HTML-source toggle.
//
// The toolbar drives `document.execCommand`, so text changes visibly as you
// press B/I/U and the buttons light up to show what the caret sits inside
// (flaw F3). It also emits semantic <b>/<i>/<u> tags, which the outgoing
// sanitizer keeps — Qt's rich-text TextArea emitted inline styles that were
// stripped on send, so formatting silently never arrived.
//
// Rust (`resolve_bodies` + the `compose_send_format`/`compose_include_plain`
// settings) derives the plain/multipart shape and sanitizes the HTML:
// Auto sends text/plain unless the body carries real formatting.
Dialog {
    id: root
    title: qsTr("Compose")
    modal: true
    parent: Overlay.overlay
    padding: Theme.lg
    closePolicy: Popup.NoAutoClose

    readonly property int minWidth: 520
    readonly property int minHeight: 360
    readonly property int preferredW: 760
    readonly property int preferredH: 640

    property real wantW: preferredW
    property real wantH: preferredH
    property real wantX: 0
    property real wantY: 0
    property bool positioned: false

    readonly property real hostW: parent ? parent.width : preferredW
    readonly property real hostH: parent ? parent.height : preferredH
    readonly property int gapX: hostW < 900 ? 16 : 48
    readonly property int gapY: hostH < 700 ? 16 : 32
    readonly property real maxW: Math.max(240, hostW - gapX)
    readonly property real maxH: Math.max(200, hostH - gapY)

    width: Math.min(Math.max(wantW, Math.min(minWidth, maxW)), maxW)
    height: Math.min(Math.max(wantH, Math.min(minHeight, maxH)), maxH)
    x: positioned ? Math.round(Math.max(0, Math.min(wantX, hostW - width)))
                  : Math.round((hostW - width) / 2)
    y: positioned ? Math.round(Math.max(0, Math.min(wantY, hostH - height)))
                  : Math.round((hostH - height) / 2)

    function applyDefaultGeometry() {
        wantW = preferredW
        wantH = preferredH
        positioned = false
    }

    function applyResize(edges, sX, sY, sW, sH, dx, dy) {
        var nx = sX
        var ny = sY
        var nw = sW
        var nh = sH
        var minW = Math.min(minWidth, maxW)
        var minH = Math.min(minHeight, maxH)
        if (edges & Qt.RightEdge)
            nw = Math.min(Math.max(minW, sW + dx), hostW - nx)
        if (edges & Qt.LeftEdge) {
            nw = Math.min(Math.max(minW, sW - dx), sX + sW)
            nx = sX + sW - nw
        }
        if (edges & Qt.BottomEdge)
            nh = Math.min(Math.max(minH, sH + dy), hostH - ny)
        if (edges & Qt.TopEdge) {
            nh = Math.min(Math.max(minH, sH - dy), sY + sH)
            ny = sY + sH - nh
        }
        positioned = true
        wantX = nx
        wantY = ny
        wantW = nw
        wantH = nh
    }

    onOpened: applyDefaultGeometry()

    signal statusMessage(string text)
    signal sendRequested(string payload)
    signal saveDraftRequested(string payload)

    property string accountEmail: ""
    property string accountFromName: ""
    property string sendFormat: "auto"
    property var backend
    property bool collectContacts: true
    // Signature + reply placement, bound to settings via Main.
    property bool signatureEnabled: false
    property string signatureText: ""
    property bool replyBelowQuote: false
    property bool sourceMode: false
    // UID of the server draft being edited; -1 means a new draft.
    property int draftUid: -1

    // Cc/Bcc rows stay collapsed until toggled (or non-empty).
    property bool showCc: false
    property bool showBcc: false
    // Reply-To for our mail ("replies go here instead of From"): collapsed
    // beside From until toggled (or non-empty, e.g. a reopened draft).
    property bool showReplyTo: false

    // Incoming Reply-To that points elsewhere than From, for the mail being
    // answered: shown as a banner so the different recipient is obvious.
    // Cleared on reset; the banner hides itself once the user edits To to
    // something else (they took control of the recipient).
    property string replyNoticeAddr: ""
    property string replyNoticeSender: ""

    // Outgoing files picked via FileDialog: [{path, name}]. Paths (plain or
    // `file://` URLs) travel in the send payload; Rust reads the bytes at
    // send time, so no binary crosses the QML bridge.
    property var attachments: []

    // Flaw F5: Cancel used to throw the draft away silently. Everything the
    // user types sets this, and closing then asks first.
    property bool dirty: false

    // A send of this composition is queued but SMTP has not accepted it yet.
    // The dialog is already closed, but its fields still hold the text a
    // failure reopens — so no new composition may reuse them until then, and
    // a result only acts on the composer while this is still set.
    property bool sendPending: false
    // A draft save of this composition is running. Editing is locked until it
    // reports back, so closing on success can never drop newer typing.
    property bool saving: false

    // The domain is fixed to the account: only the local part is editable,
    // since sending as another domain breaks SPF and domain-aligned
    // DKIM/DMARC authentication.
    readonly property string accountDomain: {
        var at = root.accountEmail.indexOf("@")
        return at < 0 ? "" : root.accountEmail.substring(at)
    }
    readonly property string accountLocalPart: {
        var at = root.accountEmail.indexOf("@")
        return at < 0 ? root.accountEmail : root.accountEmail.substring(0, at)
    }
    readonly property string effectiveFrom:
        fromLocal.text.trim() === "" ? root.accountEmail
                                     : fromLocal.text.trim() + root.accountDomain

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
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            anchors.rightMargin: Theme.lg
            text: root.sendFormat === "auto" ? qsTr("Send as: Auto") : qsTr("Send as: %1").arg(root.sendFormat)
            color: Theme.textMuted
            font.pixelSize: Theme.fontTiny
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }

        MouseArea {
            id: dragArea
            anchors.fill: parent
            cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
            property real sMX
            property real sMY
            property real sX
            property real sY
            onPressed: function (mouse) {
                var p = mapToItem(root.parent, mouse.x, mouse.y)
                sMX = p.x
                sMY = p.y
                sX = root.x
                sY = root.y
                root.positioned = true
            }
            onPositionChanged: function (mouse) {
                if (!pressed)
                    return
                var p = mapToItem(root.parent, mouse.x, mouse.y)
                root.wantX = sX + p.x - sMX
                root.wantY = sY + p.y - sMY
            }
        }
    }

    function markClean() { root.dirty = false }

    // Every open* resets the one shared set of fields; refuse while an
    // earlier composition still needs them (see sendPending / saving).
    function readyForNew() {
        if (root.sendPending) {
            root.statusMessage(qsTr("Still sending the previous message — try again in a moment"))
            return false
        }
        if (root.saving) {
            root.statusMessage(qsTr("Still saving the draft — try again in a moment"))
            return false
        }
        return true
    }

    function requestClose() {
        if (root.dirty)
            discardConfirm.open()
        else
            root.close()
    }

    function escapeHtml(t) {
        return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    }

    // Plain-text quote as `>` citations: renders in every client and keeps
    // Auto sends as text/plain unless the user adds real formatting.
    function plainToHtmlQuote(t) {
        var lines = root.escapeHtml(t).split("\n")
        for (var i = 0; i < lines.length; i++)
            lines[i] = "&gt; " + lines[i]
        return "<p>" + lines.join("<br>") + "</p>"
    }

    // Quote in the format the original arrived in: HTML mail gets a styled
    // <blockquote> of its HTML body, plain mail gets `>` citations of its
    // text. The toolbar Quote button still inserts a styled blockquote on
    // explicit request. Bodies come pre-sanitized from the Rust feed and are
    // sanitized again on send. Gapless: callers add the spacing that fits
    // the reply placement (above vs below the quote).
    function quoteCore(message, headerText) {
        var wasHtml = message.is_html === true
        var html = message.body_html !== undefined ? message.body_html : ""
        if (wasHtml && html !== "")
            return "<p>" + root.escapeHtml(headerText) + "</p><blockquote>" + html + "</blockquote>"
        var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
        return root.plainToHtmlQuote(headerText + "\n" + q)
    }

    // Signature block with the standard `-- ` separator, or "" when the
    // setting is off/blank. Plain text with <br> so it survives both the
    // rich editor and plain-text sends.
    function signatureHtml() {
        if (!root.signatureEnabled)
            return ""
        var lines = root.signatureText.split("\n")
        while (lines.length > 0 && lines[lines.length - 1].trim() === "")
            lines.pop()
        while (lines.length > 0 && lines[0].trim() === "")
            lines.shift()
        if (lines.length === 0)
            return ""
        return "<p>-- <br>" + root.escapeHtml(lines.join("\n")).split("\n").join("<br>") + "</p>"
    }

    function resetHeaders() {
        fromLocal.text = root.accountLocalPart
        fromName.text = root.accountFromName
        toField.text = ""
        ccField.text = ""
        bccField.text = ""
        replyToField.text = ""
        subjectField.text = ""
        root.showCc = false
        root.showBcc = false
        root.showReplyTo = false
        root.replyNoticeAddr = ""
        root.replyNoticeSender = ""
        root.attachments = []
        root.draftUid = -1
    }

    function setBody(html) {
        bodyEditor.setHtml(html)
        sourceArea.text = html
    }

    function openBlank() {
        if (!root.readyForNew())
            return
        root.sourceMode = false
        root.resetHeaders()
        root.setBody(root.signatureHtml())
        root.markClean()
        open()
    }

    function openForReply(message) {
        if (!root.readyForNew())
            return
        root.sourceMode = false
        root.resetHeaders()
        if (message !== undefined) {
            // Replies go to Reply-To when the sender set one, else From —
            // and when the two differ the banner below says so out loud.
            var from = message.from || ""
            var rt = (message.reply_to || "").trim()
            var differs = rt !== "" && rt.toLowerCase() !== from.trim().toLowerCase()
            toField.text = differs ? rt : from
            if (differs) {
                root.replyNoticeAddr = rt
                root.replyNoticeSender = from
            }
            subjectField.text = "Re: " + (message.subject || "")
            var core = root.quoteCore(message,
                "On " + (message.date || "") + ", " + (message.from || "") + " wrote:")
            if (root.replyBelowQuote)
                root.setBody(core + "<p></p>" + root.signatureHtml())
            else
                root.setBody("<p></p>" + root.signatureHtml() + core)
        }
        root.markClean()
        open()
    }

    function openForForward(message) {
        if (!root.readyForNew())
            return
        root.sourceMode = false
        root.resetHeaders()
        if (message !== undefined) {
            subjectField.text = "Fwd: " + (message.subject || "")
            var lead = root.signatureHtml() + "<p></p>"
            var wasHtml = message.is_html === true
            var html = message.body_html !== undefined ? message.body_html : ""
            if (wasHtml && html !== "") {
                root.setBody(lead + "<p>— Forwarded message —<br>From: "
                    + root.escapeHtml(message.from || "") + "<br>Date: "
                    + root.escapeHtml(message.date || "") + "<br>Subject: "
                    + root.escapeHtml(message.subject || "") + "</p><blockquote>" + html + "</blockquote>")
            } else {
                var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
                root.setBody(lead + root.plainToHtmlQuote("— Forwarded message —\nFrom: "
                    + (message.from || "") + "\nDate: " + (message.date || "") + "\nSubject: "
                    + (message.subject || "") + "\n\n" + q))
            }
        }
        root.markClean()
        open()
    }

    function openForDraft(draft) {
        if (!root.readyForNew())
            return
        root.sourceMode = false
        root.resetHeaders()
        root.draftUid = draft.draft_uid === undefined ? -1 : draft.draft_uid
        var from = draft.from || root.accountEmail
        var at = from.indexOf("@")
        fromLocal.text = at < 0 ? from : from.substring(0, at)
        toField.text = draft.to || ""
        ccField.text = draft.cc || ""
        bccField.text = draft.bcc || ""
        replyToField.text = draft.reply_to || ""
        root.showCc = ccField.text !== ""
        root.showBcc = bccField.text !== ""
        root.showReplyTo = replyToField.text !== ""
        subjectField.text = draft.subject || ""
        root.attachments = draft.attachments || []
        root.setBody(draft.body || "")
        root.markClean()
        open()
    }

    // Source mode shows exactly what will be sent, and edits round-trip.
    function toggleSource() {
        if (!root.sourceMode) {
            bodyEditor.fetchHtml(function (html) {
                sourceArea.text = html
                root.sourceMode = true
            })
        } else {
            bodyEditor.setHtml(sourceArea.text)
            root.sourceMode = false
        }
    }

    function baseName(url) {
        var s = url.toString()
        var i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"))
        var name = i < 0 ? s : s.substring(i + 1)
        try { name = decodeURIComponent(name) } catch (e) {}
        return name === "" ? s : name
    }

    function addAttachments(urls) {
        var next = root.attachments.slice()
        for (var i = 0; i < urls.length; i++) {
            var u = urls[i].toString()
            var known = false
            for (var j = 0; j < next.length; j++) {
                if (next[j].path === u) {
                    known = true
                    break
                }
            }
            if (!known)
                next.push({ path: u, name: root.baseName(u) })
        }
        root.attachments = next
        root.dirty = true
    }

    function removeAttachment(index) {
        var next = root.attachments.slice()
        next.splice(index, 1)
        root.attachments = next
        root.dirty = true
    }

    function payloadFor(html) {
        var paths = []
        for (var i = 0; i < root.attachments.length; i++)
            paths.push(root.attachments[i].path)
        return JSON.stringify({
            from: root.effectiveFrom,
            from_name: fromName.text.trim(),
            reply_to: replyToField.text,
            to: toField.text,
            cc: ccField.text,
            bcc: bccField.text,
            subject: subjectField.text,
            body: html,
            body_html: html,
            attachments: paths,
            draft_uid: root.draftUid
        })
    }

    // To accepts placeholder text or stays blank: the real recipients may
    // live in Cc/Bcc alone. Only all-three-empty blocks the send.
    readonly property bool hasRecipients:
        toField.text.trim() !== "" || ccField.text.trim() !== "" || bccField.text.trim() !== ""

    // Reading the document back is asynchronous, so Send finishes inside the
    // callback rather than returning a payload.
    function requestSend() {
        if (!root.hasRecipients) {
            root.statusMessage(qsTr("Add at least one recipient (To, Cc or Bcc)"))
            return
        }
        if (root.sourceMode) {
            root.sendRequested(root.payloadFor(sourceArea.text))
        } else {
            bodyEditor.fetchHtml(function (html) {
                root.sendRequested(root.payloadFor(html))
            })
        }
    }

    function requestSaveDraft() {
        if (root.sourceMode) {
            root.saveDraftRequested(root.payloadFor(sourceArea.text))
        } else {
            bodyEditor.fetchHtml(function (html) {
                root.saveDraftRequested(root.payloadFor(html))
            })
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.sm
        enabled: !root.saving

        // --- headers ------------------------------------------------------
        // One row each for From / To / Subject; Cc and Bcc hide behind
        // toggles beside the To field until needed.
        GridLayout {
            Layout.fillWidth: true
            columns: 3
            columnSpacing: Theme.sm
            rowSpacing: Theme.xs

            Label {
                text: qsTr("From")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                Layout.preferredWidth: 52
            }
            // Sender name (per-account default, editable per mail) beside the
            // address: local part editable, domain locked to the account.
            RowLayout {
                Layout.fillWidth: true
                Layout.columnSpan: 2
                spacing: Theme.sm

                AppTextField {
                    id: fromName
                    Layout.fillWidth: true
                    Layout.preferredWidth: 140
                    text: root.accountFromName
                    placeholderText: qsTr("Name")
                    onTextChanged: root.dirty = true
                }
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredWidth: 200
                implicitHeight: Theme.controlHeight
                radius: Theme.radius
                color: Theme.bg
                border.width: 1
                border.color: fromLocal.activeFocus ? Theme.accent : Theme.border

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    spacing: 0

                    AppTextField {
                        id: fromLocal
                        Layout.fillWidth: true
                        text: root.accountLocalPart
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        placeholderTextColor: Theme.textMuted
                        selectByMouse: true
                        leftPadding: 0
                        rightPadding: 0
                        background: null
                        onTextChanged: root.dirty = true
                    }
                    Label {
                        text: root.accountDomain
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontBase
                        ToolTip.visible: domainHover.hovered
                        ToolTip.text: qsTr("Fixed to this account's domain")
                        HoverHandler { id: domainHover }
                    }
                }
            }
            // Collapsed Reply-To toggle: our mail asks replies to go to
            // another address instead of From. Optional, off by default.
            IconButton {
                text: Icons.reply
                iconFont: true
                fontSize: Theme.fontSmall
                implicitWidth: Math.round(36 * Theme.uiScale)
                implicitHeight: Theme.controlHeight
                active: root.showReplyTo || replyToField.text !== ""
                tooltip: qsTr("Set Reply-To address")
                onClicked: root.showReplyTo = !root.showReplyTo
            }
            }

            Label {
                text: qsTr("To")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            RecipientField {
                id: toField
                Layout.fillWidth: true
                backend: root.backend
                enabledSuggestions: root.collectContacts
                placeholderText: qsTr("name@example.com, … (or anything — Bcc can carry the real addresses)")
                onEdited: root.dirty = true
            }
            Row {
                spacing: 2
                IconButton {
                    text: "Cc"
                    fontSize: Theme.fontSmall
                    implicitWidth: Math.round(36 * Theme.uiScale)
                    implicitHeight: Theme.controlHeight
                    active: root.showCc || ccField.text !== ""
                    tooltip: qsTr("Show Cc field")
                    onClicked: root.showCc = !root.showCc
                }
                IconButton {
                    text: qsTr("Bcc")
                    fontSize: Theme.fontSmall
                    implicitWidth: Math.round(40 * Theme.uiScale)
                    implicitHeight: Theme.controlHeight
                    active: root.showBcc || bccField.text !== ""
                    tooltip: qsTr("Show Bcc field")
                    onClicked: root.showBcc = !root.showBcc
                }
            }

            Label {
                text: qsTr("Cc")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.showCc || ccField.text !== ""
            }
            RecipientField {
                id: ccField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                visible: root.showCc || ccField.text !== ""
                backend: root.backend
                enabledSuggestions: root.collectContacts
                placeholderText: qsTr("optional, comma separated")
                onEdited: root.dirty = true
            }

            Label {
                text: qsTr("Bcc")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.showBcc || bccField.text !== ""
            }
            RecipientField {
                id: bccField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                visible: root.showBcc || bccField.text !== ""
                backend: root.backend
                enabledSuggestions: root.collectContacts
                placeholderText: qsTr("optional, hidden recipients")
                onEdited: root.dirty = true
            }

            Label {
                text: qsTr("Reply-To")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.showReplyTo || replyToField.text !== ""
            }
            AppTextField {
                id: replyToField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                visible: root.showReplyTo || replyToField.text !== ""
                placeholderText: qsTr("replies to this mail go here instead of From (optional, one address)")
                onTextChanged: root.dirty = true
            }

            Label {
                text: qsTr("Subject")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            AppTextField {
                id: subjectField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                onTextChanged: root.dirty = true
            }
        }

        // --- reply-to notice ------------------------------------------------
        // Answering a mail whose Reply-To points elsewhere: the To field
        // alone would not say the recipient differs from the sender, so the
        // banner says it out loud. Editing To to something else dismisses
        // it (the user took control of the recipient).
        Rectangle {
            Layout.fillWidth: true
            visible: root.replyNoticeAddr !== ""
                     && toField.text.trim().toLowerCase() === root.replyNoticeAddr.toLowerCase()
            radius: Theme.radius
            color: Theme.bgAlt
            border.width: 1
            border.color: Theme.danger
            implicitHeight: noticeRow.implicitHeight + Theme.sm * 2

            RowLayout {
                id: noticeRow
                anchors.fill: parent
                anchors.margins: Theme.sm
                spacing: Theme.sm
                Label {
                    text: Icons.reply
                    font.family: Icons.fontFamily
                    color: Theme.danger
                    font.pixelSize: Theme.fontBase
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("Replies to this mail go to %1 — not to the sender (%2).").arg(root.replyNoticeAddr).arg(root.replyNoticeSender)
                }
            }
        }

        // --- attachments ----------------------------------------------------
        ComposerAttachmentTray {
            Layout.fillWidth: true
            visible: root.attachments.length > 0
            attachments: root.attachments
            onAddRequested: attachDialog.open()
            onRemoveRequested: index => root.removeAttachment(index)
        }

        // --- formatting toolbar -------------------------------------------
        ComposerToolbar {
            Layout.fillWidth: true
            sourceMode: root.sourceMode
            bodyEditor: bodyEditor
            onLinkRequested: linkDialog.open()
            onAttachRequested: attachDialog.open()
            onToggleSourceRequested: root.toggleSource()
        }

        // --- body ---------------------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.radius
            color: Theme.bg
            border.width: 1
            border.color: Theme.border
            clip: true

            EditorFrame {
                id: bodyEditor
                anchors.fill: parent
                anchors.margins: 1
                visible: !root.sourceMode
                onContentChanged: root.dirty = true
            }

            ScrollView {
                anchors.fill: parent
                anchors.margins: 1
                visible: root.sourceMode
                clip: true

                TextArea {
                    id: sourceArea
                    placeholderText: qsTr("HTML source…")
                    wrapMode: TextArea.Wrap
                    textFormat: TextArea.PlainText
                    font.family: "monospace"
                    font.pixelSize: Theme.fontSmall
                    color: Theme.text
                    placeholderTextColor: Theme.textMuted
                    selectByMouse: true
                    background: null
                    onTextChanged: root.dirty = true
                }
            }
        }

        // --- actions ------------------------------------------------------
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm
            Label {
                text: root.dirty ? qsTr("Unsaved draft") : ""
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
            }
            Item { Layout.fillWidth: true }
            // Server drafts get an explicit delete: closing (Discard) only
            // ever abandons local edits, it never destroys the server copy.
            IconButton {
                visible: root.draftUid >= 0
                text: Icons.trash
                iconFont: true
                contentColor: Theme.danger
                tooltip: qsTr("Delete this draft from the server…")
                onClicked: deleteDraftConfirm.open()
            }
            AppButton {
                text: qsTr("Discard")
                onClicked: root.requestClose()
            }
            AppButton {
                text: qsTr("Save draft")
                onClicked: root.requestSaveDraft()
            }
            AppButton {
                text: qsTr("Send")
                intent: "primary"
                enabled: root.hasRecipients
                onClicked: root.requestSend()
            }
            Item {
                Layout.preferredWidth: 24
                Layout.preferredHeight: Theme.controlHeight

                Canvas {
                    anchors.centerIn: parent
                    width: 12
                    height: 12
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        ctx.strokeStyle = Theme.border
                        ctx.lineWidth = 1.5
                        for (var i = 0; i < 3; i++) {
                            ctx.beginPath()
                            ctx.moveTo(2 + i * 3, height - 1)
                            ctx.lineTo(width - 1, 2 + i * 3)
                            ctx.stroke()
                        }
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    preventStealing: true
                    cursorShape: Qt.SizeFDiagCursor
                    property real sMX
                    property real sMY
                    property real sX
                    property real sY
                    property real sW
                    property real sH
                    onPressed: function (mouse) {
                        var p = mapToItem(root.parent, mouse.x, mouse.y)
                        sMX = p.x
                        sMY = p.y
                        sX = root.x
                        sY = root.y
                        sW = root.width
                        sH = root.height
                        // Resize from this corner without restoring the
                        // centered fallback position.
                        root.wantX = sX
                        root.wantY = sY
                    }
                    onPositionChanged: function (mouse) {
                        if (!pressed)
                            return
                        var p = mapToItem(root.parent, mouse.x, mouse.y)
                        root.applyResize(Qt.RightEdge | Qt.BottomEdge,
                            sX, sY, sW, sH, p.x - sMX, p.y - sMY)
                    }
                }
            }
        }
    }

    // --- file picker ------------------------------------------------------
    FileDialog {
        id: attachDialog
        title: qsTr("Attach files")
        fileMode: FileDialog.OpenFiles
        onAccepted: root.addAttachments(selectedFiles)
    }

    // --- link insertion ---------------------------------------------------
    Dialog {
        id: linkDialog
        title: qsTr("Insert link")
        modal: true
        anchors.centerIn: parent
        // Never wider than the composer, which itself shrinks with the window.
        width: Math.min(420, root.width - 2 * Theme.lg)
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        onOpened: urlField.text = "https://"

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: linkDialog.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Insert")
                intent: "primary"
                onClicked: {
                    bodyEditor.exec("createLink", urlField.text.trim())
                    linkDialog.close()
                }
            }
        }

        FormField {
            id: urlField
            width: parent.width
            label: qsTr("Address")
            hint: qsTr("Select text first to turn it into a link.")
        }
    }

    // Flaw F5: never lose typed content without asking. This dialog only
    // ever abandons local edits — destroying a server draft is a separate
    // explicit action (the 🗑 button → deleteDraftConfirm below).
    Dialog {
        id: discardConfirm
        title: qsTr("Unsent changes")
        modal: true
        anchors.centerIn: parent
        // Never wider than the composer, which itself shrinks with the window.
        width: Math.min(440, root.width - 2 * Theme.lg)
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: discardConfirm.close()
            }
            AppButton {
                text: root.draftUid >= 0 ? qsTr("Discard changes") : qsTr("Discard")
                intent: "danger"
                onClicked: {
                    root.markClean()
                    discardConfirm.close()
                    root.close()
                }
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Save draft")
                intent: "primary"
                onClicked: {
                    // Stays open until the save job reports back (see
                    // onSaveDraftRequested): a failure keeps the text.
                    discardConfirm.close()
                    root.requestSaveDraft()
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: root.draftUid >= 0
                  ? qsTr("Discard your edits? The saved draft on the server is kept.")
                  : qsTr("This message has not been sent. Save it as a draft on the server?")
        }
    }

    // Destroying a server draft: permanent, so it says so out loud with
    // its own Cancel. Reached only via the 🗑 button, never via Discard.
    Dialog {
        id: deleteDraftConfirm
        title: qsTr("Delete draft?")
        modal: true
        anchors.centerIn: parent
        // Never wider than the composer, which itself shrinks with the window.
        width: Math.min(400, root.width - 2 * Theme.lg)
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: deleteDraftConfirm.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Delete draft")
                intent: "danger"
                onClicked: {
                    if (root.backend && root.backend.delete_draft) {
                        var uid = root.draftUid
                        root.markClean()
                        deleteDraftConfirm.close()
                        root.close()
                        var r = root.backend.delete_draft(uid)
                        if (r !== "")
                            root.statusMessage(r)
                    } else {
                        root.markClean()
                        deleteDraftConfirm.close()
                        root.close()
                    }
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: qsTr("This draft will be permanently deleted from the server. This cannot be undone.")
        }
    }
}
