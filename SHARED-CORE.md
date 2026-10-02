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
| Search: index threshold, trimmed query, hit limit, debounce, short-input filter | `mailcore::search::{plan, filter_matches, HIT_LIMIT}` |
| Link safety: may a link open, scheme / domain / path for the examine dialog | `mailcore::html::link_info` (the sanitizer's `safe_href` rule) |
| Reader HTML document: paint decision, palette, dark rewrite, width fitting, CSP and base CSS | `mailcore::html::reader`; Qt dropped its CSS `filter`, Flutter desktop its `ColorFiltered` — all three renderers show the same rewritten colours |
| Folder rules: permanent delete, "Show older" state | `mailcore::undo::delete_is_permanent`, `mailcore::feed::older_state`; folder feed fields `delete_is_permanent`, `server_total`, `older`, `can_load_older` |
| Bulk actions across folders (search hits) | `mailcore::bulk` (one flag write, one Undo batch, one purge job); search hits arrive grouped by folder from `feed::search_json` |

## Open

Ordered by priority; numbers stay as first assigned, so a done item leaves
a gap. "Drift" says whether the two versions already behave differently.

### 11. Small items

Pick these up when the area is touched anyway.

- **File size text:** Qt shows KB with one decimal, Flutter rounds. Feed
  field `size_text`.
- **"Yesterday":** Flutter ignores `date_key`, so the word stays English.
  Use `date_key` like Qt does.
- **Folder depth and short name:** `MoveTo.qml` and Dart `models.dart`.
  Feed fields `depth` and `leaf`.
- **Mark-read on open:** Flutter only marks unread rows, Qt always asks.
  One `mark_read_plan(settings)` deciding whether and when.
- **Job outcome:** Qt detects partial success by matching the status text
  ("sent, but …"), Flutter reads a flag, and Flutter also starts the Sent
  sync itself. An `outcome` field on job events.
- **Recipient autocomplete segment:** `components/RecipientField.qml` and Dart
  `composer_widgets.dart` match, but both break on a quoted name containing
  a comma. `compose::recipient_segment` / `replace_segment`.

## Deliberate frontend-only logic

None listed yet. Add an entry with the reason when something shared stays
in one frontend on purpose.
