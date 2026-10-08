//! New, reply, reply-all and forward drafts, built once for both composers:
//! recipients, the `Re:`/`Fwd:` subject, the attribution line, the quoted
//! original and the signature with its placement. A frontend only puts the
//! fields into its editor.
//!
//! The quote is HTML in the format the original arrived in: an HTML mail is
//! quoted as its sanitized body inside a `<blockquote>`, a plain mail as
//! `> ` citations, which keeps an Auto send text/plain unless the user adds
//! formatting. `body_html` is the whole editable body for a rich editor
//! (Qt); a plain-text editor (Flutter) puts `signature_text` in its box and
//! carries `quote_html` alongside, appending it on send.

use serde::Serialize;

use super::from::sender_parts;
use super::reply::{bare, reply_address, ReplyAddress};
use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::html::{escape_text, html_to_text};
use crate::store::{accounts, messages, settings};

/// Which answer to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerMode {
    Reply,
    ReplyAll,
    Forward,
    /// Edit and send again the sent original a bounce reports on, to the
    /// addresses delivery failed for.
    Resend,
}

impl AnswerMode {
    /// `"reply"`, `"reply_all"` or `"forward"`, as the adapters pass it.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "reply" => Some(Self::Reply),
            "reply_all" => Some(Self::ReplyAll),
            "forward" => Some(Self::Forward),
            "resend" => Some(Self::Resend),
            _ => None,
        }
    }
}

/// The message being answered, bodies already sanitized.
#[derive(Debug, Clone, Default)]
pub struct AnswerSource {
    pub from: String,
    pub from_name: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    /// Envelope recipients the receiving server recorded (`X-Original-To`,
    /// `Delivered-To`, `Envelope-To`): who the mail reached when To and Cc
    /// do not say, e.g. a Bcc.
    pub envelope_to: Vec<String>,
    pub reply_to: String,
    pub subject: String,
    /// Human date for the attribution line (`2026-09-12 13:50`).
    pub date: String,
    pub is_html: bool,
    pub body_html: String,
    pub body_text: String,
}

/// User settings that shape the draft.
#[derive(Debug, Clone, Default)]
pub struct AnswerOptions {
    /// The answering account's address, left out of reply-all recipients.
    pub own_address: String,
    /// Signature text, `None` when off or blank.
    pub signature: Option<String>,
    /// Bottom-posting: the reply goes below the quote.
    pub reply_below_quote: bool,
}

/// The prepared draft. Recipients are comma-joined like the To field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnswerDraft {
    /// The address to send as; `""` keeps the account's. A reply answers
    /// from the address on the account's domain the mail was sent to.
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    /// Set when the reply goes to a Reply-To other than the sender: the
    /// composer says so (`notice_addr`, not `notice_sender`).
    pub notice_addr: String,
    pub notice_sender: String,
    /// The sentence both composers show while To still holds `notice_addr`;
    /// `""` when replies go to the sender.
    pub notice: String,
    /// `<p>-- <br>…</p>` and `-- \n…`, both `""` without a signature.
    pub signature_html: String,
    pub signature_text: String,
    /// Attribution or forward header plus the quoted original.
    pub quote_html: String,
    /// The quote sits above the user's text (bottom-posting).
    pub quote_first: bool,
    /// Whole editable body for a rich editor: text slot, signature and quote
    /// in their placement.
    pub body_html: String,
}

/// A new, empty mail: only the signature.
pub fn blank_draft(opts: &AnswerOptions) -> AnswerDraft {
    let (signature_html, signature_text) = signature(opts);
    AnswerDraft {
        from: String::new(),
        to: String::new(),
        cc: String::new(),
        subject: String::new(),
        notice_addr: String::new(),
        notice_sender: String::new(),
        notice: String::new(),
        body_html: signature_html.clone(),
        signature_html,
        signature_text,
        quote_html: String::new(),
        quote_first: false,
    }
}

/// Builds the draft for `src`. `Resend` needs the bounce, not a source:
/// it is built by [`resend_draft`] and reads as a reply here.
pub fn answer_draft(src: &AnswerSource, mode: AnswerMode, opts: &AnswerOptions) -> AnswerDraft {
    let (signature_html, signature_text) = signature(opts);
    let sender = if src.from_name.trim().is_empty() {
        src.from.clone()
    } else {
        format!("{} <{}>", src.from_name.trim(), src.from)
    };

    if mode == AnswerMode::Forward {
        let header = format!(
            "— Forwarded message —\nFrom: {sender}\nDate: {}\nSubject: {}",
            src.date, src.subject
        );
        let quote_html = quote(&header, src);
        return AnswerDraft {
            from: String::new(),
            to: String::new(),
            cc: String::new(),
            subject: prefixed(&src.subject, "Fwd:", &["fwd:", "fw:", "wg:"]),
            notice_addr: String::new(),
            notice_sender: String::new(),
            notice: String::new(),
            body_html: format!("{signature_html}<p></p>{quote_html}"),
            signature_html,
            signature_text,
            quote_html,
            quote_first: false,
        };
    }

    let own = opts.own_address.trim();
    let mut from = String::new();
    let (reply, cc) = match own_mail_recipients(src, own) {
        // Our own mail (Sent): answer the people it went to, not ourselves.
        Some(to) => {
            let cc = if mode == AnswerMode::ReplyAll {
                let mut seen = vec![own.to_lowercase()];
                seen.extend(to.iter().map(|a| bare(a).to_lowercase()));
                unique_addrs(&src.cc, &mut seen).join(", ")
            } else {
                String::new()
            };
            let target = to.join(", ");
            (
                ReplyAddress {
                    target,
                    differs: false,
                },
                cc,
            )
        }
        None => {
            from = reply_from(src, own);
            let reply = reply_address(&src.from, &src.reply_to);
            let cc = if mode == AnswerMode::ReplyAll {
                reply_all_cc(src, &reply.target, &[own, &from])
            } else {
                String::new()
            };
            (reply, cc)
        }
    };
    let quote_html = quote(&format!("On {}, {sender} wrote:", src.date), src);
    let body_html = if opts.reply_below_quote {
        format!("{quote_html}<p></p>{signature_html}")
    } else {
        format!("<p></p>{signature_html}{quote_html}")
    };
    AnswerDraft {
        notice_addr: if reply.differs {
            reply.target.clone()
        } else {
            String::new()
        },
        notice_sender: if reply.differs {
            src.from.clone()
        } else {
            String::new()
        },
        notice: if reply.differs {
            format!(
                "Replies to this mail go to {} — not to the sender ({}).",
                reply.target, src.from
            )
        } else {
            String::new()
        },
        from,
        to: reply.target,
        cc,
        subject: prefixed(&src.subject, "Re:", &["re:", "aw:"]),
        signature_html,
        signature_text,
        quote_html,
        quote_first: opts.reply_below_quote,
        body_html,
    }
}

/// The reader payload's message as an answer draft, as JSON. `mode` is
/// `"reply"`, `"reply_all"`, `"forward"` or `"resend"` (the message is then
/// a bounce, see [`resend_draft`]).
pub fn answer_draft_json(db: &Db, folder_id: i64, uid: u32, mode: &str) -> Result<String> {
    let mode = AnswerMode::parse(mode)
        .ok_or_else(|| StoreError::InvalidInput(format!("unknown answer mode {mode:?}")))?;
    if mode == AnswerMode::Resend {
        return Ok(serde_json::to_string(&resend_draft(db, folder_id, uid)?)?);
    }
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let allow_remote = settings::get_bool(db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false);
    let (body_html, _, is_html, plain) =
        crate::feed::sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    let body_html = if is_html {
        crate::feed::with_inline_images(db, m.id, &body_html).0
    } else {
        body_html
    };
    let src = AnswerSource {
        from: m.from_addr.unwrap_or_default(),
        from_name: m.from_name.unwrap_or_default(),
        to: m.to_addrs,
        cc: m.cc_addrs,
        envelope_to: ["X-Original-To", "Delivered-To", "Envelope-To"]
            .iter()
            .flat_map(|h| crate::feed::header_values(m.raw_headers.as_deref(), h))
            .collect(),
        reply_to: m.reply_to.unwrap_or_default(),
        subject: m.subject.unwrap_or_default(),
        date: crate::feed::full_local_date(m.date.as_deref()),
        is_html,
        body_html,
        body_text: plain,
    };
    let opts = AnswerOptions {
        own_address: accounts::get(db, m.account_id)
            .map(|a| a.email_address)
            .unwrap_or_default(),
        ..stored_options(db)
    };
    Ok(serde_json::to_string(&answer_draft(&src, mode, &opts))?)
}

/// The sent original of the bounce at `(folder_id, uid)` as a new draft to
/// edit and send again: addressed to the recipients delivery failed for
/// (the original's To when the report names none), same subject, its body
/// as is — signature included, nothing quoted. Its files come separately
/// (`compose::stage_resend_files`).
pub fn resend_draft(db: &Db, folder_id: i64, uid: u32) -> Result<AnswerDraft> {
    let bounce = crate::report::bounce(db, folder_id, uid).map_err(StoreError::InvalidInput)?;
    let m = bounce.original;
    let to = if bounce.failed.is_empty() {
        m.to_addrs.join(", ")
    } else {
        bounce.failed.join(", ")
    };
    Ok(AnswerDraft {
        // Sent again as it was sent the first time.
        from: m.from_addr.clone().unwrap_or_default(),
        to,
        cc: String::new(),
        subject: m.subject.clone().unwrap_or_default(),
        notice_addr: String::new(),
        notice_sender: String::new(),
        notice: String::new(),
        signature_html: String::new(),
        signature_text: String::new(),
        quote_html: String::new(),
        quote_first: false,
        body_html: super::drafts::draft_editor_html(db, &m),
    })
}

/// [`blank_draft`] with the stored settings, as JSON.
pub fn blank_draft_json(db: &Db) -> Result<String> {
    Ok(serde_json::to_string(&blank_draft(&stored_options(db)))?)
}

fn stored_options(db: &Db) -> AnswerOptions {
    AnswerOptions {
        own_address: String::new(),
        signature: settings::get_bool(db, settings::SIGNATURE_ENABLED)
            .unwrap_or(false)
            .then(|| settings::get(db, settings::SIGNATURE_TEXT).ok().flatten())
            .flatten(),
        reply_below_quote: settings::get_bool(db, settings::REPLY_BELOW_QUOTE).unwrap_or(false),
    }
}

/// `<p>-- <br>…</p>` and `-- \n…` with blank edge lines trimmed; both `""`
/// when off or blank.
fn signature(opts: &AnswerOptions) -> (String, String) {
    let lines = opts
        .signature
        .as_deref()
        .map(trimmed_lines)
        .unwrap_or_default();
    if lines.is_empty() {
        return (String::new(), String::new());
    }
    (
        format!("<p>-- <br>{}</p>", escape_lines(&lines).join("<br>")),
        format!("-- \n{}", lines.join("\n")),
    )
}

/// `header` as its own paragraph, then the original: an HTML body in a
/// `<blockquote>`, plain text as `> ` citations.
fn quote(header: &str, src: &AnswerSource) -> String {
    let head = format!(
        "<p>{}</p>",
        escape_lines(&header.lines().collect::<Vec<_>>()).join("<br>")
    );
    if src.is_html && !src.body_html.trim().is_empty() {
        return format!("{head}<blockquote>{}</blockquote>", src.body_html);
    }
    let text = if src.body_text.trim().is_empty() {
        html_to_text(&src.body_html)
    } else {
        src.body_text.clone()
    };
    let cited: Vec<String> = text
        .trim_end()
        .lines()
        .map(|l| format!("&gt; {}", escape_text(l)))
        .collect();
    format!("{head}<p>{}</p>", cited.join("<br>"))
}

/// The sender (when Reply-To sends the reply elsewhere) and everyone the
/// original went to, except our addresses and the reply target, once each.
fn reply_all_cc(src: &AnswerSource, target: &str, ours: &[&str]) -> String {
    let mut seen = vec![bare(target).to_lowercase()];
    seen.extend(ours.iter().map(|a| a.trim().to_lowercase()));
    let all: Vec<String> = std::iter::once(&src.from)
        .chain(&src.to)
        .chain(&src.cc)
        .cloned()
        .collect();
    unique_addrs(&all, &mut seen).join(", ")
}

/// The address on the account's domain that `src` was delivered to, for
/// the reply to come from; `""` keeps the account address. Only the
/// envelope counts (`X-Original-To` names the alias that the mailbox's
/// `Delivered-To` hides): To and Cc also name colleagues and lists on a
/// shared domain, and answering as one of them must never happen by default.
fn reply_from(src: &AnswerSource, own: &str) -> String {
    let domain = sender_parts(own).domain.to_lowercase();
    let own = own.to_lowercase();
    if domain.is_empty() {
        return String::new();
    }
    src.envelope_to
        .iter()
        .map(|a| bare(a).to_lowercase())
        .find(|a| a.len() > domain.len() && a.ends_with(&domain) && *a != own)
        .unwrap_or_default()
}

/// When `src` is mail we sent ourselves (no Reply-To elsewhere), the
/// recipients a reply goes to: its To list without us. `None` for anyone
/// else's mail, or when nobody but us is in To (then we answer ourselves).
fn own_mail_recipients(src: &AnswerSource, own: &str) -> Option<Vec<String>> {
    let reply_to = bare(&src.reply_to);
    let ours = |a: &str| !own.is_empty() && a.eq_ignore_ascii_case(own);
    if !ours(bare(&src.from)) || !(reply_to.is_empty() || ours(reply_to)) {
        return None;
    }
    let to = unique_addrs(&src.to, &mut vec![own.to_lowercase()]);
    (!to.is_empty()).then_some(to)
}

/// `addrs` once each (by bare address, case-insensitively), skipping
/// blanks and everything already in `seen`, which grows as it goes.
fn unique_addrs(addrs: &[String], seen: &mut Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for addr in addrs {
        let key = bare(addr).to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(addr.trim().to_string());
    }
    out
}

/// `prefix subject`, unless the subject already starts with one of `known`
/// (compared case-insensitively; `aw:`/`wg:` are the German forms).
fn prefixed(subject: &str, prefix: &str, known: &[&str]) -> String {
    let s = subject.trim();
    let lower = s.to_lowercase();
    if known.iter().any(|k| lower.starts_with(k)) {
        s.to_string()
    } else {
        format!("{prefix} {s}").trim_end().to_string()
    }
}

fn trimmed_lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

fn escape_lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| escape_text(l)).collect()
}

#[cfg(test)]
mod tests;
