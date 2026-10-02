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

## Open

Ordered by priority; numbers stay as first assigned, so a done item leaves
a gap. "Drift" says whether the two versions already behave differently.

### 3. Locked From domain

- **Where:** `effectiveFrom` in `Composer.qml`, `_effectiveFrom` and its
  split helpers in `flutter/lib/src/ui/composer/composer_dialog.dart`.
- **Drift: yes.** Flutter takes a typed full address as is, so the lock is
  skipped in the form; sending still refuses a foreign domain
  (`sender_domain_is_aligned`), but only at send time.
- **Change:** `compose::effective_from(local_part, account_email)` used by
  both forms, so the address shown is the address sent.

### 4. Settings choice lists and defaults

- **Where:** `Settings.qml`, `AccountOverrides.qml`;
  `flutter/lib/src/ui/settings/` and `flutter/lib/src/models/settings.dart`
  (which mirrors every core default).
- **Drift: yes.** Labels differ ("Every hour" vs "Every 60m"); defaults are
  copied by hand into Dart.
- **Change:** a `settings::choices()` feed: per key the allowed values and
  the default. Frontends only label them.

### 5. Quiet-hours times

- **Where:** `AccountOverrides.qml`,
  `flutter/lib/src/models/settings.dart` (`QuietTime`), and the core's own parser;
  the `00:00`/`07:00` defaults repeated in `Settings.qml` and Dart.
- **Drift:** three parsers for one format.
- **Change:** expose the core's time normalising and defaults through the
  account-settings feed; delete the frontend parsers.

### 6. Search thresholds and the local filter

- **Where:** `Main.qml` and `MessageList.qml`;
  `flutter/lib/src/state/mail_state.dart` and `message_list_pane.dart`.
- **Drift: yes.** Qt trims the query, Flutter does not. Both repeat the
  three-letter threshold, debounce, hit limit and the one-to-two-letter
  local filter.
- **Change:** `search::plan(query) -> {mode, backfill}` and a shared
  `matches(row, query)`; the debounce delay is a constant from core.

### 7. Link safety

- **Where:** `crates/mailapp/qml/LinkSafety.qml`,
  `flutter/lib/src/ui/reader/link_safety.dart` (near copies).
- **Drift: yes.** Only Dart ignores case in `mailto:`.
- **Change:** `html::link_info(url) -> {safe, scheme, host, path}` next to
  the sanitizer's own `safe_href`, so the reader and the sanitizer agree.

### 8. Reader HTML document and dark mode

- **Where:** `wrapDoc`, `invertedHex`, `paintMode` in `MessageView.qml`;
  `flutter/lib/src/ui/reader/mail_paint.dart`, `mail_web_view.dart`,
  `mail_dark.dart`, `mail_fit.dart`. An unused
  `mailcore::html::wrap_document` exists.
- **Drift: yes.** Base font size and palette constants differ; Qt darkens at
  display time with a CSS filter, Flutter rewrites colours up front and
  also fits wide layouts.
- **Change (split):** the document itself (CSP, base CSS, palette from the
  frontend's theme colours, paint-mode decision, colour rewriting, width
  fitting) in `mailcore::html`. How the page is painted on screen stays
  toolkit-specific where the toolkits genuinely differ. Largest item;
  do it on its own.

### 9. Folder rules: permanent delete, "Show older"

- **Where:** `Main.qml`, `MessageList.qml`; `mail_state.dart`,
  `message_list_widgets.dart`.
- **Drift: yes.** Both lists now show every cached row, so "Show older"
  always asks the server (Flutter used to page the cache and fell back to
  the first page on reopen). Left: a "No cached messages" case only in Qt,
  and both re-derive from the backend whether delete is permanent.
- **Change:** folder feed fields `delete_is_permanent` and
  `older: {can_load, cached, server}`.

### 10. Bulk actions across folders (search hits)

- **Where:** `Main.qml`, `MessageList.qml`; `mail_state.dart`,
  `message_list_widgets.dart`.
- **Drift:** same behaviour today, written twice (grouping by folder,
  joining undo batches and labels).
- **Change:** core bulk operations taking `[(folder, uid)]`; search hits
  arrive grouped from the feed.

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
