//! Tests for outgoing MIME assembly and body resolution.
//!
//! Split out of the parent once they outgrew it (see AGENT.md, "File size
//! & where tests live"). Still a `#[cfg(test)]` submodule of it, so
//! `super::*` reaches its private items exactly as before.

use super::*;

use super::super::support::test_account;

#[test]
fn formatting_a_draft_does_not_submit_or_queue_it() {
    let account = test_account();
    let to = vec!["you@example.com".to_string()];
    let cc = Vec::new();
    let bcc = Vec::new();
    let files = Vec::new();
    let request = SendRequest {
        to: &to,
        cc: &cc,
        bcc: &bcc,
        from: None,
        from_name: None,
        reply_to: None,
        subject: "unfinished",
        body_text: "<p>still writing</p>",
        body_html: Some("<p>still writing</p>"),
        attachments: &files,
        format: SendFormat::Auto,
        include_plain: true,
        policy: &SendPolicy::TestAllowlist(Vec::new()),
        password: "",
        imap_password: None,
        request_mdn: false,
    };
    let raw = String::from_utf8(format_draft(&account, &request).unwrap()).unwrap();
    assert!(raw.contains("Subject: unfinished"));
    assert!(raw.contains("still writing"));
}

#[test]
fn a_draft_without_any_recipient_still_saves() {
    // A half-written draft (no To/Cc/Bcc yet) must be storable: lettre
    // refuses to build without an envelope, so the builder gets a
    // sender-pointed placeholder — headers + body carry the real
    // `To: undisclosed-recipients:;` line either way.
    let account = test_account();
    let empty: Vec<String> = Vec::new();
    let files = Vec::new();
    let request = SendRequest {
        to: &empty,
        cc: &empty,
        bcc: &empty,
        from: None,
        from_name: None,
        reply_to: None,
        subject: "not yet addressed",
        body_text: "<p>still writing</p>",
        body_html: Some("<p>still writing</p>"),
        attachments: &files,
        format: SendFormat::Auto,
        include_plain: true,
        policy: &SendPolicy::TestAllowlist(Vec::new()),
        password: "",
        imap_password: None,
        request_mdn: false,
    };
    let raw = String::from_utf8(format_draft(&account, &request).unwrap()).unwrap();
    assert!(raw.contains("Subject: not yet addressed"));
    assert!(raw.contains("To: undisclosed-recipients:;"));
    assert!(raw.contains("still writing"));
}

#[test]
fn bodies_resilient_across_formats() {
    // Legacy: composer rich HTML arrived in `body_text`, must not leak tags.
    let (p, h) = resolve_bodies("<b>hi</b><script>x()</script>", None, SendFormat::Plain);
    assert_eq!(p, "hi");
    assert!(h.is_none());
    // Multipart derives the missing plain side.
    let (p2, h2) = resolve_bodies("<p>hi<br>there</p>", None, SendFormat::Multipart);
    assert!(h2.unwrap().contains("hi"));
    assert_eq!(p2, "hi\nthere");
    // Plain input still gains an html twin in multipart mode.
    let (p3, h3) = resolve_bodies("hello", None, SendFormat::Multipart);
    assert_eq!(p3, "hello");
    assert!(h3.unwrap().contains("hello"));
    // Outgoing scripts are stripped even for the sender's own HTML.
    let (_, evil) = resolve_bodies("<p>t</p><script>alert(1)</script>", None, SendFormat::Html);
    assert!(!evil.unwrap().contains("script"));
    // Unknown format string falls back to auto, never panics.
    assert_eq!(SendFormat::parse("nonsense"), SendFormat::Auto);
    assert_eq!(SendFormat::parse(""), SendFormat::Auto);
}

#[test]
fn full_html_document_body_survives_sanitizing() {
    // A complete document is the normal case: Qt's rich-text editor emits
    // one, and so does most HTML mail. Listing html/body/meta as
    // content-dropping tags made every such body sanitize to nothing, so
    // the recipient got "(empty)".
    let doc = concat!(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0//EN\">",
        "<html><head><meta charset=\"utf-8\">",
        "<style type=\"text/css\">p { color: red }</style></head>",
        "<body><p>Hello <b>bold</b> world</p></body></html>"
    );
    for format in [SendFormat::Plain, SendFormat::Multipart, SendFormat::Html] {
        let (plain, html) = resolve_bodies(doc, Some(doc), format);
        assert!(
            plain.contains("Hello") && plain.contains("bold"),
            "plain lost the body for {format:?}: {plain:?}"
        );
        assert_ne!(plain, "(empty)", "for {format:?}");
        if let Some(h) = html {
            assert!(
                h.contains("Hello"),
                "html lost the body for {format:?}: {h:?}"
            );
            // Semantic tags survive; the dropped <style> block does not.
            assert!(h.contains("<b>"), "formatting lost for {format:?}: {h:?}");
            assert!(!h.contains("color: red"), "style leaked for {format:?}");
        }
    }
}

#[test]
fn reply_to_header_roundtrips() {
    use SendFormat::Plain;
    assert!(parse_reply_to("").unwrap().is_none());
    assert!(parse_reply_to("   ").unwrap().is_none());
    let mbox = parse_reply_to("replies@example.com").unwrap().unwrap();
    assert_eq!(mbox.email.to_string(), "replies@example.com");
    assert!(parse_reply_to("not an address").is_err());

    let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
    let call = |reply_to| {
        assemble_message(
            from.clone(),
            "hi",
            valid_mailboxes(&["bob@example.com".to_string()]),
            None,
            &[],
            &[],
            reply_to,
            Plain,
            "hello".to_string(),
            None,
            &[],
            false,
        )
        .unwrap()
    };
    let raw = String::from_utf8(call(Some(mbox)).formatted()).unwrap();
    assert!(
        raw.contains("Reply-To: replies@example.com"),
        "no Reply-To: {raw:?}"
    );
    let raw_off = String::from_utf8(call(None).formatted()).unwrap();
    assert!(
        !raw_off.contains("Reply-To"),
        "Reply-To leaked in: {raw_off:?}"
    );
}

#[test]
fn bcc_only_send_carries_group_to_and_bcc_envelope() {
    use SendFormat::Plain;
    let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
    let bcc = vec!["hidden@example.com".to_string()];
    // Placeholder text becomes the group name recipients see…
    let m = assemble_message(
        from.clone(),
        "hi",
        vec![],
        Some("my friends"),
        &[],
        &bcc,
        None,
        Plain,
        "hello".to_string(),
        None,
        &[],
        false,
    )
    .unwrap();
    let raw = String::from_utf8(m.formatted()).unwrap();
    assert!(raw.contains("To: my friends:;"), "no group To: {raw:?}");
    assert_eq!(
        m.envelope().to(),
        &[lettre::Address::new("hidden", "example.com").unwrap()]
    );
    // …blank To falls back to the standard group, envelope intact.
    let m2 = assemble_message(
        from,
        "hi",
        vec![],
        Some("undisclosed-recipients"),
        &[],
        &bcc,
        None,
        Plain,
        "hello".to_string(),
        None,
        &[],
        false,
    )
    .unwrap();
    let raw2 = String::from_utf8(m2.formatted()).unwrap();
    assert!(
        raw2.contains("To: undisclosed-recipients:;"),
        "no fallback To: {raw2:?}"
    );
    assert_eq!(m2.envelope().to().len(), 1);
}

#[test]
fn from_name_renders_display_name() {
    use SendFormat::Plain;
    let named = lettre::message::Mailbox::new(
        Some("John Doe".to_string()),
        "me@example.com".parse().unwrap(),
    );
    let m = assemble_message(
        named,
        "hi",
        valid_mailboxes(&["bob@example.com".to_string()]),
        None,
        &[],
        &[],
        None,
        Plain,
        "hello".to_string(),
        None,
        &[],
        false,
    )
    .unwrap();
    let raw = String::from_utf8(m.formatted()).unwrap();
    assert!(
        raw.contains("From: \"John Doe\" <me@example.com>"),
        "bad From: {raw:?}"
    );
}

#[test]
fn read_receipt_request_adds_mdn_header() {
    use SendFormat::Plain;
    let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
    let call = |mdn: bool| {
        assemble_message(
            from.clone(),
            "hi",
            valid_mailboxes(&["bob@example.com".to_string()]),
            None,
            &[],
            &[],
            None,
            Plain,
            "hello".to_string(),
            None,
            &[],
            mdn,
        )
        .unwrap()
    };
    let raw = String::from_utf8(call(true).formatted()).unwrap();
    assert!(
        raw.contains("Disposition-Notification-To: me@example.com"),
        "no MDN header: {raw:?}"
    );
    let raw_off = String::from_utf8(call(false).formatted()).unwrap();
    assert!(
        !raw_off.contains("Disposition-Notification-To"),
        "MDN leaked in: {raw_off:?}"
    );
}
