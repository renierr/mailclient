# Shared core: what moves out of the frontends

Both frontends (Qt/QML and Flutter) sit over `mailcore`. This file tracks
logic that is still written twice, or once in one frontend while the other
has its own version, and what replaces it.

## The rule

- **Common logic lives once in `mailcore`.** Anything both frontends would
  compute (rules, decisions, parsing, formatting derived from data,
  defaults, choice lists, validation) is a `mailcore` function, and the
  result reaches the frontends as a plain struct or feed fields saying
  *what* to show.
- **Frontends decide *how* it looks:** layout, sizes, fonts, widgets,
  gestures, theme lookups, toolkit APIs.
- **Heavily UI-dependent logic may stay in a frontend, but avoid it where
  possible.** When something is half common and half toolkit (for example
  the reader's HTML document versus how each toolkit paints dark mode),
  move the common half and keep only the toolkit half in the frontend.
- A frontend-only copy of shared logic is a deliberate, stated exception,
  listed here with the reason. A QML helper plus a Dart twin kept in step
  by matching tests is not an exception; it is a candidate.

AGENTS.md §1 states the rule for agents. `flutter/README.md` ("Shared code
still to promote") points here instead of keeping its own list.

## Done

| Area | Now in |
|---|---|
| Composer send and drafts | `mailcore::compose` |
| Account saving | `mailcore::store::account_form` |
| IMAP session pool, lease, panic guard | `mailcore::sync::pool` |
| Sender avatar letters and colour | `mailcore::badge` |
| Where a reply goes, Reply-To differs | `mailcore::compose::reply_address` |
| New, reply, reply-all and forward drafts | `mailcore::compose::answer` |
| Account setup guesses, ports, security choices, field check, edit form | `mailcore::store::account_form::{guess, default_port, port_after_security_change, SECURITY_CHOICES, check, load, defaults_json}` |
| Attachment file names, Save, Save all, viewer copy | `mailcore::paths::safe_attachment_name`, `mailcore::store::messages::{save_attachment_to, save_all_attachments_to, write_attachment_copy}`; feed fields `display_name` / `file_name` |
| Locked From domain: split for the field, joined for sending | `mailcore::compose::{sender_parts, effective_from}`; both fields refuse a typed `@` |
| Settings choice lists and defaults | `mailcore::store::settings::choices` (per key: default and offered values); frontends only label them, in the same words |
| Quiet-hours times: reading a typed time, picker parts, defaults | `mailcore::store::account_settings::{normalize_time, time_parts, time_at}`; defaults from `settings::choices` |
| Search: index threshold, advanced filter tokens, structured query planning, hit limit, debounce, short-input filter | `mailcore::search::{plan, filter_matches, parse_query_full, SearchFilters, ParsedQuery, HIT_LIMIT}`, `feed::search_json` |
| Link safety: may a link open, scheme / domain / path for the examine dialog | `mailcore::html::link_info` (the sanitizer's `safe_href` rule) |
| Reader HTML document: paint decision, palette, dark rewrite, width fitting, CSP and base CSS | `mailcore::html::reader`; Qt dropped its CSS `filter`, Flutter desktop its `ColorFiltered` — all three renderers show the same rewritten colours |
| Folder rules: permanent delete, "Show older" state | `mailcore::undo::delete_is_permanent`, `mailcore::feed::older_state`; folder feed fields `delete_is_permanent`, `server_total`, `older`, `can_load_older` |
| Bulk actions across folders (search hits) | `mailcore::bulk` (one flag write, one Undo batch, one purge job); search hits arrive grouped by folder from `feed::search_json` |
| File size text | `mailcore::maintenance::format_bytes` (one decimal at every scale) via the attachment feed field `size_text`; both readers dropped their twins |
| "Yesterday" | feed `date_key` threaded through the Dart models (`displayDate`); the word stays the UI's, as in Qt |
| Folder depth and short name | folder feed fields `depth` and `leaf` (`feed::folder_depth`/`folder_leaf`); `MoveTo.qml` and Dart `Folder` read them |
| Mark-read on open | `mailcore::store::settings::mark_read_plan` (off / now / after-delay from the two settings plus the row state); both viewers follow it |
| Job outcome | `SendOutcome::outcome` (`"sent"` / `"sent_partial"`) on the job event (`job_finished` outcome, `JobEvent.outcome`); Qt keys its close decision off it, and the Sent refresh stays `deliver`'s job on both |
| Recipient autocomplete segment | `mailcore::compose::{recipient_segment, replace_recipient_segment}`, quote-aware; both recipient fields complete through them |
| Outbox rows: counts, list, dismiss, one-line row state; a send that never started | `mailcore::outbox` (`status`, `list_json`, `dismiss` — never a row being sent, `state_line`); `compose::abandon_send` for an unstarted send job; retry is the next sync (`flush_outbox`), both dialogs only start one |
| Similar messages: 3-tier match, subject normalize, keywords | `mailcore::similar` (`similar_json`, `target_subject`, `normalize_subject`, `extract_keywords`); frontends are UI only |
| Attachment open/save MIME | `mailcore::mime::open_mime`, carried as `open_mime` on every attachment in the reader feed; the frontends hand it to the OS opener / save picker as is |
| Message EML export: attachment download, MIME assembly, headers, safe filename | `mailcore::export` (`prepare`, `assemble_eml`, `export_eml_to`, `suggested_eml_name`); the body tree is the composer's `sync::sender::mime_body` (lettre encodes every header); frontends only pick the destination |
| On-demand attachment download ("make sure the bytes are cached") | `mailcore::sync::attachments::{ensure_cached, download}`; was a private helper in `mailapp` and an inline copy in `mailffi` |
| Calendar invite parsing: RFC 5545 VEVENT extraction, line unfolding, unescaping, date formatting, .ics save name | `mailcore::calendar` (`parse_ics`, `parse_ics_bytes`, `CalendarEvent` incl. `save_name`); `feed::message_json` supplies `"event"` directly; frontends are UI only |
| List date quick-filter: after (inclusive) / before (exclusive) day bounds, presets, chip label | `mailcore::search::{date_passes, date_preset_range, date_filter_label}`; feeds carry `date_raw`; frontends only pick dates and show the label |

## Open

Ordered by priority; numbers stay as first assigned, so a done item leaves
a gap. "Drift" says whether the two versions already behave differently.

### 1. Busy state: which jobs are in flight

Native reads the core's in-flight job table (`mailffi::net::busy_snapshot`,
carried as `busy` on every JNI job event, plus a `queued` event per job).
Flutter still keeps its own `_busyKinds` set, filled on a successful queue
call and cleared by kind on the first finished event. Drift: yes — when two
jobs of one kind overlap (an account sync plus a folder sync or "Load
older"), Flutter's spinner goes off at the first finish while the second
still runs. Fix: add the snapshot to the FRB `JobEvent` (codegen) and drop
`_busyKinds`.

### 2. Sync-on-resume gap

"Resume syncs unless a sync was asked for within the last minute" exists
twice: Dart `shouldSyncOnResume` and Kotlin `MailState.RESUME_SYNC_GAP_MS`.
Drift: none yet. Fix: a `mailcore` predicate taking the seconds since the
last request.

### 3. WYSIWYG editor document

`mailcore::compose::editor::document` builds the HTML editor page (CSS,
nonce CSP, the `window.mc` script) that native Android loads. Qt's
`EditorFrame.qml` still assembles its own page in QML and polls
`queryCommandState` itself. Drift: small — native's page has a CSP and a
header spacer, and quote toggling and links go through `mc.quote` /
`mc.link`. Fix: give the Qt bridge the core document (its colours and
scale as `EditorStyle`) and keep only the WebEngine wiring in QML; Qt polls
`mc.state()` since it has no `MCHost`. Flutter's Markdown editor is
Flutter's own and not a copy of anything.

### 4. "Does anything check in the background"

The core now serialises `any` with the background plan
(`BackgroundPlan::view`); native reads it. Flutter's
`BackgroundPlan.any` getter recomputes it. Drift: none. Fix: read the
`any` field in `BackgroundPlan.fromJson`.

### 5. Background run history in words

`mailcore::sync::background::describe` (`last_run_line`, `run_line`,
`run_lines`, `limiting_bucket`, `heartbeat_gap`) words the Settings status
block for native Android. Flutter still runs its Dart originals:
`describeLastRun`, `describeRun`, `limitingBucketLabel` in
`sync/background_power.dart` and `AccountSettings.describeGap`. Drift: none
yet (same wording, ported with the stale-run threshold). Fix: an FRB
function returning `RunLines` plus the two small helpers (codegen), then
delete the Dart copies.

## Deliberate frontend-only logic

None listed yet. Add an entry with the reason when something shared stays
in one frontend on purpose.
