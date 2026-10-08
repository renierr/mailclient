use super::*;

fn html_mail() -> AnswerSource {
    AnswerSource {
        from: "alice@example.com".into(),
        from_name: "Alice".into(),
        to: vec!["me@example.org".into(), "bob@example.com".into()],
        cc: vec![
            "Carol <carol@example.com>".into(),
            "ALICE@example.com".into(),
        ],
        subject: "Plans".into(),
        date: "2026-09-12 13:50".into(),
        is_html: true,
        body_html: "<p>Hi <b>there</b></p>".into(),
        body_text: "Hi there".into(),
        ..Default::default()
    }
}

fn opts() -> AnswerOptions {
    AnswerOptions {
        own_address: "Me@Example.org".into(),
        signature: Some("\n Me\nExample Ltd \n\n".into()),
        reply_below_quote: false,
    }
}

#[test]
fn reply_quotes_html_in_a_blockquote_under_the_attribution() {
    let d = answer_draft(&html_mail(), AnswerMode::Reply, &opts());
    assert_eq!(d.to, "alice@example.com");
    assert_eq!(d.cc, "");
    assert_eq!(d.subject, "Re: Plans");
    assert_eq!(
        d.quote_html,
        "<p>On 2026-09-12 13:50, Alice &lt;alice@example.com&gt; wrote:</p>\
         <blockquote><p>Hi <b>there</b></p></blockquote>"
    );
    assert_eq!(d.signature_html, "<p>-- <br> Me<br>Example Ltd </p>");
    assert_eq!(d.signature_text, "-- \n Me\nExample Ltd ");
    assert!(!d.quote_first);
    assert_eq!(
        d.body_html,
        format!("{TEXT_SLOT}{}{}", d.signature_html, d.quote_html)
    );
    assert_eq!(d.notice_addr, "");
}

#[test]
fn plain_mail_is_quoted_as_escaped_citations() {
    let src = AnswerSource {
        is_html: false,
        body_html: String::new(),
        body_text: "a < b\nline two\n".into(),
        ..html_mail()
    };
    let d = answer_draft(&src, AnswerMode::Reply, &AnswerOptions::default());
    assert!(d
        .quote_html
        .ends_with("<p>&gt; a &lt; b<br>&gt; line two</p>"));
    assert_eq!(d.signature_html, "");
    assert_eq!(d.body_html, format!("{TEXT_SLOT}{}", d.quote_html));
}

#[test]
fn reply_all_copies_everyone_but_us_and_the_target() {
    let d = answer_draft(&html_mail(), AnswerMode::ReplyAll, &opts());
    assert_eq!(d.to, "alice@example.com");
    assert_eq!(d.cc, "bob@example.com, Carol <carol@example.com>");
}

#[test]
fn a_differing_reply_to_is_the_target_and_raises_the_notice() {
    let src = AnswerSource {
        reply_to: "list@example.org".into(),
        ..html_mail()
    };
    let d = answer_draft(&src, AnswerMode::ReplyAll, &opts());
    assert_eq!(d.to, "list@example.org");
    assert_eq!(d.notice_addr, "list@example.org");
    assert_eq!(d.notice_sender, "alice@example.com");
    assert_eq!(
        d.notice,
        "Replies to this mail go to list@example.org — not to the sender (alice@example.com)."
    );
    // The sender is copied once; its other spelling in Cc stays out.
    assert_eq!(
        d.cc,
        "alice@example.com, bob@example.com, Carol <carol@example.com>"
    );
}

#[test]
fn bottom_posting_puts_the_quote_first() {
    let o = AnswerOptions {
        reply_below_quote: true,
        ..opts()
    };
    let d = answer_draft(&html_mail(), AnswerMode::Reply, &o);
    assert!(d.quote_first);
    assert_eq!(
        d.body_html,
        format!("{}{TEXT_SLOT}{}", d.quote_html, d.signature_html)
    );
}

#[test]
fn forward_has_no_recipients_and_a_forward_header() {
    let d = answer_draft(&html_mail(), AnswerMode::Forward, &opts());
    assert_eq!(d.to, "");
    assert_eq!(d.subject, "Fwd: Plans");
    assert!(d.quote_html.starts_with(
        "<p>— Forwarded message —<br>From: Alice &lt;alice@example.com&gt;<br>\
         Date: 2026-09-12 13:50<br>Subject: Plans</p><blockquote>"
    ));
    assert!(!d.quote_first);
    assert_eq!(
        d.body_html,
        format!("{TEXT_SLOT}{}{}", d.signature_html, d.quote_html)
    );
}

#[test]
fn subject_prefixes_never_stack() {
    assert_eq!(prefixed("Re: x", "Re:", &["re:", "aw:"]), "Re: x");
    assert_eq!(prefixed("AW: x", "Re:", &["re:", "aw:"]), "AW: x");
    assert_eq!(prefixed("RE:x", "Re:", &["re:", "aw:"]), "RE:x");
    assert_eq!(prefixed("x", "Re:", &["re:", "aw:"]), "Re: x");
    assert_eq!(prefixed("", "Re:", &["re:", "aw:"]), "Re:");
    assert_eq!(prefixed("Fw: x", "Fwd:", &["fwd:", "fw:", "wg:"]), "Fw: x");
    assert_eq!(
        prefixed("Re: x", "Fwd:", &["fwd:", "fw:", "wg:"]),
        "Fwd: Re: x"
    );
}

#[test]
fn blank_signature_is_no_signature() {
    let o = AnswerOptions {
        signature: Some(" \n\n".into()),
        ..opts()
    };
    let d = answer_draft(&html_mail(), AnswerMode::Reply, &o);
    assert_eq!(d.signature_html, "");
    assert_eq!(d.signature_text, "");
}

#[test]
fn modes_parse_from_adapter_strings() {
    assert_eq!(AnswerMode::parse("reply"), Some(AnswerMode::Reply));
    assert_eq!(AnswerMode::parse("reply_all"), Some(AnswerMode::ReplyAll));
    assert_eq!(AnswerMode::parse("forward"), Some(AnswerMode::Forward));
    assert_eq!(AnswerMode::parse("Reply"), None);
}

#[test]
fn blank_draft_is_a_text_slot_above_the_signature() {
    let d = blank_draft(&opts());
    assert_eq!(d.body_html, format!("{TEXT_SLOT}{}", d.signature_html));
    // Without a signature the editor's placeholder shows instead.
    assert_eq!(blank_draft(&AnswerOptions::default()).body_html, "");
    assert_eq!(d.signature_text, "-- \n Me\nExample Ltd ");
    assert_eq!(d.to, "");
    assert_eq!(d.subject, "");
    assert_eq!(d.quote_html, "");
}

fn sent_mail() -> AnswerSource {
    AnswerSource {
        from: "me@example.org".into(),
        from_name: "Me".into(),
        to: vec![
            "Bob <bob@example.com>".into(),
            "ME@example.org".into(),
            "dave@example.com".into(),
        ],
        cc: vec!["carol@example.com".into(), "bob@example.com".into()],
        ..html_mail()
    }
}

#[test]
fn replying_to_own_mail_answers_its_recipients() {
    let d = answer_draft(&sent_mail(), AnswerMode::Reply, &opts());
    assert_eq!(d.to, "Bob <bob@example.com>, dave@example.com");
    assert_eq!(d.cc, "");
    assert_eq!(d.notice_addr, "");

    let d = answer_draft(&sent_mail(), AnswerMode::ReplyAll, &opts());
    assert_eq!(d.to, "Bob <bob@example.com>, dave@example.com");
    assert_eq!(d.cc, "carol@example.com");
}

#[test]
fn own_mail_sent_only_to_ourselves_answers_ourselves() {
    let src = AnswerSource {
        to: vec!["me@example.org".into()],
        cc: Vec::new(),
        ..sent_mail()
    };
    let d = answer_draft(&src, AnswerMode::Reply, &opts());
    assert_eq!(d.to, "me@example.org");
}

#[test]
fn own_mail_with_a_foreign_reply_to_follows_the_reply_to() {
    let src = AnswerSource {
        reply_to: "list@example.org".into(),
        ..sent_mail()
    };
    let d = answer_draft(&src, AnswerMode::Reply, &opts());
    assert_eq!(d.to, "list@example.org");
}

#[test]
fn reply_comes_from_the_alias_the_envelope_names() {
    // The mailbox is Delivered-To; X-Original-To keeps the alias.
    let src = AnswerSource {
        to: vec!["Sales <sales@example.org>".into(), "bob@example.com".into()],
        cc: Vec::new(),
        envelope_to: vec!["sales@example.org".into(), "me@example.org".into()],
        ..html_mail()
    };
    let d = answer_draft(&src, AnswerMode::ReplyAll, &opts());
    assert_eq!(d.from, "sales@example.org");
    // Answering as that address does not copy it in as well.
    assert_eq!(d.cc, "bob@example.com");
}

#[test]
fn a_colleague_in_to_is_not_taken_when_the_envelope_says_otherwise() {
    // Bcc'd on a shared domain: To names someone else, delivery names us.
    let src = AnswerSource {
        to: vec!["colleague@example.org".into()],
        cc: Vec::new(),
        envelope_to: vec!["me@example.org".into()],
        ..html_mail()
    };
    assert_eq!(answer_draft(&src, AnswerMode::Reply, &opts()).from, "");
    let other = AnswerSource {
        envelope_to: vec!["someone@sub.example.org".into()],
        ..src
    };
    assert_eq!(answer_draft(&other, AnswerMode::Reply, &opts()).from, "");
}

#[test]
fn without_envelope_headers_the_account_sends() {
    // To alone cannot tell an alias from a colleague on the same domain.
    let src = AnswerSource {
        to: vec!["sales@example.org".into()],
        ..html_mail()
    };
    assert_eq!(answer_draft(&src, AnswerMode::Reply, &opts()).from, "");
}

#[test]
fn forward_and_own_mail_keep_the_account_sender() {
    let src = AnswerSource {
        envelope_to: vec!["sales@example.org".into()],
        ..html_mail()
    };
    assert_eq!(answer_draft(&src, AnswerMode::Forward, &opts()).from, "");
    assert_eq!(
        answer_draft(&sent_mail(), AnswerMode::Reply, &opts()).from,
        ""
    );
}
