use super::*;

const POSTFIX: &str = "Reporting-MTA: dns; mx.example.org\r\n\
X-Postfix-Queue-ID: 4ABC\r\n\
Arrival-Date: Tue,  6 Oct 2026 14:00:00 +0000 (UTC)\r\n\
\r\n\
Final-Recipient: rfc822; bob@example.net\r\n\
Original-Recipient: rfc822;bob@example.net\r\n\
Action: failed\r\n\
Status: 5.1.1\r\n\
Remote-MTA: dns; mx.example.net\r\n\
Diagnostic-Code: smtp; 550 5.1.1 <bob@example.net>: Recipient address\r\n\
\x20   rejected: User unknown\r\n\
\r\n\
Final-Recipient: rfc822; carol@example.net\r\n\
Action: delayed\r\n\
Status: 4.4.1\r\n\
Diagnostic-Code: X-Postfix; connect to mx.example.net timed out\r\n";

#[test]
fn parses_recipient_blocks() {
    let dsn = parse_dsn(POSTFIX).expect("dsn");
    assert_eq!(dsn.reporting_mta.as_deref(), Some("mx.example.org"));
    assert_eq!(dsn.recipients.len(), 2);
    let bob = &dsn.recipients[0];
    assert_eq!(bob.address, "bob@example.net");
    assert_eq!(bob.action, "failed");
    assert_eq!(bob.status.as_deref(), Some("5.1.1"));
    assert_eq!(
        bob.reason.as_deref(),
        Some("The address does not exist (5.1.1)")
    );
    assert_eq!(bob.action_label, "Failed");
    assert_eq!(
        bob.diagnostic.as_deref(),
        Some("550 5.1.1 <bob@example.net>: Recipient address rejected: User unknown")
    );
    let carol = &dsn.recipients[1];
    assert_eq!(carol.action, "delayed");
    assert_eq!(
        carol.reason.as_deref(),
        Some("The receiving server did not answer (4.4.1)")
    );
    assert_eq!(outcome(&dsn.recipients), "failed");
}

#[test]
fn status_falls_back_to_the_diagnostic() {
    let dsn = parse_dsn(
        "Final-Recipient: rfc822;x@example.net\nAction: failed\nDiagnostic-Code: smtp; 552 5.2.2 Mailbox full\n",
    )
    .unwrap();
    assert_eq!(dsn.recipients[0].status.as_deref(), Some("5.2.2"));
    assert_eq!(
        dsn.recipients[0].reason.as_deref(),
        Some("The mailbox is full (5.2.2)")
    );
}

#[test]
fn explains_codes_by_detail_then_class() {
    assert_eq!(
        explain_status("5.7.1"),
        Some("The receiving server refused the message")
    );
    assert_eq!(explain_status("4.2.2"), Some("The mailbox is full"));
    assert_eq!(explain_status("5.1.99"), Some("Problem with the address"));
    assert_eq!(explain_status("2.0.0"), Some("Delivered"));
    assert_eq!(explain_status("bogus"), None);
    assert_eq!(
        status_code("smtp; 550 5.7.26 unauthenticated"),
        Some("5.7.26".to_string())
    );
    assert_eq!(status_code("version 1.2.3.4"), None);
}

#[test]
fn no_recipient_is_no_report() {
    assert_eq!(parse_dsn("Reporting-MTA: dns; mx.example.org\r\n"), None);
    assert_eq!(parse_dsn(""), None);
}

#[test]
fn outcome_is_the_worst_action() {
    let r = |action: &str| ReportRecipient {
        address: "a@example.com".into(),
        action: action.into(),
        action_label: String::new(),
        tone: String::new(),
        status: None,
        reason: None,
        diagnostic: None,
    };
    assert_eq!(outcome(&[r("delivered"), r("delayed")]), "delayed");
    assert_eq!(outcome(&[r("relayed")]), "relayed");
    assert_eq!(outcome(&[r("delivered"), r("relayed")]), "relayed");
    assert_eq!(outcome(&[r("delivered"), r("expanded")]), "delivered");
}

#[test]
fn original_headers_without_a_trailing_blank_line() {
    let h = OriginalHeaders::parse(b"Message-ID: <orig@example.com>\r\nSubject: Hello").unwrap();
    assert_eq!(h.message_id.as_deref(), Some("orig@example.com"));
    assert_eq!(h.subject.as_deref(), Some("Hello"));
}

#[test]
fn parses_read_receipts() {
    let mdn = parse_mdn(
        "Reporting-UA: pc.example.org; SomeMailer 2.0
Original-Recipient: rfc822;jane@example.org
Final-Recipient: rfc822; jane@example.org
Original-Message-ID: <asked@example.com>
Disposition: automatic-action/MDN-sent-automatically;
  deleted
",
    )
    .expect("mdn");
    assert_eq!(mdn.recipient, "jane@example.org");
    assert_eq!(mdn.disposition, "deleted");
    assert_eq!(
        mdn.original_message_id.as_deref(),
        Some("<asked@example.com>")
    );
    assert_eq!(mdn.reporting_ua.as_deref(), Some("pc.example.org"));
    assert_eq!(disposition_text(&mdn.disposition).0, "Deleted unread");

    let modified = parse_mdn(
        "Final-Recipient: rfc822;a@example.org
Disposition: manual-action/MDN-sent-manually; displayed/error
",
    )
    .unwrap();
    assert_eq!(modified.disposition, "displayed");
    assert_eq!(
        parse_mdn(
            "Final-Recipient: rfc822;a@example.org
"
        ),
        None
    );
    assert_eq!(
        parse_mdn(
            "Disposition: x; displayed
"
        ),
        None
    );
}

#[test]
fn tones_follow_the_action() {
    assert_eq!(action_tone("failed"), "negative");
    assert_eq!(action_tone("delayed"), "warning");
    assert_eq!(action_tone("delivered"), "positive");
    assert_eq!(action_tone("relayed"), "neutral");
    assert_eq!(action_label("relayed"), "Handed on");
    assert!(is_disposition_part(Some(
        "Message/Disposition-Notification"
    )));
    assert!(!is_disposition_part(Some("message/delivery-status")));
}

#[test]
fn recipients_are_capped() {
    let mut text = String::from("Reporting-MTA: dns; mx.example.org\r\n\r\n");
    for i in 0..200 {
        text.push_str(&format!(
            "Final-Recipient: rfc822; u{i}@example.net\r\nAction: failed\r\n\r\n"
        ));
    }
    let dsn = parse_dsn(&text).unwrap();
    assert_eq!(dsn.recipients.len(), MAX_DSN_RECIPIENTS);
    assert_eq!(dsn.recipients[0].address, "u0@example.net");
}

#[test]
fn only_the_report_prefix_is_read() {
    // A recipient past the byte cap is never reached, and the cut lands on
    // a character boundary rather than panicking inside one.
    let filler = "é".repeat(MAX_REPORT_BYTES);
    let text = format!(
        "Final-Recipient: rfc822; first@example.net\r\n\r\nX-Note: {filler}\r\n\r\n\
         Final-Recipient: rfc822; late@example.net\r\n"
    );
    let dsn = parse_dsn(&text).unwrap();
    let addrs: Vec<_> = dsn.recipients.iter().map(|r| r.address.as_str()).collect();
    assert_eq!(addrs, ["first@example.net"]);
}
