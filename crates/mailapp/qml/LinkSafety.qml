pragma Singleton

import QtQuick

// Pure link-safety helpers for the reader: which URLs may ever leave the
// app, what a click does with one, and how the examine dialog displays it.
//
// No imports beyond QtQuick and no UI here, so this stays headless-testable
// (`tst_LinkSafety.qml` via `scripts/qml-check.sh`). QML's JS engine has no
// WHATWG `URL` constructor, hence the hand parsing. The allow-list shape
// mirrors `normalize_link_click` / `safe_href` in mailcore: unknown input
// fails closed (inert text, examine-first).
QtObject {
    // Only these schemes ever leave the app (user-gated). Everything else
    // (javascript:, data:, file:, …) is inert.
    function isWebScheme(url) {
        var s = (url || "").trim().toLowerCase();
        return s.indexOf("http://") === 0 || s.indexOf("https://") === 0 || s.indexOf("mailto:") === 0;
    }

    // Normalized click action: anything but "browser" examines first.
    function actionFor(linkClickAction) {
        return linkClickAction === "browser" ? "browser" : "examine";
    }

    function schemeOf(u) {
        var s = (u || "").trim();
        var scheme = s.indexOf("://");
        if (scheme > 0)
            return s.substring(0, scheme).toLowerCase();
        if (s.indexOf("mailto:") === 0)
            return "mailto";
        return "—";
    }

    function hostOf(u) {
        var s = (u || "").trim();
        var scheme = s.indexOf("://");
        var rest = scheme >= 0 ? s.substring(scheme + 3) : s;
        var end = rest.indexOf("/");
        var host = end >= 0 ? rest.substring(0, end) : rest;
        var at = host.lastIndexOf("@");
        if (at >= 0)
            host = host.substring(at + 1);
        var colon = host.indexOf(":");
        if (colon >= 0)
            host = host.substring(0, colon);
        return host === "" ? "—" : host;
    }

    function pathOf(u) {
        var s = (u || "").trim();
        var scheme = s.indexOf("://");
        var rest = scheme >= 0 ? s.substring(scheme + 3) : s;
        var slash = rest.indexOf("/");
        if (slash < 0)
            return "—";
        var p = rest.substring(slash);
        return p === "" ? "—" : p;
    }
}
