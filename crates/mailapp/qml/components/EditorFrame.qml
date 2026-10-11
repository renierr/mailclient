import QtQuick
import QtQuick.Controls
import QtWebEngine

import Mailclient

// WYSIWYG body editor: the core's editor page
// (`mailcore::compose::editor::document`, via `backend.editor_document`)
// in WebEngine, driven through its `window.mc` API — the same document the
// native composer loads, so formatting, caret and quote behaviour match.
//
// Why not TextArea: QML's rich-text TextArea has no API to format a
// selection, so the toolbar could only insert literal "<b>" tags — no visual
// feedback, and the caret ended up in the wrong place (flaw F3). Worse, Qt's
// rich text expresses formatting as inline `style="font-weight:700"`, which
// the outgoing sanitizer strips, so bold never survived the send either.
// execCommand with styleWithCSS off emits real <b>/<i>/<u> tags, which do.
//
// Local content only: no remote loads, no navigation. JavaScript is on
// because the editor *is* JavaScript, and the document is ours, not mail.
Item {
    id: root

    // Bridge building the document (Composer.qml sets it); the page's
    // colours, size and placeholder come from it.
    property var backend

    // Live formatting state, polled from the document so the toolbar can
    // show what the caret is actually inside of.
    property bool boldActive: false
    property bool italicActive: false
    property bool underlineActive: false
    property bool listActive: false
    property bool quoteActive: false
    property bool ready: false
    // Loaded once (a reload would drop the user's edits); a theme change
    // mid-composer keeps the page it started with, like before.
    property bool loaded: false
    // Body set before the document was ready; applied on load.
    property string pendingHtml: ""

    signal contentChanged

    // The document around the empty editable body is mailcore's (CSP, base
    // CSS, `window.mc` script); only the scrollbar styling is Qt's
    // (`extra_css`), like the reader (MessageView.wrapDoc).
    function buildDocument() {
        if (!root.backend)
            return "";
        return root.backend.editor_document(JSON.stringify({
                                                               "paper": Theme.bg.toString(),
                                                               "ink": Theme.text.toString(),
                                                               "muted": Theme.textMuted.toString(),
                                                               "accent": Theme.accent.toString(),
                                                               "rule": Theme.border.toString(),
                                                               "font_px": Math.round(14 * Theme.uiScale),
                                                               "placeholder": qsTr("Write your message…"),
                                                               "extra_css": Theme.webScrollbarCss()
                                                           }));
    }

    function ensureLoaded() {
        if (root.loaded || !root.backend)
            return;
        var doc = root.buildDocument();
        if (doc === "")
            return;
        root.loaded = true;
        view.loadHtml(doc, "");
    }

    function exec(command, value) {
        if (!root.ready)
            return;
        // JSON.stringify, not string concatenation: an apostrophe in the
        // value would otherwise close the JS string literal.
        var arg = value === undefined ? "" : "," + JSON.stringify(value);
        view.runJavaScript("mc.exec(" + JSON.stringify(command) + arg + ");", function () {
            root.pollState();
            root.contentChanged();
        });
    }

    function quote() {
        if (!root.ready)
            return;
        view.runJavaScript("mc.quote();", function () {
            root.pollState();
            root.contentChanged();
        });
    }

    // A link at the selection, or an inserted one when the caret is bare.
    function insertLink(url) {
        if (!root.ready)
            return;
        view.runJavaScript("mc.link(" + JSON.stringify(url) + ");", function () {
            root.pollState();
            root.contentChanged();
        });
    }

    // Keep the selection across a picker or dialog that takes focus away.
    function saveSelection() {
        if (root.ready)
            view.runJavaScript("mc.save();");
    }

    // Put an image (a `data:` URL, see Bridge.image_data_url) at the caret.
    // The sender turns it into an inline part on Send / Save draft.
    function insertImage(dataUrl) {
        root.exec("insertImage", dataUrl);
    }

    // Read the body back. Async by nature, so the caller passes a callback
    // (Send collects the payload from it). `mc.html` drops the caret
    // placeholders and emptied formatting tags, so an unused toggle sends
    // nothing.
    function fetchHtml(callback) {
        if (!root.ready) {
            callback("");
            return;
        }
        view.runJavaScript("mc.html()", callback);
    }

    function setHtml(html) {
        if (!root.ready) {
            // Arrives before the document finished loading (openForReply on a
            // freshly created dialog); replay it once we are ready.
            root.pendingHtml = html === undefined ? "" : html;
            return;
        }
        // The caret starts in the text slot, the first empty top-level
        // paragraph (`<p><br></p>`, mailcore's `compose::answer`), so a
        // reply is typed above the quote — the core page places it.
        var body = JSON.stringify(html === undefined ? "" : html);
        view.runJavaScript("mc.set(" + body + ");");
    }

    function focusEditor() {
        if (root.ready)
            view.runJavaScript("mc.focus();");
    }

    function pollState() {
        if (!root.ready)
            return;
        view.runJavaScript("mc.state()", function (json) {
            if (!json)
                return;
            var s = FeedJson.parse(json, ({}));
            root.boldActive = s.b === true;
            root.italicActive = s.i === true;
            root.underlineActive = s.u === true;
            root.listActive = s.l === true;
            root.quoteActive = s.q === true;
        });
    }

    onBackendChanged: root.ensureLoaded()

    WebEngineView {
        id: view
        anchors.fill: parent
        backgroundColor: Theme.bg

        settings.javascriptEnabled: true
        settings.localContentCanAccessRemoteUrls: false
        settings.localContentCanAccessFileUrls: false
        settings.autoLoadImages: false
        settings.pluginsEnabled: false
        settings.javascriptCanOpenWindows: false
        settings.javascriptCanAccessClipboard: true

        onLoadingChanged: loadRequest => {
            if (loadRequest.status === WebEngineView.LoadSucceededStatus) {
                root.ready = true;
                if (root.pendingHtml !== "") {
                    root.setHtml(root.pendingHtml);
                    root.pendingHtml = "";
                }
                root.pollState();
            }
        }

        // Clicking a link in the draft must not navigate the editor away.
        // Same Qt 6 API note as the reader (MessageView): the type lives on
        // `WebEngineNavigationRequest` and the verdict is accept()/reject().
        // Only TypedNavigation (our own loadHtml) is accepted.
        onNavigationRequested: request => {
            if (request.navigationType === WebEngineNavigationRequest.TypedNavigation)
                request.accept();
            else
                request.reject();
        }

        Component.onCompleted: root.ensureLoaded()
    }

    // Caret and selection changes have no QML signal, so the toolbar state is
    // sampled. 200 ms is below noticing and costs nothing while idle.
    Timer {
        interval: 200
        running: root.ready && root.visible
        repeat: true
        onTriggered: root.pollState()
    }
}
