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
    x: positioned ? Math.round(Math.max(0, Math.min(wantX, hostW - width))) : Math.round((hostW - width) / 2)
    y: positioned ? Math.round(Math.max(0, Math.min(wantY, hostH - height))) : Math.round((hostH - height) / 2)

    function applyDefaultGeometry() {
        wantW = preferredW;
        wantH = preferredH;
        positioned = false;
    }

    function applyResize(edges, sX, sY, sW, sH, dx, dy) {
        var nx = sX;
        var ny = sY;
        var nw = sW;
        var nh = sH;
        var minW = Math.min(minWidth, maxW);
        var minH = Math.min(minHeight, maxH);
        if (edges & Qt.RightEdge)
            nw = Math.min(Math.max(minW, sW + dx), hostW - nx);
        if (edges & Qt.LeftEdge) {
            nw = Math.min(Math.max(minW, sW - dx), sX + sW);
            nx = sX + sW - nw;
        }
        if (edges & Qt.BottomEdge)
            nh = Math.min(Math.max(minH, sH + dy), hostH - ny);
        if (edges & Qt.TopEdge) {
            nh = Math.min(Math.max(minH, sH - dy), sY + sH);
            ny = sY + sH - nh;
        }
        positioned = true;
        wantX = nx;
        wantY = ny;
        wantW = nw;
        wantH = nh;
    }

    onOpened: applyDefaultGeometry()

    signal statusMessage(string text)
    signal sendRequested(string payload)
    signal saveDraftRequested(string payload)

    property string accountEmail: ""
    // The account sending: receipt defaults depend on its SMTP server.
    property int accountId: -1
    property string accountFromName: ""
    property string sendFormat: "auto"
    property var backend
    property bool collectContacts: true
    property bool sourceMode: false
    // What the body goes out as right now (`send_format_note`), like the
    // native composer's line: Auto names plain or HTML by the content.
    property string formatNote: ""
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
    // mailcore's sentence for it (`AnswerDraft.notice`).
    property string replyNotice: ""
    // A forward's files that could not be downloaded (mailcore's sentence).
    property string filesNotice: ""

    // Outgoing files picked via FileDialog: [{path, name}]. Paths (plain or
    // `file://` URLs) travel in the send payload; Rust reads the bytes at
    // send time, so no binary crosses the QML bridge.
    property var attachments: []

    // Receipts this mail asks for (compose::Receipts): each composition
    // starts from the settings, the toggles beside Send change just this one.
    property bool requestMdn: false
    property bool requestDsn: false
    // mailcore's warning while delivery is on but the server lacked DSN.
    property string deliveryNote: ""

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
    // The split and the join are mailcore's (`compose::sender_parts` /
    // `effective_from`), so the address shown is the address sent.
    readonly property var accountParts: root.senderParts(root.accountEmail)
    readonly property string accountDomain: root.accountParts.domain || ""
    readonly property string accountLocalPart: root.accountParts.local || ""
    readonly property string effectiveFrom: root.backend ? root.backend.effective_from(fromLocal.text,
                                                                                       root.accountEmail) :
                                                           root.accountEmail

    function senderParts(address) {
        return root.backend ? FeedJson.parse(root.backend.sender_parts_json(address), ({})) : ({});
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
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            anchors.rightMargin: Theme.lg
            text: root.formatNote
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
                var p = mapToItem(root.parent, mouse.x, mouse.y);
                sMX = p.x;
                sMY = p.y;
                sX = root.x;
                sY = root.y;
                root.positioned = true;
            }
            onPositionChanged: function (mouse) {
                if (!pressed)
                    return;
                var p = mapToItem(root.parent, mouse.x, mouse.y);
                root.wantX = sX + p.x - sMX;
                root.wantY = sY + p.y - sMY;
            }
        }
    }

    function markClean() {
        root.dirty = false;
    }

    // Every open* resets the one shared set of fields; refuse while an
    // earlier composition still needs them (see sendPending / saving).
    function readyForNew() {
        if (root.sendPending) {
            root.statusMessage(qsTr("Still sending the previous message — try again in a moment"));
            return false;
        }
        if (root.saving) {
            root.statusMessage(qsTr("Still saving the draft — try again in a moment"));
            return false;
        }
        return true;
    }

    function requestClose() {
        if (root.dirty)
            discardConfirm.open();
        else
            root.close();
    }

    function resetHeaders() {
        fromLocal.text = root.accountLocalPart;
        fromName.text = root.accountFromName;
        toField.text = "";
        ccField.text = "";
        bccField.text = "";
        replyToField.text = "";
        subjectField.text = "";
        root.showCc = false;
        root.showBcc = false;
        root.showReplyTo = false;
        root.replyNoticeAddr = "";
        root.replyNotice = "";
        root.filesNotice = "";
        root.attachments = [];
        root.draftUid = -1;
        var receipts = root.backend ? FeedJson.parse(root.backend.receipt_defaults_json(root.accountId), ({})) : ({});
        root.requestMdn = receipts.read === true;
        root.requestDsn = receipts.delivery === true;
        root.deliveryNote = receipts.delivery_note || "";
    }

    function setBody(html) {
        bodyEditor.setHtml(html);
        sourceArea.text = html;
        formatNoteTimer.restart();
    }

    function refreshFormatNote() {
        if (!root.backend)
            return;
        // The source view, or an editor still loading, holds the body as text.
        if (root.sourceMode || !bodyEditor.ready) {
            root.formatNote = root.backend.send_format_note(root.sendFormat, sourceArea.text);
            return;
        }
        bodyEditor.fetchHtml(html => root.formatNote = root.backend.send_format_note(root.sendFormat, html || ""));
    }

    Timer {
        id: formatNoteTimer
        interval: 300
        onTriggered: root.refreshFormatNote()
    }

    onSendFormatChanged: formatNoteTimer.restart()
    onSourceModeChanged: formatNoteTimer.restart()

    // New mail and answers come prepared by mailcore (compose::answer):
    // recipients, subject, quote and signature in their placement. The
    // composer only fills its fields.
    function openBlank() {
        if (!root.readyForNew())
            return;
        root.sourceMode = false;
        root.resetHeaders();
        var draft = root.backend ? FeedJson.parse(root.backend.blank_draft_json(), ({})) : ({});
        root.setBody(draft.body_html || "");
        root.markClean();
        open();
    }

    // `mode` is "reply", "reply_all" or "forward"; `uid` is in the current
    // folder.
    function openForAnswer(uid, mode) {
        if (!root.readyForNew())
            return;
        root.openAnswerDraft(root.backend && uid >= 0 ? FeedJson.parse(root.backend.answer_draft_json(uid, mode), ({})) : ({}));
    }

    // A prepared answer draft; a forward's also carries the original's files
    // (`attachments`) and `files_notice` for any left out.
    function openAnswerDraft(draft) {
        if (!root.readyForNew())
            return;
        if (draft.body_html === undefined) {
            root.statusMessage(qsTr("This message is no longer available"));
            return;
        }
        root.sourceMode = false;
        root.resetHeaders();
        toField.text = draft.to || "";
        ccField.text = draft.cc || "";
        root.showCc = ccField.text !== "";
        subjectField.text = draft.subject || "";
        // Reply-To elsewhere than the sender: the banner says so out loud.
        root.replyNoticeAddr = draft.notice_addr || "";
        root.replyNotice = draft.notice || "";
        root.attachments = draft.attachments || [];
        root.filesNotice = draft.files_notice || "";
        root.setBody(draft.body_html);
        root.markClean();
        open();
    }

    function openForDraft(draft) {
        if (!root.readyForNew())
            return;
        root.sourceMode = false;
        root.resetHeaders();
        root.draftUid = draft.draft_uid === undefined ? -1 : draft.draft_uid;
        fromLocal.text = root.senderParts(draft.from || root.accountEmail).local || "";
        toField.text = draft.to || "";
        ccField.text = draft.cc || "";
        bccField.text = draft.bcc || "";
        replyToField.text = draft.reply_to || "";
        root.showCc = ccField.text !== "";
        root.showBcc = bccField.text !== "";
        root.showReplyTo = replyToField.text !== "";
        subjectField.text = draft.subject || "";
        root.attachments = draft.attachments || [];
        root.setBody(draft.body || "");
        root.markClean();
        open();
    }

    // Source mode shows exactly what will be sent, and edits round-trip.
    function toggleSource() {
        if (!root.sourceMode) {
            bodyEditor.fetchHtml(function (html) {
                sourceArea.text = html;
                root.sourceMode = true;
            });
        } else {
            bodyEditor.setHtml(sourceArea.text);
            root.sourceMode = false;
        }
    }

    function baseName(url) {
        var s = url.toString();
        var i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"));
        var name = i < 0 ? s : s.substring(i + 1);
        try {
            name = decodeURIComponent(name);
        } catch (e) {}
        return name === "" ? s : name;
    }

    function addAttachments(urls) {
        var next = root.attachments.slice();
        for (var i = 0; i < urls.length; i++) {
            var u = urls[i].toString();
            var known = false;
            for (var j = 0; j < next.length; j++) {
                if (next[j].path === u) {
                    known = true;
                    break;
                }
            }
            if (!known)
                next.push({
                              path: u,
                              name: root.baseName(u)
                          });
        }
        root.attachments = next;
        root.dirty = true;
    }

    // Images shown inside the text: each file becomes a `data:` URL the
    // editor can display; the sender turns them into inline parts. A file
    // that cannot go inline (type, size) is reported and not inserted.
    function insertImages(urls) {
        if (root.sourceMode) {
            root.addAttachments(urls);
            return;
        }
        for (var i = 0; i < urls.length; i++) {
            var r = backend.image_data_url(urls[i].toString());
            if (r.indexOf("data:") === 0)
                bodyEditor.insertImage(r);
            else
                root.statusMessage(r);
        }
    }

    // Dropped files: images ask inline-or-attach, everything else attaches.
    function filesDropped(images, others) {
        if (others.length > 0)
            root.addAttachments(others);
        if (images.length > 0) {
            if (root.sourceMode)
                root.addAttachments(images);
            else
                imagePlacement.ask(images);
        }
    }

    function removeAttachment(index) {
        var next = root.attachments.slice();
        next.splice(index, 1);
        root.attachments = next;
        root.dirty = true;
    }

    function payloadFor(html) {
        var paths = [];
        for (var i = 0; i < root.attachments.length; i++)
            paths.push(root.attachments[i].path);
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
                                  draft_uid: root.draftUid,
                                  request_mdn: root.requestMdn,
                                  request_dsn: root.requestDsn
                              });
    }

    // To accepts placeholder text or stays blank: the real recipients may
    // live in Cc/Bcc alone. Only all-three-empty blocks the send.
    readonly property bool hasRecipients: toField.text.trim() !== "" || ccField.text.trim() !== "" || bccField.text.trim()
                                          !== ""

    // Reading the document back is asynchronous, so Send finishes inside the
    // callback rather than returning a payload.
    function requestSend() {
        if (!root.hasRecipients) {
            root.statusMessage(qsTr("Add at least one recipient (To, Cc or Bcc)"));
            return;
        }
        if (root.sourceMode) {
            root.sendRequested(root.payloadFor(sourceArea.text));
        } else {
            bodyEditor.fetchHtml(function (html) {
                root.sendRequested(root.payloadFor(html));
            });
        }
    }

    function requestSaveDraft() {
        if (root.sourceMode) {
            root.saveDraftRequested(root.payloadFor(sourceArea.text));
        } else {
            bodyEditor.fetchHtml(function (html) {
                root.saveDraftRequested(root.payloadFor(html));
            });
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
                            horizontalAlignment: TextInput.AlignRight
                            // The domain is the account's: an `@` typed here
                            // would show an address that is not the one sent.
                            validator: RegularExpressionValidator {
                                regularExpression: /[^@]*/
                            }
                            onTextChanged: root.dirty = true
                        }
                        Label {
                            text: root.accountDomain
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontBase
                            ToolTip.visible: domainHover.hovered
                            ToolTip.text: qsTr("Fixed to this account's domain")
                            HoverHandler {
                                id: domainHover
                            }
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
            visible: root.replyNoticeAddr !== "" && toField.text.trim().toLowerCase()
                     === root.replyNoticeAddr.toLowerCase()
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
                    text: root.replyNotice
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.requestDsn && root.deliveryNote !== ""
            wrapMode: Text.Wrap
            color: Theme.warning
            font.pixelSize: Theme.fontSmall
            text: root.deliveryNote
        }

        // --- attachments ----------------------------------------------------
        Label {
            Layout.fillWidth: true
            visible: root.filesNotice !== ""
            wrapMode: Text.Wrap
            color: Theme.warning
            font.pixelSize: Theme.fontSmall
            text: root.filesNotice
        }
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
            onImageRequested: imageDialog.open()
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
                onContentChanged: {
                    root.dirty = true;
                    formatNoteTimer.restart();
                }
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
                    onTextChanged: {
                        root.dirty = true;
                        formatNoteTimer.restart();
                    }
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
            Item {
                Layout.fillWidth: true
            }
            IconButton {
                text: Icons.markRead
                iconFont: true
                active: root.requestMdn
                tooltip: root.requestMdn ? qsTr("Read receipt requested — the recipient's mail app may confirm opening")
                                         : qsTr("Request a read receipt")
                onClicked: root.requestMdn = !root.requestMdn
            }
            IconButton {
                text: Icons.doneAll
                iconFont: true
                active: root.requestDsn
                tooltip: root.requestDsn ? qsTr("Delivery confirmation requested — the receiving server reports delivery")
                                         : qsTr("Request a delivery confirmation")
                onClicked: root.requestDsn = !root.requestDsn
            }
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
                        var ctx = getContext("2d");
                        ctx.clearRect(0, 0, width, height);
                        ctx.strokeStyle = Theme.border;
                        ctx.lineWidth = 1.5;
                        for (var i = 0; i < 3; i++) {
                            ctx.beginPath();
                            ctx.moveTo(2 + i * 3, height - 1);
                            ctx.lineTo(width - 1, 2 + i * 3);
                            ctx.stroke();
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
                        var p = mapToItem(root.parent, mouse.x, mouse.y);
                        sMX = p.x;
                        sMY = p.y;
                        sX = root.x;
                        sY = root.y;
                        sW = root.width;
                        sH = root.height;
                        // Resize from this corner without restoring the
                        // centered fallback position.
                        root.wantX = sX;
                        root.wantY = sY;
                    }
                    onPositionChanged: function (mouse) {
                        if (!pressed)
                            return;
                        var p = mapToItem(root.parent, mouse.x, mouse.y);
                        root.applyResize(Qt.RightEdge | Qt.BottomEdge, sX, sY, sW, sH, p.x - sMX, p.y - sMY);
                    }
                }
            }
        }
    }

    // Drag & drop of files from the desktop over the whole composer.
    ComposerDropZone {
        anchors.fill: parent
        z: 10
        isInlineImage: url => backend.is_inline_image(url)
        onFilesDropped: (images, others) => root.filesDropped(images, others)
    }

    ImagePlacementDialog {
        id: imagePlacement
        onInlineChosen: urls => root.insertImages(urls)
        onAttachChosen: urls => root.addAttachments(urls)
    }

    // --- file picker ------------------------------------------------------
    FileDialog {
        id: attachDialog
        title: qsTr("Attach files")
        fileMode: FileDialog.OpenFiles
        onAccepted: root.addAttachments(selectedFiles)
    }

    FileDialog {
        id: imageDialog
        title: qsTr("Insert image")
        fileMode: FileDialog.OpenFiles
        nameFilters: [qsTr("Images (*.png *.jpg *.jpeg *.gif *.webp)")]
        onAccepted: root.insertImages(selectedFiles)
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
            Item {
                Layout.fillWidth: true
            }
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
                    bodyEditor.exec("createLink", urlField.text.trim());
                    linkDialog.close();
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
            Item {
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("Cancel")
                onClicked: discardConfirm.close()
            }
            AppButton {
                text: root.draftUid >= 0 ? qsTr("Discard changes") : qsTr("Discard")
                intent: "danger"
                onClicked: {
                    root.markClean();
                    discardConfirm.close();
                    root.close();
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
                    discardConfirm.close();
                    root.requestSaveDraft();
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: root.draftUid >= 0 ? qsTr("Discard your edits? The saved draft on the server is kept.") : qsTr(
                                           "This message has not been sent. Save it as a draft on the server?")
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
            Item {
                Layout.fillWidth: true
            }
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
                        var uid = root.draftUid;
                        root.markClean();
                        deleteDraftConfirm.close();
                        root.close();
                        var r = root.backend.delete_draft(uid);
                        if (r !== "")
                            root.statusMessage(r);
                    } else {
                        root.markClean();
                        deleteDraftConfirm.close();
                        root.close();
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
