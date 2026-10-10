# Maildesk spike — findings & frontend protocols

Experiment branch `experiment/egui-desktop-spike`. Throwaway scaffolding in
this crate; `mailcore` is read-only (no core changes made). All fixture data
uses `@example.com` only; the spike opens the dev DB (`MAILCLIENT_DB`,
default `./data/dev.sqlite`) and refuses the platform mailbox.

## 1. Status

- **Spike A (reader): done.** 20 fixtures, 40 scripted screenshots
  (light + dark) with block-tree dumps, grades in §4 below.
- **Spike B (composer): contract extracted (§3), prototype pending.**

## 2. Reader protocol — what `MessageView.qml` consumes

Source: `crates/mailapp/qml/MessageView.qml` (1476 lines), `FeedJson.qml`.

### 2.1 Feed roles (one message)

`subject, from/from_name, reply_to/reply_to_differs, date/date_key,
body_text/body_html, is_html, has_remote_images, html_colored,
attachments[], event/report/contacts/attached_messages` cards,
`missing_inline_images`.
Sender display name + To/Cc/full date arrive via on-demand
`message_headers_json` (loaded once per opened message); the list feed
carries only the bare From address. `FeedJson.parse` contract: every feed
getter returns valid JSON or its empty shape — never a parse error, never an
empty string.

### 2.2 Body pipeline (all in `mailcore`, frontend-agnostic)

1. `feed::sanitized_bodies(raw_html, raw_text, allow_remote)` →
   `(safe_html, had_remote, is_html, plain)`.
2. Inline `cid:` resolution against stored parts
   (`inline_cid_images`); missing bytes become alt text + a count.
3. `has_own_colors(safe)` → `html_colored`; `paint_for(colored, dark,
   keep_original)` → `Theme | Original | Darkened`.
4. `reader::body(safe, paint, fit)` — the string an egui renderer consumes;
   dark rewrite + narrow-fit already applied. `fit_below(safe)` gives the
   narrow threshold (`0` = no fixed widths).
5. `reader::document(...)` wraps the same body for web engines only
   (CSP, base CSS, `#mc-top` header spacer) — irrelevant to egui.

### 2.3 Banner flows

- `had_remote` → "Remote images blocked" + **Show once**: re-sanitizes the
  *stored raw* via `message_html(uid, allow_remote=true)` for that view
  only (the feed copy already lost the URLs). Setting `load_remote_images`
  skips the banner globally.
- `missing_inline_images > 0` → Download notice → explicit
  `download_inline_images` job → feed reload.
- Inline `cid:`/`data:` always load; they are mail content, not tracking.

### 2.4 Layout contract (Qt arrangement; egui equivalent in brackets)

- One scrolling page: header + cards overlay the body top and move with
  `contentScrollY`; the body reserves the header height (HTML: `#mc-top`
  spacer synced by JS; plain: y offset). The body keeps its **own**
  scroller — a newsletter never becomes one giant surface.
  [egui: plain top-down scroll, no overlay needed.]
- Reloads coalesced (`Qt.callLater`) and keyed on **UID, not object
  identity**; re-clicking the open message is a no-op. Scroll state is
  per-message (the spike uses `id_salt(("reader", uid))`).
- Cards above the body scroll with the header: event, delivery-report,
  contact (`.vcf`), attached-message (`.eml`), inline-missing notice,
  attachments (`display_name`/`file_name`/`size_text` from core;
  `in_card` files excluded to avoid double cards).
- `⋮` menu: Reply-all / Archive / Move / Purge / Find similar /
  Save-as-.eml / Headers / Show-remote-once. Rare actions live here, not in
  banners.
- Links: navigation blocked **first** (only the loader's own TypedNavigation
  accepted); `link_info` decides safety; then examine-dialog (default) or
  direct browser per `link_click_action`. Hover URL in a floating bubble;
  right-click over a link gets Copy/Examine. Middle/Ctrl-click treated as a
  normal click, never a new window.
- Empty state: "Select a message to read it".

## 3. Composer protocol — what `Composer.qml` needs (§8.2 resolved)

Sources: `Composer.qml` (1100), `EditorFrame.qml` (172),
`ComposerToolbar.qml`.

### 3.1 Editing operations (the complete set)

Toolbar → `document.execCommand` with `styleWithCSS` off (emits semantic
`<b>/<i>/<u>` that survive the outgoing sanitizer; Qt rich-text emitted
inline styles that were silently stripped — flaw F10):

| UI | Command | State polling |
|---|---|---|
| Bold (Ctrl+B) | `bold` | `boldActive` |
| Italic (Ctrl+I) | `italic` | `italicActive` |
| Underline (Ctrl+U) | `underline` | `underlineActive` |
| Bullet list | `insertUnorderedList` | `listActive` |
| Quote | `formatBlock` ⇄ `blockquote`/`p` | `quoteActive` |
| Insert link (dialog, "select text first") | `createLink` | — |
| Clear formatting | `removeFormat` | — |
| Insert image inline | `insertImage(data:)` | — |
| Attach files | file picker | — |
| HTML source toggle | raw round-trip both ways | — |

Toolbar state is **polled** (200 ms timer + after every command):
`queryCommandState` for marks/list, `queryCommandValue('formatBlock')`
matched against `/blockquote/i`. Caret/selection changes have no signal.

### 3.2 Body lifecycle

- New/reply/forward/draft bodies come **prepared by
  `mailcore::compose::answer`** (recipients, subject, quote + signature
  placement); the composer only fills fields. Caret starts in the first
  empty top-level paragraph (reply typed above the quote).
- Read-back is async (`innerHTML` via callback); Send finishes inside it.
- `resolve_bodies` + `compose_send_format`/`compose_include_plain`
  derive the plain/multipart shape; **Auto sends text/plain unless the
  body carries real formatting**. The header shows `send_format_note`
  ("sends as …"), refreshed on a 300 ms debounce.
- Inline images travel as `data:` URLs in the document; the sender turns
  them into MIME parts. Non-inlinable files are reported, not inserted.
  Dropped files: images ask inline-or-attach, everything else attaches.

### 3.3 Send/draft payload (all fields, `payloadFor`)

`from` (via `effective_from`: local part editable, **domain locked** to the
account — `@` rejected by validator), `from_name`, `reply_to`,
`to/cc/bcc` (autocomplete via `RecipientField` + contacts; send allowed
while **any one** is non-empty), `subject`, `body` + `body_html` (same
HTML), `attachments` (paths only — Rust reads bytes at send time),
`draft_uid` (-1 = new), `request_mdn`/`request_dsn` (per-mail toggles gated
by `receipt_toggles_json` settings).

### 3.4 Lifecycle guards (all must survive the port)

- `dirty` tracking on every field; close-with-edits → Unsent-changes
  dialog (Discard / Save-draft); server drafts: Discard only abandons
  local edits, **never** destroys the server copy (separate explicit
  delete with its own confirm).
- `sendPending`/`saving` re-entry guards: one composition at a time.
- Reply-To-elsewhere banner (`AnswerDraft.notice`), dismissable by editing
  To; forward `files_notice` for undownloadable files.
- Editor hardening: no remote/file loads, no navigation (own loads only),
  no popups; clipboard access on (paste works).

### 3.5 Spike B prototype scope (from this contract)

Smallest editor serving the send path: header fields + mark toggles
(B/I/U, list, quote) + link insert + source toggle + validate + save-draft
— against the real `compose` API, offline (no send). Known hard parts to
grade: selection-scoped formatting in immediate mode (no DOM selection),
live toolbar state without polling signals, caret placement in
reply-quotes, image-at-caret insertion.

## 4. Spike A grades (20/20 shot light + dark)

`match` = indistinguishable for reading; `readable-differs` = reads
perfectly, looks slightly plainer; `broken` = unusable.

| # | Fixture | Grade | Differs in |
|---|---|---|---|
| 1 | plain | match | — |
| 2 | simple-html | match | — |
| 3 | headings-lists | match | — |
| 4 | quotes | match | nested rule bars |
| 5 | code | match | pre + entities decoded |
| 6 | table-simple | match | header bold, `bgcolor`, striping |
| 7 | newsletter | match | equal-width (not content-proportioned) columns; banner image as alt text |
| 8 | dark-mail | match | Original + Darkened both faithful to core |
| 9 | remote-images | match | banner + alt text (show-once path stubbed) |
| 10 | inline-data | match | 1px PNG → exactly 1px (probe-tested) |
| 11 | image-only | match | blocked-banner flow |
| 12 | links | match | unsafe links visibly inert |
| 13 | align-marks | readable-differs | `text-align:right` falls back to left |
| 14 | preheader | match | hidden subtree skipped |
| 15 | float | readable-differs | image on own line by design, no wrap |
| 16 | wide-table | readable-differs | fluid equal columns vs fixed 900px (no h-scroll) |
| 17 | entities | readable-differs | Latin-ext/entities match; **CJK tofu — no bundled CJK font** |
| 18 | no-subject | match | — |
| 19 | attachments | match | card + cached/on-server states |
| 20 | long-thread | match | own scroller, Archive switch |

No `broken`. Bugs found & fixed during the spike: unstable per-frame
widget ids (egui `request_discard` storm); hand-built wrap layout claiming
full height (use `horizontal_wrapped`); `image::thumbnail` upscaling 1px
dots to 1200² (downscale-only cap); UTF-8 tokenizer mojibake
(`bytes[i] as char`); shared ScrollArea offset leaking across messages.

## 5. Core change requests

None required — every Spike A need was already public API
(`sanitized_bodies`, `message_html`, `reader::{body, paint_for, palette,
fit_below, has_own_colors}`, `link_info`, `inline_cid_images`, typed
stores, `format_bytes`). Proposal for graduation (not needed now): a
**typed reader payload struct** so same-process frontends stop parsing
`message_json` strings (QML keeps the JSON shape).

## 6. Verification

`cargo fmt --check`, `cargo clippy -p maildesk-spike --all-targets --
-D warnings`, `cargo test -p maildesk-spike` (12 passed, incl. headless
egui layout probes) green; `cargo test -p mailcore` 648 passed
(untouched). QML/Flutter/Android rows N/A (no changes there). No full
`./build.sh` (packaging proves nothing here). Offline only; `image 0.25`
PNG dep approved before use; nothing committed or pushed.
