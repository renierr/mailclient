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
    // Reader text size (`reader_font_size` via Composer): scales the body
    // like the reader, through the core's factor.
    property string readerFont: "normal"

    // Live formatting state, pushed by the document (its title, see
    // `onTitleChanged`) so the toolbar shows what the caret is inside of.
    property bool boldActive: false
    property bool italicActive: false
    property bool underlineActive: false
    property bool listActive: false
    property bool quoteActive: false
    property bool ready: false
    // The options the loaded page was built with. Settings (`appSettings`)
    // load after the QML tree is created, so the first build still sees
    // their defaults; the page is rebuilt with the live values on the next
    // setHtml (every composer open seeds the body through it), never under
    // typed text.
    property string docOptions: ""
    // Body set before the document was ready; applied on load.
    property string pendingHtml: ""

    signal contentChanged

    // The document around the empty editable body is mailcore's (CSP, base
    // CSS, `window.mc` script); only the scrollbar styling is Qt's
    // (`extra_css`), like the reader (MessageView.wrapDoc). The body size
    // is the reader's (14px times interface and reader-text scale), so the
    // composer reads like the mail it answers.
    function currentOptions() {
        if (!root.backend)
            return "";
        var textScale = root.backend.reader_text_scale(root.readerFont);
        if (!(textScale > 0))
            textScale = 1;
        return JSON.stringify({
                                  "paper": Theme.bg.toString(),
                                  "ink": Theme.text.toString(),
                                  "muted": Theme.textMuted.toString(),
                                  "accent": Theme.accent.toString(),
                                  "rule": Theme.border.toString(),
                                  "font_px": Math.round(14 * Theme.uiScale * textScale),
                                  "placeholder": qsTr("Write your message…"),
                                  "extra_css": Theme.webScrollbarCss()
                              });
    }

    function ensureLoaded() {
        root.ensureDocument();
    }

    // (Re)build the page when the options moved. Every setHtml caller
    // passes the full desired body, so replaying it after the reload loses
    // nothing; anything typed meanwhile stays queued in pendingHtml.
    function ensureDocument() {
        var opts = root.currentOptions();
        if (opts === "" || opts === root.docOptions)
            return;
        var doc = root.backend.editor_document(opts);
        if (doc === "")
            return;
        root.docOptions = opts;
        root.ready = false;
        view.loadHtml(doc, "");
    }

    function exec(command, value) {
        if (!root.ready)
            return;
        // JSON.stringify, not string concatenation: an apostrophe in the
        // value would otherwise close the JS string literal.
        var arg = value === undefined ? "" : "," + JSON.stringify(value);
        view.runJavaScript("mc.exec(" + JSON.stringify(command) + arg + ");", function () {
            root.refreshState();
            root.contentChanged();
        });
    }

    function quote() {
        if (!root.ready)
            return;
        view.runJavaScript("mc.quote();", function () {
            root.refreshState();
            root.contentChanged();
        });
    }

    // A link at the selection, or an inserted one when the caret is bare.
    function insertLink(url) {
        if (!root.ready)
            return;
        view.runJavaScript("mc.link(" + JSON.stringify(url) + ");", function () {
            root.refreshState();
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
        // Settings arrive after the first build; a composer opened later
        // must not keep the defaults' size or theme. Rebuilding here is
        // safe: the caller passes the full body, replayed after the reload.
        root.ensureDocument();
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

    function applyState(json) {
        if (!json)
            return;
        var s = FeedJson.parse(json, ({}));
        root.boldActive = s.b === true;
        root.italicActive = s.i === true;
        root.underlineActive = s.u === true;
        root.listActive = s.l === true;
        root.quoteActive = s.q === true;
    }

    // Asked once where the page may not have pushed yet (a fresh load, right
    // after a toolbar command); every caret move and edit pushes by itself.
    function refreshState() {
        if (root.ready)
            view.runJavaScript("mc.state()", root.applyState);
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

        // The page pushes its formatting state as the title (`mc:<json>`,
        // see `mailcore::compose::editor`): caret and selection changes have
        // no QML signal, and WebEngine offers no call into QML without a
        // web channel.
        onTitleChanged: {
            if (view.title.indexOf("mc:") === 0)
                root.applyState(view.title.slice(3));
        }

        onLoadingChanged: loadRequest => {
            if (loadRequest.status === WebEngineView.LoadSucceededStatus) {
                root.ready = true;
                if (root.pendingHtml !== "") {
                    root.setHtml(root.pendingHtml);
                    root.pendingHtml = "";
                }
                root.refreshState();
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
}
