//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::db::Db;
use crate::error::Result;
use crate::html::{self, Sanitized};
use crate::models::FolderRole;
use crate::store::{accounts, folders, messages, settings};

/// `[{name, role, unread}]` ordered by path.
pub fn folders_json(db: &Db, account_id: i64) -> Result<String> {
    let counts = messages::counts_by_account(db, account_id)?;
    let mut arr = Vec::new();
    for f in folders::list_by_account(db, account_id)? {
        let c = counts.get(&f.id).copied().unwrap_or_default();
        let unread = if f.role == FolderRole::Trash {
            0
        } else {
            c.unread
        };
        arr.push(json!({
            "id": f.id,
            "name": f.path,
            "role": f.role.as_str(),
            "unread": unread,
            // Sidebar visibility toggle + cached total (see Folders dialog),
            // plus the hierarchy delimiter so the move picker can indent
            // subfolders (depth = segments - 1).
            "subscribed": f.subscribed,
            "count": c.total,
            "delimiter": f.delimiter,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// `[{id, name, email, from_name, imap_host, smtp_host}]` for the account manager.
pub fn accounts_json(db: &Db) -> Result<String> {
    let mut arr = Vec::new();
    for a in accounts::list(db)? {
        arr.push(json!({
            "id": a.id,
            "name": a.name,
            "email": a.email_address,
            "from_name": a.from_name,
            "imap_host": a.imap_host,
            "imap_port": a.imap_port,
            "imap_sec": a.imap_security,
            "imap_user": a.imap_username,
            "smtp_host": a.smtp_host,
            "smtp_port": a.smtp_port,
            "smtp_sec": a.smtp_security,
            "smtp_user": a.smtp_username,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// A list/reader timestamp: the text to show, plus a key naming the cases
/// that are a *word* rather than a number.
///
/// mailcore has no translation catalogue and must stay Qt-free, so it cannot
/// produce "Yesterday" in the user's language. It names the case instead and
/// QML supplies the word (`text` carries untranslated English as the
/// fallback, so a caller that ignores `key` still shows something sensible).
struct ShortDate {
    text: String,
    /// `"yesterday"`, or `""` when `text` is already a plain time or date.
    key: &'static str,
}

/// Compact human date in the **viewer's** timezone: `09:12` (today),
/// yesterday, else `2026-09-07`.
///
/// Everything is converted to local time first. Formatting the parsed value
/// directly keeps whatever offset the sender wrote (usually `Z`), so a mail
/// that arrived at 11:12 local showed 09:12, and "today"/"yesterday" flipped
/// at UTC midnight rather than the user's.
fn short_date(rfc3339: Option<&str>) -> ShortDate {
    let plain = |text: String| ShortDate { text, key: "" };
    let raw = rfc3339.unwrap_or("");
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) else {
        return plain(raw.to_string());
    };
    let local = dt.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    let date = local.date_naive();
    if date == today {
        plain(local.format("%H:%M").to_string())
    } else if date == today.pred_opt().unwrap_or(today) {
        ShortDate {
            text: "Yesterday".to_string(),
            key: "yesterday",
        }
    } else {
        plain(local.format("%Y-%m-%d").to_string())
    }
}

/// Sanitized bodies for one message: `(safe_html, had_remote, is_html, plain)`.
///
/// Shared by the list feed and the on-demand `message_html` path, so the
/// banner ("Show once") and the feed can never disagree.
pub fn sanitized_bodies(
    body_html: Option<&str>,
    body_text: Option<&str>,
    allow_remote: bool,
) -> (String, bool, bool, String) {
    let raw_html = body_html.unwrap_or("");
    let raw_text = body_text.unwrap_or("");
    // A stored `body_text` may itself hold HTML source (legacy
    // plain-only sends of composer rich text). Prefer a real html part,
    // else upgrade text that looks like HTML.
    let candidate_html = if !raw_html.trim().is_empty() {
        raw_html
    } else if html::looks_like_html(raw_text) {
        raw_text
    } else {
        ""
    };
    let Sanitized {
        html: safe_html,
        had_remote,
    } = html::sanitize(candidate_html, allow_remote);
    // `had_remote` only matters when we actually had an html body.
    let is_html = !safe_html.trim().is_empty() || (!candidate_html.trim().is_empty() && had_remote);
    let plain = if raw_text.trim().is_empty() && !candidate_html.trim().is_empty() {
        html::html_to_text(candidate_html)
    } else {
        raw_text.to_string()
    };
    // When remote images were stripped the sanitized html can be empty
    // (image-only newsletter) — still route to WebEngine so the banner
    // ("images blocked") shows instead of raw-tag text.
    let body_html = if is_html && safe_html.trim().is_empty() {
        // Keeps the html branch alive, and says which of the two it is
        // rather than blaming blocked images for an empty body.
        if had_remote {
            "<p>[images blocked — choose Show images]</p>".to_string()
        } else {
            "<p>[no displayable content]</p>".to_string()
        }
    } else {
        safe_html
    };
    (body_html, had_remote, is_html, plain)
}

/// Sanitized HTML for one message, re-sanitized on demand.
///
/// The list feed strips remote images when the setting is off, so "Show
/// once" cannot reuse `body_html` — the URLs are already gone. This
/// re-sanitizes the stored raw body with `allow_remote=true` for that one
/// view. Inline `cid:`/`data:` images are always kept (they are part of the
/// mail, not tracking pixels).
pub fn message_html(db: &Db, folder_id: i64, uid: u32, allow_remote: bool) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let (body_html, _, _, _) =
        sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    Ok(body_html)
}

/// List snippets are single-line by contract: the FTS `snippet()` context
/// keeps the body's line breaks, which would paint past the fixed row
/// height and overlap the next row. Collapse all whitespace runs.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rows for the mailbox list: `[{uid, subject, from, date, date_key, snippet,
/// unread, starred, has_attachments}]`, in the user's sort order (see
/// `message_sort_field` / `message_sort_desc` — Date means newest IMAP UID
/// first).
///
/// Bodies and attachment records are deliberately absent: they are loaded
/// only for the selected message, via [`message_json`]. Serializing and
/// sanitizing them for every row made a cached 200-message folder switch
/// noticeably stall the UI.
///
/// `limit`/`offset` page the local cache in sort order. The list grows via
/// "load older": the bridge backfills the next server batch into SQLite
/// first (`sync_older`), then raises `limit` so the new rows appear.
///
/// `date_key` is `"yesterday"` when `date` is that word and `""` otherwise —
/// see [`ShortDate`] for why the word is QML's to supply.
pub fn messages_list_json_paged(
    db: &Db,
    folder_id: i64,
    limit: u64,
    offset: u64,
) -> Result<String> {
    let field = settings::get_sort_field(db);
    let descending = settings::get_sort_descending(db);
    let rows =
        messages::list_compact_by_folder_sorted(db, folder_id, limit, offset, &field, descending)?;
    let folder = folders::get(db, folder_id).ok();
    let is_trash = folder.as_ref().is_some_and(|f| f.role == FolderRole::Trash);
    Ok(serde_json::to_string(
        &rows
            .into_iter()
            .map(|m| {
                let date = short_date(m.date.as_deref());
                json!({
                    "uid": m.uid,
                    "subject": m.subject.unwrap_or_else(|| "(no subject)".to_string()),
                    "from": m.from_addr.unwrap_or_else(|| "?".to_string()),
                    "date": date.text,
                    "date_key": date.key,
                    "snippet": m.snippet.unwrap_or_default(),
                    "unread": if is_trash { false } else { !m.is_read },
                    "starred": m.is_starred,
                    "has_attachments": m.has_attachments,
                })
            })
            .collect::<Vec<_>>(),
    )?)
}

/// Full reader payload for one message, produced on demand after selection.
pub fn message_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let allow_remote = settings::get_bool(db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false);
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let folder = folders::get(db, folder_id).ok();
    let is_trash = folder.as_ref().is_some_and(|f| f.role == FolderRole::Trash);
    let (body_html, had_remote, is_html, plain) =
        sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    let legacy_body = if is_html {
        body_html.clone()
    } else {
        plain.clone()
    };
    let files: Vec<serde_json::Value> = messages::list_attachments(db, m.id)
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id, "filename": a.filename, "mime_type": a.mime_type,
                "size": a.size, "content_id": a.content_id, "is_inline": a.is_inline,
            })
        })
        .collect();
    let date = short_date(m.date.as_deref());
    Ok(serde_json::to_string(&json!({
        "uid": m.uid,
        "subject": m.subject.as_deref().unwrap_or("(no subject)"),
        "from": m.from_addr.as_deref().unwrap_or("?"),
        "reply_to": m.reply_to.as_deref().unwrap_or(""),
        "date": date.text,
        "date_key": date.key,
        "snippet": m.snippet.as_deref().unwrap_or(""),
        "unread": if is_trash { false } else { !m.is_read }, "starred": m.is_starred,
        "has_attachments": m.has_attachments || !files.is_empty(),
        "attachments": files, "body_text": plain, "body_html": body_html,
        "is_html": is_html, "has_remote_images": had_remote && is_html,
        "body": legacy_body,
    }))?)
}

/// Attachment metadata for one message (`[{id, filename, mime_type, size,
/// content_id, is_inline}]`, no bytes). Used by the reader pane and the
/// save dialog; bytes leave Rust only via `save_attachment_to_path`.
pub fn attachments_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let files: Vec<serde_json::Value> = messages::list_attachments(db, m.id)
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id,
                "filename": a.filename,
                "mime_type": a.mime_type,
                "size": a.size,
                "content_id": a.content_id,
                "is_inline": a.is_inline,
            })
        })
        .collect();
    Ok(serde_json::to_string(&files)?)
}

/// Header details for one message (the reader's "Headers" dialog):
/// `{from, to, cc, date, subject, message_id, reply_to}`. `date` is the full
/// local timestamp (`2026-09-12 13:50`), falling back to the stored raw value
/// when unparseable. Empty/absent fields become `""`/`[]` for QML.
pub fn headers_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let date = match m.date.as_deref() {
        Some(raw) => match chrono::DateTime::parse_from_rfc3339(raw) {
            Ok(dt) => dt
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            Err(_) => raw.to_string(),
        },
        None => String::new(),
    };
    Ok(serde_json::to_string(&json!({
        "from": rfc_header(&m.raw_headers, "From").unwrap_or_else(|| m.from_addr.unwrap_or_default()),
        "to": rfc_header(&m.raw_headers, "To").unwrap_or_else(|| m.to_addrs.join(", ")),
        "cc": rfc_header(&m.raw_headers, "Cc").unwrap_or_else(|| m.cc_addrs.join(", ")),
        "date": date,
        "subject": m.subject.unwrap_or_default(),
        "message_id": m.message_id_header.unwrap_or_default(),
        "reply_to": m.reply_to.unwrap_or_default(),
        "raw": m.raw_headers.unwrap_or_default(),
    }))?)
}

/// Extract and unfold one RFC 5322 header from the stored header block.
/// Keeps display names and group syntax (`freunde:;`) lost by address parsing.
fn rfc_header(raw: &Option<String>, wanted: &str) -> Option<String> {
    let raw = raw.as_deref()?;
    let mut value: Option<String> = None;
    for line in raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(v) = value.as_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, v)) = line.split_once(':') {
            if name.eq_ignore_ascii_case(wanted) {
                value = Some(v.trim().to_string());
                continue;
            }
        }
        if value.is_some() {
            break;
        }
    }
    value
}

/// Account-wide FTS search rows for the search UI: `[{uid, folder_id,
/// folder, subject, from, date, snippet, unread, starred,
/// has_attachments}]` in FTS rank order. `folder` scopes the search to one
/// folder path (empty = whole account). `snippet` is plain match context
/// (the empty-string `snippet()` markers produce it tag-free — the list
/// renders plain rows). Blank or operator-only queries yield `[]`, never an
/// error.
pub fn search_json(
    db: &Db,
    account_id: i64,
    query: &str,
    limit: u64,
    folder: &str,
) -> Result<String> {
    let Some(match_query) = crate::search::escape_fts_query(query) else {
        return Ok("[]".to_string());
    };
    let mut stmt = db.conn().prepare(
        "select m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
                snippet(messages_fts, 2, '', '', '…', 12),
                m.is_read, m.is_starred, m.has_attachments
         from messages_fts
         join messages m on m.id = messages_fts.rowid
         join folders f on f.id = m.folder_id
         where messages_fts match ?1 and m.account_id = ?2
           and (?4 = '' or f.path = ?4)
         order by rank limit ?3",
    )?;
    let mut arr = Vec::new();
    let rows = stmt.query_map(
        rusqlite::params![match_query, account_id, limit as i64, folder],
        |row| {
            let date = short_date(row.get::<_, Option<String>>(5)?.as_deref());
            Ok(json!({
                "uid": row.get::<_, u32>(0)?,
                "folder_id": row.get::<_, i64>(1)?,
                "folder": row.get::<_, String>(2)?,
                "subject": row.get::<_, Option<String>>(3)?.unwrap_or_else(|| "(no subject)".to_string()),
                "from": row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "?".to_string()),
                "date": date.text,
                "date_key": date.key,
                "snippet": one_line(&row.get::<_, String>(6)?),
                "unread": row.get::<_, i64>(7)? == 0,
                "starred": row.get::<_, i64>(8)? != 0,
                "has_attachments": row.get::<_, i64>(9)? != 0,
            }))
        },
    )?;
    for row in rows {
        arr.push(row?);
    }
    Ok(serde_json::to_string(&arr)?)
}

#[cfg(test)]
mod tests;
