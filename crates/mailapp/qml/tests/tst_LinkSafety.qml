import QtQuick
import QtTest

import Mailclient

// Headless unit tests for the LinkSafety singleton (pure link policy: no
// UI, no WebEngine). Run via `scripts/qml-check.sh` (offscreen
// qmltestrunner against a generated stub `Mailclient` module).
TestCase {
    name: "LinkSafety"

    function test_web_schemes_pass() {
        verify(LinkSafety.isWebScheme("https://example.com/x"));
        verify(LinkSafety.isWebScheme("http://example.com/"));
        verify(LinkSafety.isWebScheme("mailto:a@example.com"));
        verify(LinkSafety.isWebScheme("  HTTPS://example.com  "));
    }

    function test_evil_schemes_fail_closed() {
        verify(!LinkSafety.isWebScheme("javascript:alert(1)"));
        verify(!LinkSafety.isWebScheme("JaVaScRiPt:alert(1)"));
        verify(!LinkSafety.isWebScheme("java\tscript:alert(1)"));
        verify(!LinkSafety.isWebScheme("data:text/html,<p>x</p>"));
        verify(!LinkSafety.isWebScheme("file:///etc/passwd"));
        verify(!LinkSafety.isWebScheme("vbscript:msgbox(1)"));
        verify(!LinkSafety.isWebScheme("ftp://example.com/x"));
        verify(!LinkSafety.isWebScheme(""));
        verify(!LinkSafety.isWebScheme(null));
        verify(!LinkSafety.isWebScheme(undefined));
        verify(!LinkSafety.isWebScheme("#fragment"));
    }

    function test_action_normalizes_to_examine() {
        compare(LinkSafety.actionFor("browser"), "browser");
        compare(LinkSafety.actionFor("examine"), "examine");
        compare(LinkSafety.actionFor(""), "examine");
        compare(LinkSafety.actionFor("weird"), "examine");
        compare(LinkSafety.actionFor(null), "examine");
        compare(LinkSafety.actionFor(undefined), "examine");
    }

    function test_display_parsing() {
        var u = "https://user@example.com:443/a/b?x=1";
        compare(LinkSafety.schemeOf(u), "https");
        compare(LinkSafety.hostOf(u), "example.com");
        compare(LinkSafety.pathOf(u), "/a/b?x=1");
        compare(LinkSafety.schemeOf("mailto:a@example.com"), "mailto");
        compare(LinkSafety.hostOf("https://example.com"), "example.com");
        compare(LinkSafety.pathOf("https://example.com"), "—");
        compare(LinkSafety.schemeOf(""), "—");
        compare(LinkSafety.hostOf(""), "—");
        compare(LinkSafety.pathOf(""), "—");
        compare(LinkSafety.hostOf(null), "—");
    }
}
