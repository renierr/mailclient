//! Tests for the HTML sanitizer and its text conversions.

use super::entities::*;
use super::sanitize::*;
use super::text::*;

use super::*;

#[test]
fn strips_script_and_handlers() {
    let s = sanitize("<p onclick=\"x()\">hi<script>alert(1)</script></p>", false);
    assert!(!s.html.contains("script"));
    assert!(!s.html.contains("onclick"));
    assert!(s.html.contains("hi"));
}

#[test]
fn quoted_gt_does_not_end_tag() {
    let s = sanitize("<img alt=\"a>b\" src=\"cid:x\">", false);
    assert!(s.html.contains("cid:x"));
    assert!(!s.html.contains("a&gt;b\">"));
}

#[test]
fn blocks_remote_img_by_default() {
    let s = sanitize("<img src=\"https://example.com/t.png\" alt=\"t\">", false);
    assert!(s.had_remote);
    assert!(!s.html.contains("example.com"));
    let s2 = sanitize("<img src=\"https://example.com/t.png\">", true);
    assert!(s2.html.contains("example.com"));
}

#[test]
fn blocks_private_hosts_even_when_allowed() {
    let s = sanitize("<img src=\"http://192.168.1.2/x.png\">", true);
    assert!(!s.html.contains("192.168"));
}

#[test]
fn rejects_javascript_href() {
    let s = sanitize("<a href=\"javascript:alert(1)\">x</a>", true);
    assert!(!s.html.contains("javascript"));
    assert!(s.html.contains("x"));
    let ok = sanitize("<a href=\"https://example.com\">x</a>", true);
    assert!(ok.html.contains("https://example.com"));
}

#[test]
fn presentational_styles_survive() {
    let s = sanitize(
        "<table width=\"100%\" bgcolor=\"#f4f4f4\" cellpadding=\"8\" align=\"center\">         <tr><td valign=\"top\" style=\"color:#333;font-size:16px;padding:0 12px\">         <font color=\"red\" face=\"Arial, sans-serif\">hi</font></td></tr></table>",
        false,
    );
    for kept in [
        "width=\"100%\"",
        "bgcolor=\"#f4f4f4\"",
        "cellpadding=\"8\"",
        "align=\"center\"",
        "valign=\"top\"",
        "style=\"color:#333;font-size:16px;padding:0 12px;\"",
        "<font color=\"red\" face=\"Arial, sans-serif\">",
    ] {
        assert!(s.html.contains(kept), "{kept} missing from {}", s.html);
    }
}

#[test]
fn styles_that_fetch_or_escape_are_dropped() {
    for style in [
        "background:url(https://example.com/x)",
        "background-image:url('https://example.com/x')",
        r"color:red;background:u\72l(https://example.com/x)",
        "width:expression(alert(1))",
        "behavior:url(x.htc)",
        "color:red/*x*/;background:url(https://example.com/x)",
        "position:fixed;top:0;left:0",
        "content:'x'",
        "background:&#117;rl(https://example.com/x)",
    ] {
        let s = sanitize(&format!("<p style=\"{style}\">t</p>"), true);
        assert!(!s.html.contains("example.com"), "{style} -> {}", s.html);
        assert!(!s.html.contains("expression"), "{style} -> {}", s.html);
        assert!(!s.html.contains("position"), "{style} -> {}", s.html);
        assert!(!s.html.contains("content"), "{style} -> {}", s.html);
        assert!(s.html.contains('t'));
    }
    // The safe declaration next to a dropped one is kept.
    let s = sanitize("<p style=\"position:absolute;color:red\">t</p>", false);
    assert!(s.html.contains("style=\"color:red;\""), "{}", s.html);
}

#[test]
fn layout_attributes_reject_junk() {
    let s = sanitize(
        "<td bgcolor=\"red;background:url(x)\" align=\"evil\" width=\"99999\">t</td>         <p bgcolor=\"red\">u</p><table border=\"500\"></table>",
        false,
    );
    assert!(!s.html.contains("bgcolor=\"red;"));
    assert!(!s.html.contains("evil"));
    assert!(s.html.contains("width=\"1200\""));
    assert!(!s.html.contains("<p bgcolor"));
    assert!(s.html.contains("border=\"40\""));
}

#[test]
fn own_colours_are_detected_after_sanitizing() {
    let colored = [
        "<p style=\"color:#333\">t</p>",
        "<td style=\"background-color:#fff\">t</td>",
        "<table bgcolor=\"#eee\"><tr><td>t</td></tr></table>",
        "<font color=\"red\">t</font>",
        "<div style=\"background:#123456\">t</div>",
    ];
    for h in colored {
        assert!(has_own_colors(&sanitize(h, false).html), "{h}");
    }
    let plain = [
        "<p>hi <b>there</b></p>",
        "<p style=\"font-size:14px;margin:0\">t</p>",
        // Dropped by the sanitizer, so it never paints.
        "<p style=\"background:url(https://example.com/x)\">t</p>",
        "<p>the word color: in text</p>",
    ];
    for h in plain {
        assert!(!has_own_colors(&sanitize(h, false).html), "{h}");
    }
}

#[test]
fn preheader_hiding_is_kept() {
    let s = sanitize("<div style=\"display:none;max-height:0\">pre</div>", false);
    assert!(s.html.contains("display:none;"));
    let s = sanitize("<div style=\"display:-webkit-box\">x</div>", false);
    assert!(!s.html.contains("display"));
}

#[test]
fn style_attr_dropped() {
    let s = sanitize(
        "<p style=\"background:url(https://example.com/x)\">t</p>",
        true,
    );
    assert!(!s.html.contains("example.com"));
    assert!(s.html.contains("t"));
}

#[test]
fn no_auto_fetch_vectors_survive_sanitizing() {
    // Everything a renderer would fetch WITHOUT a click must vanish (marker
    // host). Clickable `href`s intentionally survive (user-gated navigation,
    // never a fetch) and are covered separately below.
    let s = sanitize(
        "<head><meta http-equiv=\"refresh\" content=\"0;url=https://evil.example.net/\">\
         <link rel=\"preload\" href=\"https://evil.example.net/x.css\" as=\"style\">\
         <link rel=\"stylesheet\" href=\"https://evil.example.net/x.css\">\
         <base href=\"https://evil.example.net/\">\
         <style>@import url(https://evil.example.net/x.css); \
         p { background-image: url(https://evil.example.net/x.png); }</style></head>\
         <p>hello</p>\
         <img srcset=\"https://evil.example.net/x.png 1x\" src=\"cid:k\" alt=\"t\">\
         <table background=\"https://evil.example.net/x.png\"><tr><td>v</td></tr></table>\
         <svg><image href=\"https://evil.example.net/x.png\"/></svg>\
         <video poster=\"https://evil.example.net/x.png\" src=\"https://evil.example.net/x.mp4\"></video>\
         <audio src=\"https://evil.example.net/x.mp3\"></audio>\
         <iframe src=\"https://evil.example.net/\"></iframe>\
         <object data=\"https://evil.example.net/x.swf\"></object>\
         <embed src=\"https://evil.example.net/x.swf\">\
         <form action=\"https://evil.example.net/s\"><input name=\"q\"></form>",
        false,
    );
    assert!(
        !s.html.contains("evil.example.net"),
        "network leak: {}",
        s.html
    );
    for kept in ["hello", "cid:k", "<table>", "v</td>"] {
        assert!(s.html.contains(kept), "lost legit content: {kept}");
    }
}

#[test]
fn link_vectors_keep_link_drop_beacon() {
    // `ping` (hyperlink auditing beacon) is stripped; the link itself stays
    // clickable. `javascript:`/`data:` hrefs lose the URL, keep the text.
    let s = sanitize(
        "<a href=\"https://example.com/\" ping=\"https://evil.example.net/p\">x</a>",
        false,
    );
    assert!(!s.html.contains("ping="), "beacon kept: {}", s.html);
    assert!(
        !s.html.contains("evil.example.net"),
        "beacon kept: {}",
        s.html
    );
    assert!(
        s.html.contains("<a href=\"https://example.com/\""),
        "link lost: {}",
        s.html
    );
    let js = sanitize("<a href=\"javascript:alert(1)\">y</a>", false);
    assert!(!js.html.contains("javascript"));
    assert!(js.html.contains("y</a>"));
    let data = sanitize("<a href=\"data:text/html,<p>z</p>\">w</a>", false);
    assert!(!data.html.contains("data:text"));
    assert!(data.html.contains("w</a>"));
}

#[test]
fn html_to_text_keeps_lines() {
    assert_eq!(html_to_text("<p>hi<br>there</p>"), "hi\nthere");
}

#[test]
fn looks_like_html_ignores_stray() {
    assert!(!looks_like_html("I <3 you 5 > 3"));
    assert!(looks_like_html("<p>hi</p>"));
}

#[test]
fn nbsp_survives_send_pipeline() {
    // The WYSIWYG editor emits `&nbsp;`; it must reach the recipient as
    // a real non-breaking space, never as literal "&nbsp;" text
    // (`&amp;nbsp;` on the wire) in either the HTML or the plain twin.
    assert_eq!(decode_entities("a&nbsp;b"), "a\u{a0}b");
    let s = sanitize_for_send("<p>a&nbsp;&nbsp;b</p>");
    assert!(s.contains('\u{a0}'), "nbsp lost: {s:?}");
    assert!(!s.contains("&amp;nbsp;"), "nbsp leaked: {s:?}");
    assert!(!s.contains("&nbsp;"), "nbsp leaked: {s:?}");
    assert_eq!(html_to_text("<p>a&nbsp;b</p>"), "a\u{a0}b");
}

#[test]
fn invisible_format_entities_stay_invisible() {
    // Newsletter spacer divs (`&zwnj;` runs) must not leak as literal
    // "&zwnj;" text in the reader or get baked into forwards that way.
    assert_eq!(decode_entities("a&zwnj;b"), "a\u{200c}b");
    let s = sanitize("<p>a&zwnj;&zwj;b</p>", false);
    assert!(s.html.contains('\u{200c}'), "zwnj lost: {s:?}");
    assert!(!s.html.contains("&amp;zwnj;"), "zwnj leaked: {s:?}");
    assert!(!s.html.contains("&zwnj;"), "zwnj leaked: {s:?}");
    assert_eq!(html_to_text("<p>a&zwnj;b</p>"), "a\u{200c}b");
}

#[test]
fn oversized_body_truncates_without_splitting_a_character() {
    // One 2-byte char sitting exactly across MAX_HTML_BYTES: slicing by
    // byte index there used to panic, taking the reader down with it.
    for pad in 0..4 {
        let mut s = "a".repeat(MAX_HTML_BYTES - 1 - pad);
        s.push('\u{20ac}'); // 3 bytes
        s.push('ä'); // 2 bytes
        s.push_str("<p>tail</p>");
        let out = sanitize(&s, false);
        assert!(!out.html.is_empty());
    }
    // The cap still holds, and nothing is cut mid-character.
    let big = "ä".repeat(MAX_HTML_BYTES);
    assert!(truncate_on_char_boundary(&big, MAX_HTML_BYTES).len() <= MAX_HTML_BYTES);
    // Short input is returned whole.
    assert_eq!(truncate_on_char_boundary("äöü", MAX_HTML_BYTES), "äöü");
    // A cut that lands inside the first character yields nothing rather
    // than half a character.
    assert_eq!(truncate_on_char_boundary("ä", 1), "");
}

#[test]
fn named_entities_decode_to_their_characters() {
    // German mail generators spell umlauts as named entities; the reader
    // and the plain twin must show the letters, not "&auml;".
    assert_eq!(
        decode_entities("Gr&uuml;&szlig;e aus K&ouml;ln &amp; M&Auml;rz &euro;"),
        "Grüße aus Köln & MÄrz €"
    );
    let s = sanitize(
        "<p>Sch&ouml;ne Gr&uuml;&szlig;e &ndash; &bdquo;Hallo&ldquo;</p>",
        false,
    );
    assert!(s.html.contains("Schöne Grüße – „Hallo“"), "{s:?}");
    assert!(!s.html.contains("&amp;"), "{s:?}");
    assert_eq!(html_to_text("<p>M&uuml;ller</p>"), "Müller");
    // Names are case-sensitive and unknown ones stay visible as typed.
    assert_eq!(decode_entities("&AUML; &bogus;"), "&AUML; &bogus;");
    // Decoded markup characters are still escaped on the way out.
    let s = sanitize("<p>&lt;script&gt;</p>", false);
    assert!(!s.html.contains("<script"), "{s:?}");
}

#[test]
fn links_open_only_on_web_schemes() {
    for url in [
        "https://example.com/x",
        "http://example.com/",
        "mailto:a@example.com",
        "MAILTO:a@example.com",
        "  HTTPS://example.com  ",
    ] {
        assert!(link_info(url).safe, "{url}");
    }
    for url in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "java	script:alert(1)",
        "data:text/html,<p>x</p>",
        "file:///etc/passwd",
        "vbscript:msgbox(1)",
        "ftp://example.com/x",
        "",
        "#fragment",
    ] {
        assert!(!link_info(url).safe, "{url}");
    }
}

#[test]
fn links_split_for_the_examine_dialog() {
    let i = link_info("https://user@example.com:443/a/b?x=1");
    assert_eq!(
        (i.scheme.as_str(), i.host.as_str(), i.path.as_str()),
        ("https", "example.com", "/a/b?x=1")
    );
    let m = link_info("Mailto:a@example.com?subject=hi");
    assert_eq!(
        (m.scheme.as_str(), m.host.as_str()),
        ("mailto", "example.com")
    );
    assert_eq!(m.path, "");
    assert_eq!(link_info("https://example.com").path, "");
    let empty = link_info("");
    assert_eq!(
        (empty.scheme, empty.host, empty.path),
        (String::new(), String::new(), String::new())
    );
}
