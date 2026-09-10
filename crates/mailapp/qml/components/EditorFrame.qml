import QtQuick
import QtQuick.Controls
import QtWebEngine

import Mailclient

// WYSIWYG body editor: a `contentEditable` document in WebEngine, driven by
// `document.execCommand`.
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

    // Live formatting state, polled from the document so the toolbar can
    // show what the caret is actually inside of.
    property bool boldActive: false
    property bool italicActive: false
    property bool underlineActive: false
    property bool listActive: false
    property bool quoteActive: false
    property bool ready: false
    // Body set before the document was ready; applied on load.
    property string pendingHtml: ""

    signal contentChanged()

    readonly property string documentHtml:
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>"
        + "html,body{margin:0;padding:0;height:100%}"
        + "#e{box-sizing:border-box;min-height:100%;padding:12px 14px;outline:none;"
        + "font-family:sans-serif;font-size:14px;line-height:1.55;"
        + "color:" + Theme.text + ";background:" + Theme.bg + ";caret-color:" + Theme.accent + "}"
        + "#e:empty:before{content:attr(data-placeholder);color:" + Theme.textMuted + "}"
        + "blockquote{margin:8px 0;padding-left:12px;border-left:3px solid " + Theme.border
        + ";color:" + Theme.textMuted + "}"
        + "a{color:" + Theme.accent + "}"
        + "</style></head><body><div id=\"e\" contenteditable=\"true\" "
        + "data-placeholder=\"" + qsTr("Write your message…") + "\"></div>"
        + "<script>"
        + "document.execCommand('defaultParagraphSeparator', false, 'p');"
        + "document.execCommand('styleWithCSS', false, false);"
        + "var e = document.getElementById('e');"
        + "e.focus();"
        + "</script></body></html>"

    function exec(command, value) {
        if (!root.ready)
            return
        // JSON.stringify, not string concatenation: an apostrophe in the
        // value would otherwise close the JS string literal.
        var arg = value === undefined ? "null" : JSON.stringify(value)
        view.runJavaScript(
            "document.getElementById('e').focus();"
            + "document.execCommand(" + JSON.stringify(command) + ", false, " + arg + ");",
            function () {
                root.pollState()
                root.contentChanged()
            })
    }

    // Read the body back. Async by nature, so the caller passes a callback
    // (Send collects the payload from it).
    function fetchHtml(callback) {
        if (!root.ready) {
            callback("")
            return
        }
        view.runJavaScript("document.getElementById('e').innerHTML", callback)
    }

    function setHtml(html) {
        if (!root.ready) {
            // Arrives before the document finished loading (openForReply on a
            // freshly created dialog); replay it once we are ready.
            root.pendingHtml = html === undefined ? "" : html
            return
        }
        view.runJavaScript("document.getElementById('e').innerHTML = "
                           + JSON.stringify(html === undefined ? "" : html) + ";")
    }

    function focusEditor() {
        if (root.ready)
            view.runJavaScript("document.getElementById('e').focus();")
    }

    function pollState() {
        if (!root.ready)
            return
        view.runJavaScript(
            "JSON.stringify({"
            + "b: document.queryCommandState('bold'),"
            + "i: document.queryCommandState('italic'),"
            + "u: document.queryCommandState('underline'),"
            + "l: document.queryCommandState('insertUnorderedList'),"
            + "q: !!document.queryCommandValue('formatBlock').match(/blockquote/i)"
            + "})", function (json) {
                if (!json)
                    return
                var s = JSON.parse(json)
                root.boldActive = s.b === true
                root.italicActive = s.i === true
                root.underlineActive = s.u === true
                root.listActive = s.l === true
                root.quoteActive = s.q === true
            })
    }

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
                root.ready = true
                if (root.pendingHtml !== "") {
                    root.setHtml(root.pendingHtml)
                    root.pendingHtml = ""
                }
                root.pollState()
            }
        }

        // Clicking a link in the draft must not navigate the editor away;
        // everything else (including this view's own initial loadHtml) is
        // allowed, or the document would never appear.
        onNavigationRequested: request => {
            if (request.navigationType === WebEngineView.NavigationTypeLinkClicked
                    || request.navigationType === WebEngineView.NavigationTypeFormSubmitted)
                request.action = WebEngineView.IgnoreRequest
        }

        Component.onCompleted: view.loadHtml(root.documentHtml, "")
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
