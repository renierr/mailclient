//! Fixture seeder for Spike A (throwaway).
//!
//! Seeds the dev database (`MAILCLIENT_DB`, default `./data/dev.sqlite`)
//! with ~20 `@example.com`-only mails covering the reader contract: plain,
//! simple HTML, tables, dark-designed, remote/inline images, links,
//! preheaders, alignment, entities, attachments metadata, reply threading.
//!
//! Idempotent via the `spike_seed_v1` setting. Never touches the real
//! mailbox: the spike binary refuses to open the platform database path.
//! mailcore is used read-write here only as a fixture store; no core code
//! is changed.

use mailcore::models::{FolderRole, NewAccount, NewAttachment, NewMessage};
use mailcore::store::{accounts, folders, messages};
use mailcore::{Db, Result};

pub const SEED_KEY: &str = "spike_seed_v1";
pub const SPIKE_EMAIL: &str = "spike@example.com";

pub struct SeedMail {
    pub uid: u32,
    pub slug: &'static str,
    pub kind: &'static str,
}

fn msg(account_id: i64, folder_id: i64, uid: u32, subject: &str, from: (&str, &str)) -> NewMessage {
    NewMessage {
        account_id,
        folder_id,
        uid,
        message_id_header: Some(format!("<spike-{uid}@example.com>")),
        thread_id: None,
        subject: Some(subject.to_string()),
        from_addr: Some(from.1.to_string()),
        from_name: Some(from.0.to_string()),
        to_addrs: vec![SPIKE_EMAIL.to_string()],
        cc_addrs: vec![],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some(format!(
            "2026-09-{:02}T10:{:02}:00+00:00",
            1 + uid % 27,
            uid % 60
        )),
        snippet: Some(format!("{subject} — preview")),
        body_text: None,
        body_html: None,
        raw_headers: None,
        is_read: !uid.is_multiple_of(3),
        is_starred: uid.is_multiple_of(5),
        is_draft: false,
        has_attachments: false,
        keywords: vec![],
        size: 1024,
        downloaded_full: true,
    }
}

/// Seed (once) and return the fixture mails in uid order.
pub fn seed(db: &Db) -> Result<Vec<SeedMail>> {
    if mailcore::store::settings::get(db, SEED_KEY)?.is_some() {
        return existing(db);
    }
    let account_id = match accounts::list(db)?
        .iter()
        .find(|a| a.email_address == SPIKE_EMAIL)
    {
        Some(a) => a.id,
        None => accounts::create(
            db,
            &NewAccount {
                name: "Spike Fixture".to_string(),
                email_address: SPIKE_EMAIL.to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: SPIKE_EMAIL.into(),
                smtp_host: "smtp.example.com".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: SPIKE_EMAIL.into(),
                auth_vault_key: "vault-spike-fixture".into(),
                check_interval_secs: 300,
            },
        )?,
    };
    let inbox = folders::upsert(db, account_id, "INBOX", ".", FolderRole::Inbox)?;
    let archive = folders::upsert(db, account_id, "Archive", ".", FolderRole::Archive)?;

    let mut uid = 0u32;
    let mut out = Vec::new();
    let mut add = |db: &Db,
                   folder: i64,
                   slug: &'static str,
                   kind: &'static str,
                   m: NewMessage|
     -> Result<i64> {
        uid += 1;
        let mut m = m;
        m.uid = uid;
        m.folder_id = folder;
        let id = messages::upsert(db, &m)?;
        out.push(SeedMail { uid, slug, kind });
        Ok(id)
    };

    // 1. Plain text only.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Plain hello",
        ("Alice", "alice@example.com"),
    );
    m.body_text = Some(
        "Hello,\n\njust a plain-text mail with several paragraphs.\n\n\
         Second paragraph here.\n\n-- \nAlice"
            .to_string(),
    );
    add(db, inbox, "plain", "plain text", m)?;

    // 2. Simple HTML: marks + link.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Simple formatting",
        ("Bob", "bob@example.com"),
    );
    m.body_html = Some(
        "<p>Hello, this mail has <b>bold</b>, <i>italic</i>, <u>underline</u> \
         and <s>struck</s> text, plus <code>code()</code>.</p>\
         <p>See <a href=\"https://example.com/docs\">the docs</a> for details.</p>"
            .to_string(),
    );
    m.body_text = Some("Hello, formatted mail. See the docs.".to_string());
    add(db, inbox, "simple-html", "simple html", m)?;

    // 3. Headings + nested lists.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Headings and lists",
        ("Carol", "carol@example.com"),
    );
    m.body_html = Some(
        "<h1>Weekly update</h1><p>Three things:</p>\
         <ul><li>first<ul><li>nested one</li><li>nested two</li></ul></li>\
         <li>second</li><li>third</li></ul>\
         <h2>Next steps</h2><ol><li>do this</li><li>then that</li></ol>"
            .to_string(),
    );
    add(db, inbox, "headings-lists", "headings/lists", m)?;

    // 4. Nested blockquotes (a thread).
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Re: Re: lunch",
        ("Dan", "dan@example.com"),
    );
    m.body_html = Some(
        "<p>Thursday works for me.</p>\
         <blockquote><p>Wednesday then?</p>\
         <blockquote><p>Are we still on for lunch?</p></blockquote></blockquote>"
            .to_string(),
    );
    add(db, inbox, "quotes", "nested quotes", m)?;

    // 5. Pre + inline code.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Build log",
        ("erin@example.com", "erin@example.com"),
    );
    m.body_html = Some(
        "<p>The build failed like this:</p>\
         <pre>error[E0308]: mismatched types\n  --&gt; src/main.rs:12:5\n   | expected u32, found &amp;str</pre>\
         <p>Run <code>cargo test</code> to reproduce.</p>".to_string(),
    );
    add(db, inbox, "code", "pre/code", m)?;

    // 6. Simple table with header + bgcolor.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Q3 numbers",
        ("Finance", "finance@example.com"),
    );
    m.body_html = Some(
        "<p>Numbers at a glance:</p>\
         <table><tr><th>Quarter</th><th>Revenue</th></tr>\
         <tr bgcolor=\"#eef4ff\"><td>Q1</td><td>120k</td></tr>\
         <tr><td>Q2</td><td>135k</td></tr></table>"
            .to_string(),
    );
    add(db, inbox, "table-simple", "simple table", m)?;

    // 7. Newsletter: wrapper + content table, preheader, remote image, footer.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Example Weekly #42",
        ("Example Weekly", "news@example.com"),
    );
    m.body_html = Some(
        "<div style=\"display:none\">Preheader: the week in review, inside.</div>\
         <table width=\"600\"><tr><td>\
         <img src=\"https://example.com/banner.png\" alt=\"Weekly banner\">\
         <h1>The week in review</h1>\
         <table><tr><td valign=\"top\"><b>Story one.</b> Details follow here.</td>\
         <td valign=\"top\"><b>Story two.</b> More details here.</td></tr></table>\
         <p><a href=\"https://example.com/unsub\">Unsubscribe</a> | \
         <a href=\"https://example.com/webview\">View in browser</a></p>\
         </td></tr></table>"
            .to_string(),
    );
    add(db, inbox, "newsletter", "table newsletter", m)?;

    // 8. Dark-designed mail (dark bg + light text): Darkened paint path.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Night build report",
        ("CI", "ci@example.com"),
    );
    m.body_html = Some(
        "<div style=\"background-color:#16181d;color:#e8eaed\">\
         <h1 style=\"color:#ffffff\">Nightly: green</h1>\
         <p style=\"color:#bdc1c6\">412 tests passed, <b>0 failed</b>.</p>\
         <p><a href=\"https://example.com/builds/42\" style=\"color:#8ab4f8\">Open the report</a></p></div>"
            .to_string(),
    );
    add(db, inbox, "dark-mail", "dark designed", m)?;

    // 9. Remote images (had_remote banner case).
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Photos from the trip",
        ("Frank", "frank@example.com"),
    );
    m.body_html = Some(
        "<p>Some photos:</p>\
         <p><img src=\"https://example.com/p1.jpg\" alt=\"lake\"> \
         <img src=\"http://example.com/p2.jpg\" alt=\"hut\"></p>\
         <p>Wish you were here.</p>"
            .to_string(),
    );
    add(db, inbox, "remote-images", "remote images", m)?;

    // 10. Inline data: image (real 1px PNG).
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Logo draft",
        ("Gina", "gina@example.com"),
    );
    m.body_html = Some(format!(
        "<p>New logo:</p><p><img src=\"data:image/png;base64,{TINY_PNG}\" alt=\"draft logo\"></p>"
    ));
    add(db, inbox, "inline-data", "inline data image", m)?;

    // 11. Image-only remote body (blocked banner placeholder path).
    let mut m = msg(account_id, inbox, 0, "Poster", ("Hank", "hank@example.com"));
    m.body_html = Some("<img src=\"https://example.com/poster.png\" alt=\"poster\">".to_string());
    add(db, inbox, "image-only", "image-only remote", m)?;

    // 12. Link zoo: safe + unsafe schemes.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Links to check",
        ("Ivy", "ivy@example.com"),
    );
    m.body_html = Some(
        "<p><a href=\"https://example.com/ok\">https</a> \
         <a href=\"http://example.com/plain\">http</a> \
         <a href=\"mailto:ivy@example.com\">mail me</a> \
         <a href=\"javascript:alert(1)\">suspicious</a> \
         <a href=\"/relative/path\">relative</a></p>"
            .to_string(),
    );
    add(db, inbox, "links", "link safety", m)?;

    // 13. Alignment, rules, small/big/sub/sup/mark/del.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Bits and pieces",
        ("Jan", "jan@example.com"),
    );
    m.body_html = Some(
        "<center><h2>Centered title</h2></center><hr>\
         <p style=\"text-align:right\">right-aligned line</p>\
         <p><small>small</small> <big>big</big> H<sub>2</sub>O x<sup>2</sup> \
         <mark>highlight</mark> <del>removed</del> <ins>added</ins></p>"
            .to_string(),
    );
    add(db, inbox, "align-marks", "alignment/marks", m)?;

    // 14. display:none preheader must be skipped.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Visible content only",
        ("Karl", "karl@example.com"),
    );
    m.body_html = Some(
        "<span style=\"display:none\">SECRET-PREHEADER-STRING</span>\
         <div style=\"display:none\">more hidden text</div>\
         <p>The actual visible body.</p>"
            .to_string(),
    );
    add(db, inbox, "preheader", "hidden preheader", m)?;

    // 15. Floated image approximated.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Floated photo",
        ("Lena", "lena@example.com"),
    );
    m.body_html = Some(format!(
        "<p><img src=\"data:image/png;base64,{TINY_PNG}\" alt=\"portrait\" \
         style=\"float:left\">Text that wraps around the portrait in a browser; \
         the spike renders the image on its own line instead.</p>"
    ));
    add(db, inbox, "float", "floated image", m)?;

    // 16. Wide fixed-width table (fit_below narrow-fit path).
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Wide report",
        ("Mallory", "mallory@example.com"),
    );
    let mut wide = String::from("<table width=\"900\"><tr>");
    for i in 0..6 {
        wide.push_str(&format!("<td>column number {i} with padding text</td>"));
    }
    wide.push_str("</tr></table>");
    m.body_html = Some(format!("<p>Wide table:</p>{wide}"));
    add(db, inbox, "wide-table", "fixed-width table", m)?;

    // 17. Entities, nbsp, CJK.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Grüße aus München",
        ("Nina", "nina@example.com"),
    );
    m.body_html = Some(
        "<p>Gr&uuml;&szlig;e &amp; willkommen&nbsp;— together: A&nbsp;B.</p>\
         <p>Japanese: こんにちは, Chinese: 你好, quotes: &ldquo;hi&rdquo;.</p>"
            .to_string(),
    );
    add(db, inbox, "entities", "entities/CJK", m)?;

    // 18. No subject, address-only sender.
    let mut m = msg(
        account_id,
        inbox,
        0,
        "",
        ("noreply@example.com", "noreply@example.com"),
    );
    m.subject = Some(String::new());
    m.body_text = Some("Untitled mail with no subject line.".to_string());
    add(db, inbox, "no-subject", "empty subject", m)?;

    // 19. Attachments metadata (bytes only for one file).
    let mut m = msg(
        account_id,
        inbox,
        0,
        "Slides attached",
        ("Omar", "omar@example.com"),
    );
    m.body_text = Some("Slides attached — see files.".to_string());
    m.has_attachments = true;
    let id = add(db, inbox, "attachments", "attachments card", m)?;
    messages::add_attachment(
        db,
        id,
        &NewAttachment {
            filename: Some("slides.pdf".into()),
            mime_type: Some("application/pdf".into()),
            content_id: None,
            size: 184_320,
            data: None, // not downloaded yet: reader offers Open/Save
            is_inline: false,
        },
    )?;
    messages::add_attachment(
        db,
        id,
        &NewAttachment {
            filename: Some("notes.txt".into()),
            mime_type: Some("text/plain".into()),
            content_id: None,
            size: 412,
            data: Some(b"cached notes".to_vec()),
            is_inline: false,
        },
    )?;

    // 20. Reply with differing Reply-To + thread (long body for scroll).
    let mut m = msg(
        account_id,
        archive,
        0,
        "Re: venue booking",
        ("Priya", "priya@example.com"),
    );
    m.reply_to = Some("events@example.com".to_string());
    m.thread_id = Some("<thread-1@example.com>".to_string());
    let mut long = String::from("<p>Confirming the venue for Thursday.</p>");
    for i in 1..=30 {
        long.push_str(&format!(
            "<p>Detail paragraph {i}: tables, chairs, and timing.</p>"
        ));
    }
    m.body_html = Some(long);
    add(db, archive, "long-thread", "long scroll body", m)?;

    mailcore::store::settings::set(db, SEED_KEY, "1")?;
    Ok(out)
}

fn existing(db: &Db) -> Result<Vec<SeedMail>> {
    let account_id = accounts::list(db)?
        .into_iter()
        .find(|a| a.email_address == SPIKE_EMAIL)
        .map(|a| a.id)
        .unwrap_or(-1);
    let mut out = Vec::new();
    for folder in folders::list_by_account(db, account_id).unwrap_or_default() {
        for row in messages::list_compact_by_folder(db, folder.id).unwrap_or_default() {
            let subject = row.subject.clone().unwrap_or_default();
            out.push(SeedMail {
                uid: row.uid,
                slug: Box::leak(slugify(&subject).into_boxed_str()),
                kind: "reseeded",
            });
        }
    }
    out.sort_by_key(|m| m.uid);
    Ok(out)
}

fn slugify(subject: &str) -> String {
    let mut s: String = subject
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s.chars().take(28).collect()
    }
}

/// 1x1 red PNG, base64. Real bytes so the inline-image path decodes.
const TINY_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
