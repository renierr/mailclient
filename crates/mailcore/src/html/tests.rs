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
fn blocked_remote_img_is_our_own_compact_badge() {
    // Long alt text used to render inline and break narrow table cells
    // per-character into tall columns; the badge below stays 64x48 and
    // reveals the alt on hover (title) or tap (details disclosure).
    let long_alt = "A".repeat(100);
    let s = sanitize(
        &format!("<img src=\"https://example.com/t.png\" alt=\"{long_alt}\">"),
        false,
    );
    assert!(s.had_remote);
    assert!(!s.html.contains("example.com"));
    assert!(!s.html.contains("[image:"));
    assert!(!s.html.contains("<img"), "{}", s.html);
    assert!(
        s.html.contains("<details class=\"mc-blocked\">"),
        "{}",
        s.html
    );
    assert!(s.html.contains("<svg"), "{}", s.html);
    assert!(!s.html.contains("<script"), "{}", s.html);
    assert!(
        s.html.contains(&format!("title=\"{long_alt}\"")),
        "{}",
        s.html
    );
    assert!(
        s.html.contains(&format!("<span>{long_alt}</span>")),
        "{}",
        s.html
    );

    let generic = sanitize("<img src=\"https://example.com/t.png\">", false);
    assert!(
        generic.html.contains("<details class=\"mc-blocked\">"),
        "{}",
        generic.html
    );
    assert!(generic.html.contains("Blocked image"), "{}", generic.html);
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
fn void_input_and_embed_do_not_swallow_the_rest() {
    let s = sanitize(
        "<input type=\"checkbox\" checked><p>after input</p>\
         <embed src=\"https://evil.example.net/x.swf\"><p>after embed</p>\
         <form><input name=\"q\"><p>in form</p></form><p>after form</p>",
        true,
    );
    for kept in ["after input", "after embed", "after form"] {
        assert!(s.html.contains(kept), "lost content: {kept} in {}", s.html);
    }
    assert!(!s.html.contains("<input"));
    assert!(!s.html.contains("in form"));
    assert!(!s.html.contains("evil.example.net"));
}

#[test]
fn an_invisible_link_overlay_loses_its_reach() {
    let s = sanitize(
        "<p>Pay here</p><a href=\"https://evil.example.net/\" \
         style=\"display:block;width:100%;height:1400px;opacity:0;margin:-16px 0;margin-top:-1400px\">x</a>",
        false,
    );
    assert!(
        !s.html.contains("margin"),
        "negative margin kept: {}",
        s.html
    );
    assert!(!s.html.contains("opacity"), "zero opacity kept: {}", s.html);
    // The link and its harmless layout stay.
    assert!(s.html.contains("display:block;width:100%;height:1400px;"));
    assert!(s.html.contains("href=\"https://evil.example.net/\""));

    for (style, kept) in [
        ("opacity:0.01", false),
        ("opacity:5%", false),
        ("opacity:nan", false),
        ("opacity:.5", true),
        ("opacity:80%", true),
        ("margin:0 auto", true),
        ("margin-left:-2px", false),
    ] {
        let s = sanitize(&format!("<div style=\"{style}\">t</div>"), false);
        assert_eq!(s.html.contains("style="), kept, "{style}: {}", s.html);
    }
}

#[test]
fn an_unclosed_head_does_not_hide_the_body() {
    // `</head>` is optional; a parser ends the head at `<body>`, at the
    // first tag that cannot live in one, or at visible text.
    for raw in [
        "<html><head><meta charset=\"utf-8\"><title>T</title><body><p>shown</p></body></html>",
        "<head><style>p{color:red}</style><p>shown</p>",
        "<head><link rel=\"stylesheet\" href=\"x.css\">shown",
        "<head><title>T</title></body><p>shown</p>",
    ] {
        let s = sanitize(raw, false);
        assert!(s.html.contains("shown"), "body lost for {raw}: {}", s.html);
        assert!(
            !s.html.contains('T'),
            "head text leaked for {raw}: {}",
            s.html
        );
        assert!(
            !s.html.contains("color"),
            "style leaked for {raw}: {}",
            s.html
        );
    }
}

#[test]
fn a_close_only_ends_its_own_drop_tag() {
    // Under a depth count `</form>` closed the `<style>`, leaking the rule
    // text; and a `<form>` written inside a script left the count at one
    // after `</script>`, hiding everything after it.
    let s = sanitize(
        "<style></form>p{x:y}</style><p>a</p>\
         <script>document.write('<form>')</script><p>b</p>",
        false,
    );
    assert!(!s.html.contains("p{x:y}"), "style leaked: {}", s.html);
    assert!(!s.html.contains("document"), "script leaked: {}", s.html);
    assert!(
        s.html.contains("<p>a</p>") && s.html.contains("<p>b</p>"),
        "{}",
        s.html
    );
}

#[test]
fn an_unclosed_style_still_drops_the_rest() {
    // Raw text to the end, as a browser renders it; only `<head>` gets the
    // implicit close.
    let s = sanitize("<p>a</p><style>p{x:y}<p>b</p>", false);
    assert!(s.html.contains("<p>a</p>"));
    assert!(
        !s.html.contains('b') && !s.html.contains("x:y"),
        "{}",
        s.html
    );
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
fn ampersands_without_a_closing_semicolon_decode_in_linear_time() {
    // The closing `;` used to be found with `s[i..].find(';')` and filtered
    // for `< 24` afterwards, so the scan reached the end of the string before
    // being discarded: quadratic in any text full of `&` with no `;` after it.
    let s = "&".repeat(512 * 1024);
    let out = decode_entities(&s);
    assert_eq!(out.len(), s.len());
    assert_eq!(out, s);
    // A `;` outside the scan window is not an entity either.
    assert_eq!(decode_entities("&a;"), "&a;");
    let far = format!("&{};", "a".repeat(40));
    assert_eq!(decode_entities(&far), far);
    assert_eq!(decode_entities("x&"), "x&");
    assert_eq!(decode_entities("a&b"), "a&b");
}

#[test]
fn entities_on_the_scan_boundary_still_decode() {
    // The window is 23 bytes past the `&`, so the longest entity that still
    // decodes is 24 bytes in total; one more byte stays literal. The first
    // fails if the window shrinks, the second if it widens.
    let inside = "&#000000000000000000065;";
    assert_eq!(inside.len(), 24);
    assert_eq!(decode_entities(inside), "A");
    let outside = "&#0000000000000000000065;";
    assert_eq!(outside.len(), 25);
    assert_eq!(decode_entities(outside), outside);
    // Ordinary and short forms are untouched.
    assert_eq!(decode_entities("&#x00000000000000000026;"), "&");
    assert_eq!(decode_entities("&AMP;"), "&");
    assert_eq!(decode_entities("&nbsp;x&#65;y"), "\u{a0}xAy");
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
fn private_hosts_stay_blocked_with_query_or_fragment() {
    for url in [
        "http://192.168.1.2?x=1",
        "http://192.168.1.2#x",
        "http://localhost?x=1",
        "http://localhost#x",
        "http://127.0.0.1?x=1",
    ] {
        assert!(!super::urls::is_public_remote(url), "{url}");
    }
    assert!(super::urls::is_public_remote("https://example.com?x=1"));
}

#[test]
fn ipv6_hosts_are_not_mistaken_for_public_ones() {
    // `host.split(':').next()` used to cut every bracketed literal down to
    // "[", which passed every literal check, so all of these looked public.
    for url in [
        "http://[::1]/x.png",
        "http://[::1]:8080/x.png",
        "http://[::]/x.png",
        "http://[0:0:0:0:0:0:0:1]:80/x.png",
        "http://[fc00::1]/x.png",
        "http://[fd12:3456::1]/x.png",
        "http://[fe80::1]/x.png",
        "http://[fe80::1%25eth0]/x.png",
        "http://[ff02::1]/x.png",
        "http://[::ffff:127.0.0.1]/x.png",
        "http://[::ffff:169.254.169.254]/x.png",
        "http://[::10.0.0.1]/x.png",
        "http://::1/x.png",
        "http://user@[::1]/x.png",
    ] {
        assert!(!super::urls::is_public_remote(url), "{url}");
    }
}

#[test]
fn link_local_and_cgnat_and_zero_addresses_stay_blocked() {
    for url in [
        // 169.254.169.254 is the cloud metadata endpoint.
        "http://169.254.169.254/latest/meta-data/",
        "http://169.254.0.1/x.png",
        "http://100.64.0.1/x.png",
        "http://100.127.255.255/x.png",
        "http://0.0.0.0/x.png",
        "http://0.1.2.3/x.png",
        "http://172.16.0.1/x.png",
        "http://172.20.5.5/x.png",
        "http://172.31.255.255/x.png",
        "http://192.0.0.8/x.png",
        "http://198.18.0.1/x.png",
        "http://240.0.0.1/x.png",
        "http://255.255.255.255/x.png",
    ] {
        assert!(!super::urls::is_public_remote(url), "{url}");
    }
    // Just outside the blocked ranges.
    assert!(super::urls::is_public_remote("http://100.63.255.255/x.png"));
    assert!(super::urls::is_public_remote("http://100.128.0.0/x.png"));
    assert!(super::urls::is_public_remote("http://172.15.0.1/x.png"));
    assert!(super::urls::is_public_remote("http://172.32.0.1/x.png"));
    assert!(super::urls::is_public_remote("http://169.253.0.1/x.png"));
    assert!(super::urls::is_public_remote("http://169.255.0.1/x.png"));
    assert!(super::urls::is_public_remote(
        "http://239.255.255.255/x.png"
    ));
}

#[test]
fn numeric_ip_shorthand_stays_blocked() {
    // inet_aton forms a browser resolves even though they are not dotted
    // quads, so blocking the literal range means blocking these too.
    for url in [
        "http://127.1/x.png",
        "http://127.0.1/x.png",
        "http://10.1/x.png",
        "http://192.168.1/x.png",
        "http://2130706433/x.png",
        "http://2852039166/latest/meta-data/",
        "http://0/x.png",
        "http://1/x.png",
        "http://4294967295/x.png",
    ] {
        assert!(!super::urls::is_public_remote(url), "{url}");
    }
    assert!(super::urls::is_public_remote("http://1096476673/x.png"));
}

#[test]
fn hostnames_and_public_hosts_stay_allowed() {
    for url in [
        "https://example.com/x.png",
        "http://example.com:8443/x.png",
        "https://user@example.com/x.png",
        "https://sub.domain.example.co.uk/x.png",
        "https://example.com./x.png",
        "https://example.local.example.com/x.png",
        "https://2001-db8.example.com/x.png",
        "http://172.example.com/x.png",
        // A path that merely mentions a blocked name is not a host.
        "http://notlocal.example.com/localhost/x.png",
        "https://example.com:443/a?b=c#d",
    ] {
        assert!(super::urls::is_public_remote(url), "{url}");
    }
}

#[test]
fn private_host_names_stay_blocked() {
    for url in [
        "http://localhost/x.png",
        "http://localhost.evil.example.com/x.png",
        "http://foo.local/x.png",
        "http://foo.local.",
        "http://LOCALHOST/x.png",
        "http://LOCALHOST.local./x.png",
        // The root dot must not hide the address or a private name.
        "http://127.0.0.1./x.png",
        "http://192.168.1.1./x.png",
    ] {
        assert!(!super::urls::is_public_remote(url), "{url}");
    }
}

#[test]
fn blocked_hosts_never_reach_the_reader_html() {
    for host in [
        "169.254.169.254",
        "[::1]",
        "::1",
        "127.1",
        "2130706433",
        "[fc00::1]",
        "100.64.0.1",
        "localhost",
    ] {
        let html = format!("<img src=\"http://{host}/x.png\">");
        let s = super::sanitize(&html, true);
        assert!(!s.html.contains(host), "{host} survived sanitizing");
        assert!(s.had_remote, "{host} should still count as remote");
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
    // No path slash: the query/fragment belong to the path, never the host.
    let q = link_info("https://example.com?foo=bar");
    assert_eq!(
        (q.scheme.as_str(), q.host.as_str(), q.path.as_str()),
        ("https", "example.com", "?foo=bar")
    );
    let f = link_info("https://example.com#target");
    assert_eq!(
        (f.scheme.as_str(), f.host.as_str(), f.path.as_str()),
        ("https", "example.com", "#target")
    );
    let qp = link_info("https://example.com/path?x=1#f");
    assert_eq!(
        (qp.scheme.as_str(), qp.host.as_str(), qp.path.as_str()),
        ("https", "example.com", "/path?x=1#f")
    );
    let empty = link_info("");
    assert_eq!(
        (empty.scheme, empty.host, empty.path),
        (String::new(), String::new(), String::new())
    );
}

#[test]
fn an_abrupt_close_of_a_comment_does_not_eat_the_document() {
    // <!-->  and <!---> close at the `>`, the way HTML does. The old search
    // for "-->" started past it, found nothing, and consumed the rest.
    for (html, kept) in [
        (
            "<!--><p>everything after is gone</p>",
            "everything after is gone",
        ),
        ("<!---><p>dash forms too</p>", "dash forms too"),
        ("<!-- --><p>normal</p>", "normal"),
    ] {
        let s = sanitize(html, false);
        assert!(s.html.contains(kept), "{html} lost the body");
    }
}

#[test]
fn an_unterminated_processing_instruction_stops_at_the_next_gt() {
    // Looking only for ?> dropped every byte after an unterminated <?.
    let s = sanitize("<p>ok</p><?x ><p>rest</p>", false);
    assert!(s.html.contains("ok"), "the first paragraph should survive");
    assert!(
        s.html.contains("rest"),
        "the second paragraph should survive"
    );
    // An XML declaration ends at its own `?>`.
    let s = sanitize("<?xml version=\"1.0\"?><p>after</p>", false);
    assert!(
        s.html.contains("after"),
        "a real PI should not eat the body"
    );
    // And a `?>` further down does not stretch it: `<?x >` ends at its `>`.
    let s = sanitize("<?x ><p>kept</p><p>what?></p>", false);
    assert!(s.html.contains("kept"), "{s:?}");
}

#[test]
fn a_comment_without_an_end_still_drops_the_rest() {
    // An unterminated <!-- keeps the old behaviour: no closing token means
    // there is no safe place to resume, so everything after it is dropped.
    let s = sanitize("<p>before</p><!-- never closed <p>after</p>", false);
    assert!(s.html.contains("before"));
    assert!(!s.html.contains("after"));
}
