//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::badge::sender_badge;
use crate::db::Db;
use crate::error::Result;
use crate::html::{self, Sanitized};
use crate::models::FolderRole;
use crate::store::{accounts, folders, messages, settings};

/// What a folder's "Show older" row says and offers, from the cached count
/// and the count the server last reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OlderState {
    /// The server never reported a count: offer to ask it.
    Unchecked,
    /// The server holds more than the cache: offer to load older mail.
    Partial,
    /// Nothing here, on either side.
    Empty,
    /// Everything the server has is cached.
    Complete,
}

impl OlderState {
    /// Whether asking the server could bring more mail.
    #[must_use]
    pub fn can_load(self) -> bool {
        matches!(self, OlderState::Unchecked | OlderState::Partial)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OlderState::Unchecked => "unchecked",
            OlderState::Partial => "partial",
            OlderState::Empty => "empty",
            OlderState::Complete => "complete",
        }
    }
}

/// The "Show older" state for `cached` local rows against the server's
/// last count (`None`: never reported).
#[must_use]
pub fn older_state(cached: u64, server: Option<u64>) -> OlderState {
    match server {
        None => OlderState::Unchecked,
        Some(s) if s > cached => OlderState::Partial,
        Some(_) if cached == 0 => OlderState::Empty,
        Some(_) => OlderState::Complete,
    }
}

/// `[{id, name, role, unread, subscribed, count, delimiter, depth, leaf,
/// server_total, older, can_load_older, delete_is_permanent}]` ordered by
/// path. `depth`/`leaf` derive from the path and delimiter here so the move
/// picker and the sidebar indent and label subfolders identically instead of
/// each splitting the path itself.
pub fn folders_json(db: &Db, account_id: i64) -> Result<String> {
    let counts = messages::counts_by_account(db, account_id)?;
    let list = folders::list_by_account(db, account_id)?;
    let mut arr = Vec::new();
    for f in &list {
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
            // plus the hierarchy fields so the move picker can indent
            // subfolders (depth = segments - 1) and show the short name.
            "subscribed": f.subscribed,
            "count": c.total,
            "delimiter": f.delimiter,
            "depth": folder_depth(&f.path, &f.delimiter),
            "leaf": folder_leaf(&f.path, &f.delimiter),
            // `-1`: the server never reported a count.
            "server_total": f.server_total.map_or(-1, |s| s as i64),
            "older": older_state(c.total, f.server_total).as_str(),
            "can_load_older": older_state(c.total, f.server_total).can_load(),
            "delete_is_permanent": crate::undo::delete_is_permanent(f, &list),
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Hierarchy depth of an IMAP path: segments minus one (`INBOX` → 0,
/// `Work/Client` → 1). An empty delimiter means no hierarchy.
pub fn folder_depth(path: &str, delimiter: &str) -> u64 {
    if delimiter.is_empty() {
        0
    } else {
        path.split(delimiter).count().saturating_sub(1) as u64
    }
}

/// The short name of an IMAP path: the last segment (`Work/Client` →
/// `Client`). An empty delimiter leaves the path whole.
pub fn folder_leaf(path: &str, delimiter: &str) -> String {
    if delimiter.is_empty() {
        path.to_string()
    } else {
        path.rsplit(delimiter).next().unwrap_or(path).to_string()
    }
}

/// `[{id, name, email, from_name, imap_host, smtp_host, …}]` for the account
/// manager, plus the account's own sender badge (see [`crate::badge`]).
pub fn accounts_json(db: &Db) -> Result<String> {
    let mut arr = Vec::new();
    for a in accounts::list(db)? {
        let badge = sender_badge(&a.from_name, &a.email_address);
        let mut row = json!({
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
        });
        badge.extend(&mut row);
        arr.push(row);
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
pub(crate) struct ShortDate {
    pub(crate) text: String,
    /// `"yesterday"`, or `""` when `text` is already a plain time or date.
    pub(crate) key: &'static str,
}

/// Compact human date in the **viewer's** timezone: `09:12` (today),
/// yesterday, else `2026-09-07`.
///
/// Everything is converted to local time first. Formatting the parsed value
/// directly keeps whatever offset the sender wrote (usually `Z`), so a mail
/// that arrived at 11:12 local showed 09:12, and "today"/"yesterday" flipped
/// at UTC midnight rather than the user's.
pub(crate) fn short_date(rfc3339: Option<&str>) -> ShortDate {
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
    Ok(with_inline_images(db, m.id, &body_html).0)
}

/// Resolve `cid:` images from the message's stored parts. Reads SQLite
/// only — a part without stored bytes turns into its alt text and is
/// counted, so the reader can offer an explicit download.
pub(crate) fn with_inline_images(db: &Db, message_id: i64, body_html: &str) -> (String, usize) {
    let images = messages::inline_images(db, message_id).unwrap_or_else(|e| {
        log::warn!("inline images for message {message_id}: {e}");
        Vec::new()
    });
    html::inline_cid_images(body_html, &images)
}

/// List snippets are single-line by contract: the FTS `snippet()` context
/// keeps the body's line breaks, which would paint past the fixed row
/// height and overlap the next row. Collapse all whitespace runs.
pub(crate) fn one_line(s: &str) -> String {
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
                let from = m.from_addr.unwrap_or_else(|| "?".to_string());
                let from_name = m.from_name.unwrap_or_default();
                let badge = sender_badge(&from_name, &from);
                let mut row = json!({
                    "uid": m.uid,
                    "subject": m.subject.unwrap_or_else(|| "(no subject)".to_string()),
                    "from": from,
                    "from_name": from_name,
                    "date": date.text,
                    "date_key": date.key,
                    // Raw UTC timestamp for the list date quick-filter
                    // (`search::date_passes`); `date` above is display text.
                    "date_raw": m.date,
                    "snippet": m.snippet.unwrap_or_default(),
                    "unread": if is_trash { false } else { !m.is_read },
                    "starred": m.is_starred,
                    "has_attachments": m.has_attachments,
                });
                badge.extend(&mut row);
                row
            })
            .collect::<Vec<_>>(),
    )?)
}

/// Full reader payload for one message, produced on demand after selection.
/// Carries the sender as parsed parts (`from` address, `from_name`), its
/// badge, and where a reply goes (`reply_target`, `reply_to_differs`, see
/// [`crate::compose::reply_address`]) so no frontend re-parses headers.
pub fn message_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let allow_remote = settings::get_bool(db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false);
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let folder = folders::get(db, folder_id).ok();
    let is_trash = folder.as_ref().is_some_and(|f| f.role == FolderRole::Trash);
    let (body_html, had_remote, is_html, plain) =
        sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    let (body_html, missing_inline) = if is_html {
        with_inline_images(db, m.id, &body_html)
    } else {
        (body_html, 0)
    };
    let legacy_body = if is_html {
        body_html.clone()
    } else {
        plain.clone()
    };
    let files: Vec<serde_json::Value> = listed_attachments(db, m.id, m.body_html.as_deref())
        .iter()
        .map(attachment_row)
        .collect();
    let date = short_date(m.date.as_deref());
    let from = m.from_addr.as_deref().unwrap_or("?");
    let from_name = m.from_name.as_deref().unwrap_or("");
    let reply = crate::compose::reply_address(from, m.reply_to.as_deref().unwrap_or(""));
    let event = find_calendar_event(db, &m);
    let mut out = json!({
        "uid": m.uid,
        "subject": m.subject.as_deref().unwrap_or("(no subject)"),
        "from": from,
        "from_name": from_name,
        "reply_to": m.reply_to.as_deref().unwrap_or(""),
        "reply_target": reply.target,
        "reply_to_differs": reply.differs,
        "date": date.text,
        "date_key": date.key,
        "snippet": m.snippet.as_deref().unwrap_or(""),
        "unread": if is_trash { false } else { !m.is_read }, "starred": m.is_starred,
        "has_attachments": !files.is_empty(),
        "attachments": files, "body_text": plain, "body_html": body_html,
        "is_html": is_html, "has_remote_images": had_remote && is_html,
        "missing_inline_images": missing_inline,
        "html_colored": is_html && html::has_own_colors(&body_html),
        "body": legacy_body,
        "event": event,
    });
    sender_badge(from_name, from).extend(&mut out);
    Ok(serde_json::to_string(&out)?)
}

/// Try to extract a calendar event from message attachments or body.
pub fn find_calendar_event(
    db: &Db,
    message: &crate::models::Message,
) -> Option<crate::calendar::CalendarEvent> {
    if let Ok(attachments) = messages::list_attachments(db, message.id) {
        for att in attachments {
            let is_ics = att
                .filename
                .as_deref()
                .is_some_and(|f| f.to_ascii_lowercase().ends_with(".ics"));
            let is_cal = att.mime_type.as_deref().is_some_and(|m| {
                let m = m.to_ascii_lowercase();
                m == "text/calendar" || m == "application/ics"
            });
            if is_ics || is_cal {
                if let Ok(full) = messages::get_attachment(db, att.id) {
                    if let Some(bytes) = full.data.as_deref() {
                        if let Some(mut event) = crate::calendar::parse_ics_bytes(bytes) {
                            event.set_attachment(&att);
                            return Some(event);
                        }
                    }
                }
            }
        }
    }

    if let Some(text) = message.body_text.as_deref() {
        if text.contains("BEGIN:VEVENT") {
            if let Some(event) = crate::calendar::parse_ics(text) {
                return Some(event);
            }
        }
    }

    None
}

/// Attachment metadata for one message (`[{id, filename, display_name,
/// file_name, mime_type, size, size_text, content_id, is_inline}]`, no bytes). Used by the reader pane and the
/// save dialog; bytes leave Rust only via `save_attachment_to_path`.
pub fn attachments_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let files: Vec<serde_json::Value> = listed_attachments(db, m.id, m.body_html.as_deref())
        .iter()
        .map(attachment_row)
        .collect();
    Ok(serde_json::to_string(&files)?)
}

/// The attachments a reader lists, offers to open and counts: everything
/// stored except inline body parts. Besides `is_inline` this excludes a
/// part the body shows through `<img src="cid:…">`, even when the sender
/// declared it `attachment` — its stored flag may predate that rule (see
/// the v20 migration), so the feed re-checks against the body.
fn listed_attachments(
    db: &Db,
    message_id: i64,
    body_html: Option<&str>,
) -> Vec<crate::models::Attachment> {
    messages::list_attachments(db, message_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            !a.is_inline && !crate::html::is_body_referenced(a.content_id.as_deref(), body_html)
        })
        .collect()
}

/// One attachment's metadata. `display_name` is what to show (the mail's
/// name, or a fallback when it has none); `file_name` is the name a file
/// written for it gets (see `crate::paths::safe_attachment_name_for_mime`),
/// which a save dialog should suggest. `size_text` is the preformatted byte count
/// ([`crate::maintenance::format_bytes`]) so both readers show one text.
fn attachment_row(a: &crate::models::Attachment) -> serde_json::Value {
    let display_name = a
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(
            || crate::paths::safe_attachment_name_for_mime(None, a.mime_type.as_deref(), a.id),
            String::from,
        );
    json!({
        "id": a.id,
        "filename": a.filename,
        "display_name": display_name,
        "file_name": crate::paths::safe_attachment_name_for_mime(
            a.filename.as_deref(),
            a.mime_type.as_deref(),
            a.id
        ),
        "mime_type": a.mime_type,
        "size": a.size,
        "size_text": crate::maintenance::format_bytes(a.size),
        "content_id": a.content_id,
        "is_inline": a.is_inline,
    })
}

/// A stored RFC 3339 date as local `2026-09-12 13:50`; the raw value when
/// unparseable, `""` when absent.
pub(crate) fn full_local_date(raw: Option<&str>) -> String {
    match raw {
        Some(raw) => match chrono::DateTime::parse_from_rfc3339(raw) {
            Ok(dt) => dt
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            Err(_) => raw.to_string(),
        },
        None => String::new(),
    }
}

/// Header details for one message (the reader's "Headers" dialog):
/// `{from, to, cc, date, subject, message_id, reply_to}`. `date` is the full
/// local timestamp (`2026-09-12 13:50`), falling back to the stored raw value
/// when unparseable. Empty/absent fields become `""`/`[]` for QML.
pub fn headers_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let date = full_local_date(m.date.as_deref());
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
/// has_attachments}]`, newest first (like the Date view of a folder list;
/// relevance order read as random next to it), grouped by folder: folders in
/// the order of their newest hit, so both lists only start a section where
/// `folder` changes. `folder` scopes the search to one
/// folder path (empty = whole account). `snippet` is plain match context
/// (the empty-string `snippet()` markers produce it tag-free — the list
/// renders plain rows). The query language is [`crate::search`]'s. Blank,
/// operator-only or exclusion-only queries yield `[]`, never an error.
pub fn search_json(
    db: &Db,
    account_id: i64,
    query: &str,
    limit: u64,
    folder: &str,
) -> Result<String> {
    let parsed = crate::search::parse_query_full(query);
    let match_query = crate::search::fts_query(query);

    // Exclusions alone search nothing (see `is_searchable`).
    if match_query.is_none() && parsed.filters.is_empty() {
        return Ok("[]".to_string());
    }

    // The text part, bound as ?9. With a positive term the FTS match carries
    // the `NOT`s and supplies the snippet; a filter-only query reads
    // `messages` directly and drops rows matching any excluded term.
    let (source, snippet, text_clause, text_param) = match match_query {
        Some(q) => (
            "messages_fts join messages m on m.id = messages_fts.rowid",
            "snippet(messages_fts, 6, '', '', '…', 12)",
            "messages_fts match ?9",
            Some(q),
        ),
        None => (
            "messages m",
            "coalesce(m.snippet, '')",
            "(?9 is null or m.id not in
                (select rowid from messages_fts where messages_fts match ?9))",
            crate::search::fts_exclusions(query),
        ),
    };
    let flag = |v: Option<bool>| v.map(i64::from);
    let filters = &parsed.filters;
    let sql = format!(
        "select m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
                {snippet},
                m.is_read, m.is_starred, m.has_attachments, m.from_name
         from {source}
         join folders f on f.id = m.folder_id
         where {text_clause} and m.account_id = ?1
           and (?3 = '' or f.path = ?3)
           and m.id not in (select message_id from pending_moves)
           and (?4 is null or m.is_read = ?4)
           and (?5 is null or m.is_starred = ?5)
           and (?6 is null or m.has_attachments = ?6)
           and (?7 is null or m.date >= ?7)
           and (?8 is null or m.date < ?8)
         order by m.date desc, m.id desc limit ?2"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params![
            account_id,
            limit as i64,
            folder,
            flag(filters.unread.map(|unread| !unread)),
            flag(filters.starred),
            flag(filters.has_attachments),
            filters.after,
            filters.before,
            text_param,
        ],
        |row| {
            Ok(hit_json(HitRow {
                uid: row.get(0)?,
                folder_id: row.get(1)?,
                folder: row.get(2)?,
                subject: row.get(3)?,
                from_addr: row.get(4)?,
                date: row.get(5)?,
                snippet: row.get(6)?,
                is_read: row.get::<_, i64>(7)? != 0,
                is_starred: row.get::<_, i64>(8)? != 0,
                has_attachments: row.get::<_, i64>(9)? != 0,
                from_name: row.get(10)?,
            }))
        },
    )?;
    hits_json(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One search-style hit as read from SQL, before it becomes JSON.
pub(crate) struct HitRow {
    pub uid: u32,
    pub folder_id: i64,
    pub folder: String,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub date: Option<String>,
    pub snippet: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
}

/// A hit row as the frontends read it: the search shape, shared by
/// [`search_json`] and [`crate::similar::similar_json`].
pub(crate) fn hit_json(row: HitRow) -> serde_json::Value {
    let date = short_date(row.date.as_deref());
    let from = row.from_addr.unwrap_or_else(|| "?".to_string());
    // Named like the list row, so a hit keeps the row's badge.
    let from_name = row.from_name.unwrap_or_default();
    let badge = sender_badge(&from_name, &from);
    let mut hit = json!({
        "uid": row.uid,
        "folder_id": row.folder_id,
        "folder": row.folder,
        "subject": row.subject.unwrap_or_else(|| "(no subject)".to_string()),
        "from": from,
        "from_name": from_name,
        "date": date.text,
        "date_key": date.key,
        // Raw UTC timestamp for the date quick-filter on search hits too.
        "date_raw": row.date,
        "snippet": one_line(row.snippet.as_deref().unwrap_or_default()),
        "unread": !row.is_read,
        "starred": row.is_starred,
        "has_attachments": row.has_attachments,
    });
    badge.extend(&mut hit);
    hit
}

/// Hits grouped by folder, folders in the order of their first hit, the
/// hits' own order kept inside each folder.
pub(crate) fn hits_json(mut arr: Vec<serde_json::Value>) -> Result<String> {
    let mut order: Vec<String> = Vec::new();
    for hit in &arr {
        let f = hit["folder"].as_str().unwrap_or_default();
        if !order.iter().any(|o| o == f) {
            order.push(f.to_string());
        }
    }
    arr.sort_by_key(|hit| {
        let f = hit["folder"].as_str().unwrap_or_default();
        order.iter().position(|o| o == f)
    });
    Ok(serde_json::to_string(&arr)?)
}

#[cfg(test)]
mod tests;
