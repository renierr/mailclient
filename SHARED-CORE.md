# Shared core: what moves out of the frontends

The active frontends — Qt/QML (desktop) and native Kotlin (Android) — sit
over `mailcore`. This file tracks logic that is still written twice, or once
in one frontend while the other has its own version, and what replaces it.
Flutter is retired (AGENTS.md §1): its Dart copies are left as they are,
and an item that only concerns Flutter is closed unless it is a bug.

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
  listed here with the reason. A QML helper plus a Kotlin twin kept in step
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
| A forward's files: what still needs downloading, staging the rest, the left-out notice | `mailcore::compose::{forward_missing, stage_forward_files, forward_files}` |
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
| Outbox rows: counts, list, row kind and actions, one-line row state, delete, edit; a send that never started | `mailcore::outbox` (`status`, `list_json` with `kind` / `actions` / `retry_at`, `RowKind`, `RowActions`, `row_state`, `dismiss` — never a row being sent, `take_for_edit` rebuilds the composer fields and files from the stored MIME and removes the row); `compose::abandon_send` for an unstarted send job; both Outbox panes only lay the rows out |
| Similar messages: 3-tier match, subject normalize, keywords | `mailcore::similar` (`similar_json`, `target_subject`, `normalize_subject`, `extract_keywords`); frontends are UI only |
| Attachment open/save MIME | `mailcore::mime::open_mime`, carried as `open_mime` on every attachment in the reader feed; the frontends hand it to the OS opener / save picker as is |
| Message EML export: attachment download, MIME assembly, headers, safe filename | `mailcore::export` (`prepare`, `assemble_eml`, `export_eml_to`, `suggested_eml_name`); the body tree is the composer's `sync::sender::mime_body` (lettre encodes every header); frontends only pick the destination |
| On-demand attachment download ("make sure the bytes are cached") | `mailcore::sync::attachments::{ensure_cached, download}`; was a private helper in `mailapp` and an inline copy in `mailffi` |
| Calendar invite parsing: RFC 5545 VEVENT extraction, line unfolding, unescaping, date formatting, .ics save name | `mailcore::calendar` (`parse_ics`, `parse_ics_bytes`, `CalendarEvent` incl. `save_name`); `feed::message_json` supplies `"event"` directly; frontends are UI only |
| vCard contact card parsing (RFC 6350 + 2.1/3.0: groups, bare types, quoted-printable, structured N/ADR/ORG), labels, display name fallback, avatar badge, which attachments a card already covers | `mailcore::vcard` (`parse_vcard`, `ContactCard`, `is_vcard_attachment`); `feed::message_json` supplies `"contacts"` and `in_card` on each attachment (also replaced the frontends' own event-id filter); the content-line grammar is shared with `calendar` in `content_line` |
| Delivery reports (bounces): RFC 3464 parsing, status-code explanations, outcome/title/detail, action labels, finding the Sent original, resend draft and its files | `mailcore::report` (`parse_dsn`, `explain_status`, `delivery_report`, `bounce`); `compose::resend_draft` via `answer_draft_json(.., "resend")`, `compose::{resend_missing, resend_files, stage_resend_files}`; feed `"report"` |
| Read receipts: RFC 8098 parsing, disposition labels/titles/tones, the original by `Original-Message-ID` and its folder path; report and recipient tones | `mailcore::report` (`parse_mdn`, `is_disposition_part`, `DeliveryReport::{kind, tone, original_folder_path}`); frontends map the tone to a theme colour |
| Receipt requests per mail: settings decide which toggles show, the form decides the request | `mailcore::compose::Receipts` (`offered_json` via `receipt_toggles_json` / `MailNative.receiptToggles`), `ComposeForm::{request_mdn, request_dsn}`; DSN submit in `sync::sender::dsn` |
| Attached mails: header/snippet/body extraction, byline | `mailcore::attached` (`parse_attached`, `AttachedMessage`); feed `"attached_messages"` |
| Calendar reply/counter/update notice and its tone | `mailcore::calendar` (`CalendarEvent::notice`, `notice_tone`); frontends map the tone to a theme colour |
| List date quick-filter: after (inclusive) / before (exclusive) day bounds, presets, chip label | `mailcore::search::{date_passes, date_preset_range, date_filter_label}`; feeds carry `date_raw`; frontends only pick dates and show the label |
| List filters: unread / starred / attachments / date AND-ed with the short typed filter. A folder's rows are read from the cache with the filter alone, kept uids back; search hits go over as rows | `mailcore::search::list_filter::{ListFilter, keep_in_folder, keep_json}`; Qt `list_filter_uids` / `list_filter_keep`, native `MailNative.listFilterUids` / `listFilterKeep`; replaced QML `matchesQuick` and Kotlin `rowShown` (each a per-row bridge call) |
| A read/star change: only the touched list rows are re-read and swapped in, not the folder | `mailcore::feed::message_rows_json` (same row shape as the list feed); Qt `message_rows_json`, native `MailNative.messageRowsJson` |
| Custom date range: lenient days, both empty clears, Before must follow After, the error words | `mailcore::search::list_filter::date_range_check`; Qt `date_range_check_json`, native `MailNative.dateRangeCheck` |
| "Show older" footer words ("Cached N of M", "· filters cover loaded mail only") | `mailcore::feed::older_label`; Qt `older_label`, native `MailNative.olderLabel` |
| Delete confirmation: does it destroy (any target), ask first (destroys, bulk, or `confirm_delete`) | `mailcore::undo::delete_prompt` (`DeletePrompt`); Qt `delete_prompt_json`, native `MailNative.deletePrompt`; frontends pass each target folder's `delete_is_permanent` |
| Is the open message still listed (cached, not waiting out an undoable move) | `mailcore::store::messages::is_listed`; native closes its reader on it (`MailNative.messageListed`), Qt drops its preview from the reloaded rows |
| Composer "sends as" line | `mailcore::compose::editor::send_format_note`; Qt `send_format_note`, native from the editor page |
| Reply-To notice sentence | `AnswerDraft.notice` (shown while To still holds `notice_addr`) |
| What a picked contact inserts | `mailcore::store::contacts::recipient_entry`, as `entry` on every row of `contacts::contacts_json` (both adapters call it) |
| Reader text size factor | `mailcore::store::settings::reader_text_scale` — both readers and both composers follow it (Qt scales the reader/editor documents' base size by it, native its reader/composer WebView text zoom) |
| Empty message list words | `mailcore::search::empty_list_text` |
| Draft files re-attached on reopen | `mailcore::compose::stage_draft_files` (was a `mailapp` helper); native stages through `MailNative.draftFiles` after fetching `missing_files` |
| Folder sidebar fold: visible rows, collapse rule, hidden-count aggregation | `mailcore::feed::{sidebar_rows, sidebar_rows_json}` (`SidebarRow {id, collapsible, expanded, unread, total}`, expanded ids in, feed order kept); Qt via `sidebar_rows_json` invokable, native via `MailNative.sidebarRowsJson`; `Sidebar.qml` and Kotlin `collapseFolders` deleted |
| WYSIWYG editor document (was #3): page, CSP, `window.mc` script, caret slot | `mailcore::compose::editor::document` (+ `mc.set`); Qt `Bridge.editor_document` (theme colours, `font_px`, placeholder, `extra_css` for its scrollbars) and `EditorFrame.qml` drives only the `mc.*` API; native unchanged (`MailNative.editorDocument`) |
| Small UI twins (was #8): cleanup reason wording, colours-toggle rule, undo result text, capabilities error shape | `contacts::cleanup_reason_text` as `reason_text` on every `CleanupCandidate`; `html::reader::can_toggle_colors` (Qt `Bridge.can_toggle_colors`, `MailNative.canToggleColors`); `undo::undone_text` in both adapters; `sync::capabilities::{payload, failed_payload, error_payload}` with the lookup failure in the payload on both sides |
| Forward and reopened-draft file prefetch (was #9): download plus seed in one job | Qt `forward_fetch`/`resend_fetch`/`draft_form` jobs plus native `MailNative.forwardFetch`/`resendFetch`/`draftFetch` jobs (same core `forward_files`/`resend_files`/draft ensure+stage); the native shell only opens the `{draft, files}` / `{form, staged}` seed from the finish event |
| Scheduled sends: is a picked time valid (lead time) and the error words, when the next flush is due, retry backoff and give-up (`SCHEDULED_BACKOFF_SECS`, `SCHEDULED_DEADLINE_SECS`, a 5xx gives up at once), the due flush across accounts and its status line, the once-only "not sent" notice and its words, Send now / Reschedule guards | `mailcore::outbox::{send_at_error, due_in_ms, reschedule, send_now}`, `queue::{next_due, due_scheduled_account_ids, fail_scheduled_attempt, fail_due_scheduled, take_failure_notices, FailedSend::line}`, `sync::background::{send_due_scheduled, SendDueReport::status_line}`; Qt `send_at_error` / `scheduled_due_ms` / `flush_due_sends`, native `MailNative.sendAtError` / `sendDueInMs` / `sendDue`; both pickers dropped their lead-time copy and the Qt due flush its own account loop |

## Open

Ordered by priority; numbers stay as first assigned, so a done item leaves
a gap (#3, #8 and #9 closed into the Done table above — the open list is
empty). "Drift" says whether the two versions already behave differently.

## Flutter-only (retired, not pursued)

Flutter is retired, so Dart copies of core logic stay as they are. Listed
so nobody mistakes them for active work; a real Flutter bug among them may
still be fixed.

- **Busy state** — Flutter keeps its own `_busyKinds` set instead of the
  core's in-flight job table that native reads
  (`mailffi::net::busy_snapshot`). Known bug: when two jobs of one kind
  overlap (an account sync plus a folder sync or "Load older"), the spinner
  goes off at the first finish while the second still runs. Fix if it
  bites: add the snapshot to the FRB `JobEvent` (codegen) and drop
  `_busyKinds`.
- **"Does anything check in the background"** — Flutter's
  `BackgroundPlan.any` getter recomputes what the core serialises as `any`
  (`BackgroundPlan::view`). Same result today.
- **Background run history in words** — Flutter words the Settings status
  block in Dart (`describeLastRun`, `describeRun`, `limitingBucketLabel`,
  `AccountSettings.describeGap`) instead of
  `mailcore::sync::background::describe`. Same wording today.

## Deliberate frontend-only logic

Add an entry with the reason when something shared stays in one frontend
on purpose.

- **Send button enabled while any recipient field holds text** (QML
  `hasRecipients`, Kotlin `hasRecipient`). Re-evaluated on every keystroke,
  so a bridge call per character buys nothing; the core still refuses a
  message without a real recipient when Send runs
  (`ComposeForm::require_recipient`).
- **Outbox times in local words** (QML `SendAt.label` / `nextTry`, Kotlin
  `sendsAtLabel` / `nextTryLabel`). Locale date/time formatting is toolkit work: each side
  formats the core's UTC `send_at` with its own locale and 12/24-hour
  setting. The core decides validity (`outbox::send_at_error`) and drops
  `send_at` from rows that left `scheduled`, so the label never decides
  what to show.
- **When to wake for a scheduled send** (Qt `armDueTimer`, Kotlin
  `MailSendAlarm`). The core says how long until the next flush
  (`outbox::due_in_ms`); a QML Timer and an Android exact alarm are
  toolkit mechanisms. Each keeps its own backstop: Qt caps a wait at five
  minutes (Timers do not count suspend), Android re-arms after 15 minutes
  while a row stays due and after a minute when a run was skipped for the
  sync lock. How often a failing send is retried and when it gives up is
  the core's (`queue::fail_scheduled_attempt`); the backstops only wake to
  ask.
