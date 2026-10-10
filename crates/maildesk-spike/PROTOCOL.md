# Maildesk spike — findings & frontend protocols

Experiment branch `experiment/egui-desktop-spike`. Throwaway scaffolding in
this crate; `mailcore` is read-only (no core changes made). All fixture data
uses `@example.com` only; the spike opens the dev DB (`MAILCLIENT_DB`,
default `./data/dev.sqlite`) and refuses the platform mailbox.

## 1. Status

- **Spike A (reader): done.** 20 fixtures, 40 scripted screenshots
  (light + dark) with block-tree dumps, grades in §4 below.
- **Spike B (composer): contract extracted (§3), prototype done** — egui
  document model + toolbar + validate + MIME/draft path, covered by tests
  and a scripted screenshot. Findings and remaining gaps in §7.

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

## 5. Composer API map (Spike B, as built)

Everything the prototype touches, all already public in `mailcore`:

| Need | Call |
|---|---|
| new mail body | `compose::blank_draft(&opts)` → `AnswerDraft.body_html` |
| reply/forward prefill | `compose::answer_draft_for(db, folder, uid, AnswerMode::Reply)` |
| signature + bottom-posting | `AnswerOptions { own_address, signature, reply_below_quote }` (from `store::settings` keys `signature_enabled`/`signature_text`/`reply_below_quote`) |
| form → validation | `compose::ComposeForm { .. }.require_recipient()` (`"add at least one recipient (To, Cc or Bcc)"`) |
| header note | `compose::editor::send_format_note(format, html)` |
| send-format decision | `html::needs_html_formatting(html)` + `sender::effective_format(Auto, needs, include_plain)` |
| plain/html split + sanitize | `sender::resolve_bodies(text, Some(html), format)` |
| MIME bytes (draft path) | `sender::format_draft(&account, &form.as_request(..))` |
| build the send request | `ComposeForm::as_request(account, format, include_plain, receipts, policy)` |

### 5.1 Two paths, not one — the trap a port must not fall into

- **`format_draft` is the draft-save path and is always multipart/alternative
  by design** ("drafts always preserve rich text when present, independently
  of the user's send preference"). It is offline-safe and what the spike's
  MIME preview builds. It is *not* what a Send submits.
- **The send path is `compose::send::{prepare_send, deliver}`** — async, and
  it applies `effective_format(...)` itself plus `load_outgoing_attachments`
  and `split_inline_images` (`data:` images become `cid:` parts). Not
  exercised offline; the spike says "not sent" instead of faking it.
- A frontend must **not** pass `SendFormat::Auto` straight into
  `as_request`: `resolve_bodies` documents that Auto behaves like Multipart
  there. Resolve with `effective_format` (what `send_format_note` shows) and
  pass the concrete format.

### 5.2 Editor model (egui, `src/compose.rs`)

`Vec<EditBlock> { kind: Para|Bullet|Quote, runs: Vec<EditRun{text, marks,
link}> }`.

- Per-block `TextEdit::singleline` shows `runs`' concatenated text; typing
  is synced back with a **prefix/suffix char diff**, so marks survive edits.
- Selection-scoped ops (`toggle_mark`, `set_link`, `clear_range`) split runs
  at the boundaries, set/clear, then re-merge — the `execCommand` semantics
  of repeated toggling (all-set → clear).
- `Enter` splits a block, Backspace-at-0 merges into the previous one
  (cursor restored via `TextEditState`), toolbar state reads the caret's run
  directly — **no polling timer**, replacing QML's 200 ms `pollState`.
- `to_html()` emits only tags the outgoing sanitizer keeps
  (`<b>/<i>/<u>/<code>/<a>/<p>/<ul>/<li>/<blockquote>`); `from_html()` parses
  back through the Spike A parser for source-toggle and reply prefills.

### 5.3 What is NOT covered (Spike B gaps)

| Gap | Why it matters |
|---|---|
| Inline image at caret | editor inserts `data:` URLs; the turn-into-`cid:` part is `sender::split_inline_images` on the send path |
| Attachment list | paths only in the payload; picking files is a frontend file dialog |
| Received-receipt toggles | `Receipts::offered(db)` decides which show; form carries `request_mdn/dsn` |
| Send + server draft save + delete draft | async, network — needs a job thread (the real `mailapp` `mailclient-net` pattern) |
| Dirty/discard guard | trivial state machine, not yet wired |
| Multi-line paragraphs (soft wrap) | one block per line; Shift+Enter / wrapped paste needs `<br>` in runs |
| `From` local-part + locked domain | `compose::{sender_parts, effective_from}`; domain rejection is the field validator |
| Recipient autocomplete | `compose::{recipient_segment, replace_recipient_segment}` + contacts |
| Reply-To notice banner | `AnswerDraft.notice` / `notice_addr`; dismissal rule is the frontend's |

## 6. Spike B grades (20/20 reader in §4; composer graded by capability)

| Capability (Composer.qml) | Spike B |
|---|---|
| header fields (From name / local part / To / Subject) | partial — To + Subject; From/Reply-To not wired |
| B / I / U on a selection | done |
| bullet / quote | done (block kinds, toggle) |
| link insert / unlink / clear formatting | done |
| HTML source toggle round-trip | done |
| send-format note + Auto decision | done (core's own) |
| validate before send | done (`require_recipient`) |
| MIME preview of the request | done (draft path; send path marked offline) |
| attachments (add/remove) | not covered |
| inline image at caret | not covered |
| receipt toggles | not covered |
| signatures on new mail | covered via `AnswerOptions` (off in fixtures) |
| reply/forward prefill from a real cached mail | covered (`answer_draft_for`) |
| send / save draft / delete draft (server) | not covered (async) |
| dirty + discard-confirm guard | not covered |
| drag & drop files | not covered |
| Quotes in replies (bottom posting) | covered by core's `body_html` (caret placement gutted: reader-style, not caret-in-slot) |

## 7. Core change requests

None required — every need in Spike A **and** B was already public API.
Proposals for graduation (not needed for the spike to answer its question):

1. **Typed reader payload struct** (§2) so same-process frontends stop
   parsing `message_json` strings; QML keeps the JSON shape.
2. **Typed `AnswerDraft` for same-process frontends** — `answer_draft_json`
   / `blank_draft_json` exist for QML; a same-process port wants the struct
   it already gets from `answer_draft_for`.

## 8. Verification

`cargo fmt --check`, `cargo clippy -p maildesk-spike --all-targets --
-D warnings`, `cargo test -p maildesk-spike` (21 passed: parser, painter
layout probes, document ops, and the MIME/send-format contracts) green;
`cargo test -p mailcore` 648 passed (untouched). QML/Flutter/Android rows
N/A (no changes there). No full `./build.sh` (packaging proves nothing
here). Offline only; `image 0.25` PNG dep approved before use; Spike A
committed as `1422fbd`, later work uncommitted.
