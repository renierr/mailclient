//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::badge::sender_badge;
use crate::db::Db;
use crate::error::Result;
use crate::html::{self, Sanitized};
use crate::models::{Folder, FolderRole};
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

/// The "Show older" footer's words for `cached` local rows against the
/// server's last count (`None`: never reported). `filtered`: list filters
/// are on, which only ever cover the loaded rows. `""` for an empty folder,
/// where the footer is hidden.
#[must_use]
pub fn older_label(cached: u64, server: Option<u64>, filtered: bool) -> String {
    let base = match older_state(cached, server) {
        OlderState::Empty => return String::new(),
        OlderState::Unchecked => format!("Cached {cached} (server not checked)"),
        OlderState::Partial => format!("Cached {cached} of {}", server.unwrap_or(cached)),
        OlderState::Complete => format!("All {cached} messages loaded"),
    };
    if filtered {
        format!("{base} · filters cover loaded mail only")
    } else {
        base
    }
}

/// `[{id, name, role, unread, subscribed, count, delimiter, depth, leaf,
/// always_visible, server_total, older, can_load_older,
/// delete_is_permanent}]` ordered by path. `depth`/`leaf` derive from the
/// picker and the sidebar indent and label subfolders identically instead of
/// each splitting the path itself.
pub fn folders_json(db: &Db, account_id: i64) -> Result<String> {
    let counts = messages::counts_by_account(db, account_id)?;
    let list = folders::list_by_account(db, account_id)?;
    let role_by_path: std::collections::HashMap<&str, FolderRole> =
        list.iter().map(|f| (f.path.as_str(), f.role)).collect();
    let mut arr = Vec::new();
    for f in &list {
        let c = counts.get(&f.id).copied().unwrap_or_default();
        arr.push(json!({
            "id": f.id,
            "name": f.path,
            "role": f.role.as_str(),
            "unread": folder_unread(f, c.unread),
            // Sidebar visibility toggle + cached total (see Folders dialog),
            // plus the hierarchy fields so the move picker can indent
            // subfolders (depth = segments - 1) and show the short name.
            "subscribed": f.subscribed,
            "count": c.total,
            "delimiter": f.delimiter,
            "depth": folder_depth(&f.path, &f.delimiter),
            "leaf": folder_leaf(&f.path, &f.delimiter),
            "always_visible": folder_always_visible(f, &role_by_path),
            // `-1`: the server never reported a count.
            "server_total": f.server_total.map_or(-1, |s| s as i64),
            "older": older_state(c.total, f.server_total).as_str(),
            "can_load_older": older_state(c.total, f.server_total).can_load(),
            "delete_is_permanent": crate::undo::delete_is_permanent(f, &list),
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Sidebar collapse rule: known folders stay visible inside a collapsed
/// parent (see `FolderRole::always_visible`), and so do the inbox's direct
/// children — on servers that file everything below the inbox those read
/// as top-level. Deeper custom subfolders fold away.
fn folder_always_visible(
    f: &Folder,
    role_by_path: &std::collections::HashMap<&str, FolderRole>,
) -> bool {
    f.role.always_visible() || {
        parent_path(&f.path, &f.delimiter)
            .and_then(|p| role_by_path.get(p.as_str()))
            .is_some_and(|r| *r == FolderRole::Inbox)
    }
}

/// The sidebar's unread pill: Trash never carries one (the list uses the
/// same zero to decide its badge).
fn folder_unread(f: &Folder, unread: u64) -> u64 {
    if f.role == FolderRole::Trash {
        0
    } else {
        unread
    }
}

/// One painted sidebar row: the folder's id plus its collapse state and
/// the counts to show. A collapsed parent aggregates its hidden children's
/// counts so no unread pill disappears with them. Painted columns only —
/// path, role, depth and leaf still come from `folders_json`, joined by id.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SidebarRow {
    pub id: i64,
    pub collapsible: bool,
    pub expanded: bool,
    pub unread: u64,
    pub total: u64,
}

/// Fold one account's subscribed folders into painted sidebar rows for the
/// given expanded parents (`expanded`: folder ids, in-memory UI state).
/// The single implementation of the rule both frontends used to duplicate
/// (`Sidebar.qml`, Kotlin `collapseFolders`): well-known folders always
/// show, custom subfolders show only while every ancestor up to the nearest
/// always-visible one is expanded, a folder whose parent is not visible
/// reads as a root, and a collapsed parent carries its hidden children's
/// counts. Parentage uses each folder's real IMAP delimiter — never a
/// one-character assumption. Feed order is kept; hidden rows are omitted.
#[must_use = "the rows are the sidebar; discarding them paints nothing"]
pub fn sidebar_rows(db: &Db, account_id: i64, expanded: &[i64]) -> Result<Vec<SidebarRow>> {
    let counts = messages::counts_by_account(db, account_id)?;
    let list = folders::list_by_account(db, account_id)?;
    // Both frontends fold the subscribed subset, so a parent outside it
    // counts as none and the child reads as a root.
    let visible: Vec<&Folder> = list.iter().filter(|f| f.subscribed).collect();
    let role_by_path: std::collections::HashMap<&str, FolderRole> =
        visible.iter().map(|f| (f.path.as_str(), f.role)).collect();
    let id_by_path: std::collections::HashMap<&str, i64> =
        visible.iter().map(|f| (f.path.as_str(), f.id)).collect();
    let always: std::collections::HashMap<i64, bool> = visible
        .iter()
        .map(|f| (f.id, folder_always_visible(f, &role_by_path)))
        .collect();
    let expanded_set: std::collections::HashSet<i64> = expanded.iter().copied().collect();

    let parent_of = |id: i64| -> Option<i64> {
        let f = visible.iter().find(|f| f.id == id)?;
        parent_path(&f.path, &f.delimiter).and_then(|p| id_by_path.get(p.as_str()).copied())
    };
    // Shown while every ancestor up to the nearest always-visible one is
    // expanded (top-level and well-known folders always show).
    let is_shown = |mut id: i64| -> bool {
        loop {
            if visible
                .iter()
                .find(|f| f.id == id)
                .is_none_or(|f| folder_depth(&f.path, &f.delimiter) == 0 || always[&id])
            {
                return true;
            }
            match parent_of(id) {
                None => return true,
                Some(p) => {
                    if !expanded_set.contains(&p) {
                        return false;
                    }
                    id = p;
                }
            }
        }
    };
    let is_under = |row: i64, mut id: i64| -> bool {
        loop {
            if id == row {
                return true;
            }
            match parent_of(id) {
                None => return false,
                Some(p) => id = p,
            }
        }
    };

    let mut rows = Vec::new();
    for f in &visible {
        if !is_shown(f.id) {
            continue;
        }
        // Collapsible only when the toggle hides something: a direct child
        // that folds away. INBOX, whose children all stay visible, gets no
        // chevron and stays inbox-only in counts.
        let collapsible = visible
            .iter()
            .any(|d| parent_of(d.id) == Some(f.id) && !always[&d.id]);
        let open = expanded_set.contains(&f.id);
        let c = counts.get(&f.id).copied().unwrap_or_default();
        let mut unread = folder_unread(f, c.unread);
        let mut total = c.total;
        if collapsible && !open {
            for d in &visible {
                if d.id != f.id && !is_shown(d.id) && is_under(f.id, d.id) {
                    let dc = counts.get(&d.id).copied().unwrap_or_default();
                    unread += folder_unread(d, dc.unread);
                    total += dc.total;
                }
            }
        }
        rows.push(SidebarRow {
            id: f.id,
            collapsible,
            expanded: open,
            unread,
            total,
        });
    }
    Ok(rows)
}

/// `[{id, collapsible, expanded, unread, total}]` for the sidebar: the
/// painted rows of [`sidebar_rows`] as JSON.
pub fn sidebar_rows_json(db: &Db, account_id: i64, expanded: &[i64]) -> Result<String> {
    Ok(serde_json::to_string(&sidebar_rows(
        db, account_id, expanded,
    )?)?)
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

/// The parent of an IMAP path: everything but the last segment
/// (`Work/Client` → `Work`). None for a top-level folder or an empty
/// delimiter.
pub fn parent_path(path: &str, delimiter: &str) -> Option<String> {
    if delimiter.is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = path.split(delimiter).collect();
    if segments.len() < 2 {
        return None;
    }
    segments.pop();
    Some(segments.join(delimiter))
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
/// `message_sort_field` / `message_sort_desc` — Date means newest shown
/// date first).
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
    list_rows_json(rows, is_trash_folder(db, folder_id))
}

/// The list rows of `uids` in `folder_id`, shaped like
/// [`messages_list_json_paged`]'s, in uid order. Uids that are not listed
/// (gone, or hidden by a pending move) are left out. After a flag change a
/// frontend swaps in just these rows instead of re-reading the folder.
pub fn message_rows_json(db: &Db, folder_id: i64, uids: &[u32]) -> Result<String> {
    let rows = messages::list_compact_by_uids(db, folder_id, uids)?;
    list_rows_json(rows, is_trash_folder(db, folder_id))
}

/// Trash shows nothing as unread: its rows are on their way out.
pub(crate) fn is_trash_folder(db: &Db, folder_id: i64) -> bool {
    folders::get(db, folder_id).is_ok_and(|f| f.role == FolderRole::Trash)
}

/// What a list row shows for a mail without a subject or a sender address.
pub(crate) const NO_SUBJECT: &str = "(no subject)";
pub(crate) const NO_SENDER: &str = "?";

fn list_rows_json(rows: Vec<messages::CompactMessage>, is_trash: bool) -> Result<String> {
    Ok(serde_json::to_string(
        &rows
            .into_iter()
            .map(|m| {
                let date = short_date(m.date.as_deref());
                let from = m.from_addr.unwrap_or_else(|| NO_SENDER.to_string());
                let from_name = m.from_name.unwrap_or_default();
                let badge = sender_badge(&from_name, &from);
                let mut row = json!({
                    "uid": m.uid,
                    "subject": m.subject.unwrap_or_else(|| NO_SUBJECT.to_string()),
                    "from": from,
                    "from_name": from_name,
                    "date": date.text,
                    "date_key": date.key,
                    // Raw UTC timestamp for the list date quick-filter
                    // (`search::date_passes`); `date` above is display text.
                    "date_raw": m.date,
                    "snippet": m.snippet.unwrap_or_default(),
                    "unread": !is_trash && !m.is_read,
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
    let listed = listed_attachments(db, m.id, m.body_html.as_deref());
    let event = find_calendar_event(db, &m);
    let contacts = contact_cards(db, &listed);
    let report = crate::report::delivery_report(db, &m, &listed);
    let attached = attached_messages(db, &listed);
    // Files a preview card already opens and saves: the frontends leave
    // them out of the attachment list so one file never shows twice.
    let carded: Vec<i64> = event
        .iter()
        .filter_map(|e| e.attachment_id)
        .chain(contacts.iter().filter_map(|c| c.attachment_id))
        .chain(attached.iter().filter_map(|a| a.attachment_id))
        .chain(report.iter().flat_map(|r| r.covered.iter().copied()))
        .collect();
    let files: Vec<serde_json::Value> = listed
        .iter()
        .map(|a| {
            let mut row = attachment_row(a);
            row["in_card"] = carded.contains(&a.id).into();
            row
        })
        .collect();
    let contacts: Vec<serde_json::Value> = contacts.iter().map(contact_json).collect();
    let date = short_date(m.date.as_deref());
    let from = m.from_addr.as_deref().unwrap_or("?");
    let from_name = m.from_name.as_deref().unwrap_or("");
    let reply = crate::compose::reply_address(from, m.reply_to.as_deref().unwrap_or(""));
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
        "event": event,
        "contacts": contacts,
        "report": report,
        "attached_messages": attached,
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
            let is_cal = att
                .mime_type
                .as_deref()
                .is_some_and(crate::mime::is_calendar_mime);
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

/// Contact cards for the listed `.vcf` attachments, one per file (its
/// first card). A file whose bytes are not cached yet gives a pending card
/// ([`crate::vcard::ContactCard::pending`]); one that holds no vCard stays
/// an ordinary attachment.
fn contact_cards(db: &Db, listed: &[crate::models::Attachment]) -> Vec<crate::vcard::ContactCard> {
    listed
        .iter()
        .filter(|a| {
            crate::vcard::is_vcard_attachment(a.filename.as_deref(), a.mime_type.as_deref())
        })
        .filter_map(|att| {
            let full = messages::get_attachment(db, att.id).ok()?;
            match full.data.as_deref() {
                Some(bytes) => {
                    let mut card = crate::vcard::parse_vcard_bytes(bytes)?;
                    card.set_attachment(att);
                    Some(card)
                }
                None => Some(crate::vcard::ContactCard::pending(att)),
            }
        })
        .collect()
}

/// Cards for the listed mails attached to this one (`.eml`,
/// `message/rfc822`). A pending card while the bytes are not cached; a file
/// that is no mail stays an ordinary attachment.
fn attached_messages(
    db: &Db,
    listed: &[crate::models::Attachment],
) -> Vec<crate::attached::AttachedMessage> {
    listed
        .iter()
        .filter(|a| {
            crate::attached::is_message_attachment(a.filename.as_deref(), a.mime_type.as_deref())
        })
        .filter_map(|att| {
            let full = messages::get_attachment(db, att.id).ok()?;
            crate::attached::AttachedMessage::for_attachment(att, full.data.as_deref())
        })
        .collect()
}

/// A contact card plus the avatar badge for its name and first address.
fn contact_json(card: &crate::vcard::ContactCard) -> serde_json::Value {
    let mut row = serde_json::to_value(card).unwrap_or_default();
    let email = card.emails.first().map_or("", |e| e.value.as_str());
    sender_badge(&card.name, email).extend(&mut row);
    row
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
    let shown = crate::html::BodyImages::of(body_html);
    messages::list_attachments(db, message_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.is_inline && !shown.shows(a.content_id.as_deref()))
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
    let file_name = crate::paths::safe_attachment_name_for_mime(
        a.filename.as_deref(),
        a.mime_type.as_deref(),
        a.id,
    );
    json!({
        "id": a.id,
        "filename": a.filename,
        "display_name": display_name,
        // What the OS opener / save picker gets (`mime::open_mime`).
        "open_mime": crate::mime::open_mime(a.mime_type.as_deref(), Some(&file_name)),
        "file_name": file_name,
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

/// Every address in every occurrence of the address header `wanted`
/// (`Delivered-To` repeats once per hop), unfolded and comma-split.
pub(crate) fn header_values(raw: Option<&str>, wanted: &str) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut values: Vec<String> = Vec::new();
    let mut open = false;
    for line in raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let (true, Some(v)) = (open, values.last_mut()) {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        open = false;
        if let Some((name, v)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case(wanted) {
                values.push(v.trim().to_string());
                open = true;
            }
        }
    }
    values
        .iter()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(String::from)
        .collect()
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
