# Native Android UI plan — 100% Kotlin frontend over `mailcore`

Goal: a standalone native Android app (Kotlin + Jetpack Compose, `android/`)
with full feature parity to the QML desktop client and the Flutter app, over
the same Rust `mailcore` through JNI (`MailNative`, package
`de.renier.mailclient` stays JNI-bound). When done, the Flutter Android
embedding is removed and the native app is the shipped Android client.

## Ground rules (from AGENTS.md, non-negotiable)

- Behaviour lives in `mailcore`. New JNI functions are translation only
  (JSON in/out, ids in, no "current folder" state) and mirror the
  `flutter_rust_bridge` API in `crates/mailffi/src/api/`.
- Kotlin holds toolkit code only: layout, navigation, gestures, theme,
  WebView hosting, SAF pickers, notifications. Any `if`/loop/format that
  would exist twice (Kotlin + Dart/QML) goes to `mailcore` instead.
- Phone-first responsive: 1-pane stack (folders → list → reader) under
  ~600dp, 2-pane list+reader on tablets/foldables; every screen usable at
  360dp width and 150% text scale. No fixed content widths, labels above
  inputs on narrow, `SafeArea` everywhere.
- Each step ends with: `cargo fmt`, `cargo clippy`, `cargo test -p mailcore`
  (if Rust touched), `./scripts/android-dev.sh --run` on the emulator,
  manual pass of that step's checklist.

## Step 0 — Close the JNI gaps (several slices, each shippable)

Today JNI covers headless background + a single-message reader slice. The UI
steps below each need the listed functions; add them slice by slice, each a
thin wrap of the named `mailffi::api` function. Blocking variants are fine
where the UI already expects them (reader experiment precedent);
list/sync paths need the queued-job + event model, so this step also adds a
`jobEvents`-equivalent: a `JobCallback { onProgress/onFinished }` listener
fed from the same `mailclient-net` thread the FRB `job_events` stream uses.

- **0a — shell reads** ✅ done (all 15 symbols in `android.rs` + `MailNative.kt`,
  `ShellReadsProbe` smoke card in `HomeScreen`, verified on emulator):
  `accountsJson`, `accountForm`, `accountFormDefaults`,
  `accountGuess`, `accountPortForSecurity`, `accountFormCheck`, `saveAccount`,
  `deleteAccount`, `initialSelection`, `selectAccount`, `foldersJson`
  (exists), `folderIdForPath`, `folderPath`, `setFolderSubscribed`,
  `folderCounts`, `outboxStatusJson`.
- **0b — sync jobs + events** ✅ done (listener replaces like `job_events`,
  events cross as one JSON string; `SyncJobsProbe` card, verified on
  emulator): `syncAccount`, `syncFolder`, `loadOlderMessages`,
  `refreshFolders`, `refreshServerCapabilities`, `backgroundMarkSeen`,
  `backgroundRunHistory`, plus the job listener above.
- **0c — list reads + bulk mutate** ✅ done (selections cross as JSON;
  `ListBulkProbe` card with undo, verified on emulator): `messagesJson` (paged), `markReadMany`,
  `setStarMany`, `toggleStar` (exists), `delete/archive/move/purge` with
  `Vec<u32>` selections, `delete/archive/move/purgeHits` (cross-folder),
  `markReadHits`/`setStarHits`, `undoMove` (exists), `undoGraceSecs`
  (exists), `createFolder`.
- **0d — composer/send** ✅ done (queued jobs report on the 0b listener;
  `ComposerProbe` card, verified on emulator): `sendMail`, `saveDraft`, `draftForm`,
  `deleteDraft`, `answerDraft`, `blankDraft`, `imageDataUrl`,
  `isInlineImage`, `senderParts`, `effectiveFrom`.
- **0e — search/contacts/settings/misc** ✅ done (44 symbols; `MiscProbe`
  card, verified on emulator against the seeded DB): `searchJson`, `searchServer`,
  `searchPlan`, `searchFilterMatches`, `dateFilterMatches`,
  `datePresetRange`, `dateFilterLabel`, `searchSyntaxHelp`, `similarJson`,
  `similarSubject`, `contactsJson`, `setContactAlias`, `deleteContact(s)`,
  `cleanupCandidatesJson`, `recipientSegment`, `replaceRecipientSegment`,
  `settingsJson`, `settingChoicesJson`, `quietTime(_at)`, `setSetting(s)`,
  `setSort`, `accountSettingsJson`, `setAccountSettings`,
  `backgroundPlanJson`, `messageHtml` (exists), `attachmentsJson` (via
  reader payload today — expose standalone), `cachedAttachmentBytes`
  (exists), `downloadAttachments` (job form; blocking `downloadMessageFiles`
  exists), `saveAttachmentTo`, `saveAllAttachmentsTo`, `writeAttachmentCopy`
  (exists), `readerPaint`, `readerPalette`, `readerFitBelow`, `readerBody`,
  `readerDocument` options (`topSpace`, `scale`, `fit` — today hardcoded),
  `linkInfo` (exists), `prepareEmlExport`, `exportMessageEmlBytes`
  (blocking `exportEmlBytes` exists), `suggestedEmlName` (exists),
  `outboxJson`, `dismissOutbox`, `storageStatsJson`,
  `cleanupTempFilesJson`, `trimLocalCache`/`trimStatus`,
  `evictCachedAttachmentsJson`, `exportDatabaseTo`.
- Verify each slice: thin Kotlin smoke call from `HomeScreen` dev buttons
  (already the pattern) before building UI on it; delete the smoke buttons
  when the real screen lands.

## Step 1 — App shell + navigation + global state ✅ done

- `MailState` (`ui/state/`), `MailShell` + top bar + status line (`ui/shell/`),
  `FoldersScreen`, `ListScreen` (placeholders — full versions in Steps 3–4),
  probes moved to the Dev route, reader stays its activity. Verified with the
  seeded DB: folders → list → reader → back, account switcher, sync status,
  outbox pill, undo snackbar wiring, notification deep-link parsing.

- `MailApp` root: Material3 theme (light/dark, follow system), ui-scale
  plumbing, `MailViewModel` (StateFlow: accounts, active account, folders,
  selection, sync status, outbox counts, undo offers) fed by 0a/0b.
- 1-pane `NavHost`: folders → list → reader, system-back walks the stack;
  2-pane list+reader variant on wide screens (foldables/tablets).
- Top bar mirrors the Qt toolbar: back/hamburger, Compose, width-capped
  search field, folder-scope toggle, Sync (with spinner), Manage folders,
  Contacts, Accounts, Settings overflow.
- Status line: sync state/error (tap → details + copy), outbox pill (tap →
  outbox, red on failure), active account. Undo snackbar with grace period
  (`undoMove`), `Ctrl+Z` equivalent where hardware keys exist.
- First-run `NoAccountsView` → account setup. Notification tap
  (`openPayload`) deep-links to the message; `readerDirty` refresh pattern
  generalized to list/sidebar.
- Verify: rotate, back-stack, dark/light, 360dp width, kill-and-restore.

## Step 2 — Accounts (add / edit / remove / switch)

- Account switcher + manager (use/switch, edit, remove with confirm:
  "local cache dropped, server untouched"); `Add account` → setup form.
- Setup form: identity, IMAP + SMTP host/port/security/user/password with
  the core's guess/defaults/port-follow/validation (`account_form_*`);
  blank password on edit keeps the vault secret; unencrypted warnings.
- Verify against the checklist in `flutter/.../account_setup_dialog.dart`
  and `qml/AccountSetup.qml`: every field and warning reachable.

## Step 3 — Folders: drawer + manager

- Folder list (indent by `depth`, role icons, unread pills, total counts,
  subscribed-only; tap paints cache instantly, then `syncFolder` in
  background — never blocks paint).
- Manager: create (`/` = nesting), subscribed checkbox (hide, keep cache),
  refresh-from-server, jump-to-folder. Move-to picker (subscribed only,
  current disabled, single + bulk modes).
- Verify: multi-account trees, special-use roles, junk/trash semantics.

## Step 4 — Message list (the biggest step; split as 4a–4e)

- **4a rows + paging**: avatar + unread dot + paperclip, sender, date
  (`date_key` "Yesterday"), star cue, subject, snippet (comfortable density;
  compact hides it); `Load older` tile (`folderCounts`/`older_state`);
  scroll restore per folder; Drafts rows open the composer.
- **4b sort + filter**: sort menu (date/from/subject, persisted via
  `setSort`); filter menu (unread/starred/attachments + date presets +
  custom range + clear).
- **4c search**: `searchPlan` routing (≤2 chars filter folder, ≥3 FTS +
  debounced `searchServer` backfill), folder-scope toggle, hits grouped by
  folder, similar chip (`similarJson`, dismissable), open-hit returns to
  results.
- **4d selection + bulk**: tap/long-press multi-select, select menu
  (all/unread/starred/invert/clear), bulk bar (read, star, archive, move,
  trash, purge with confirms), cross-folder hit actions, EML export
  (SAF save).
- **4e row menu**: read/unread, star, archive, move, trash vs purge
  (per `delete_is_permanent`, confirms), find similar, save-as-eml.
- Verify each slice on a real mailbox: 200+ message folder, search with
  filters AND-combined, bulk across folders, trash/junk purge rules.

## Step 5 — Reader (Compose port of the `ReaderActivity` experiment)

- Migrate the Views experiment into Compose: header (avatar, sender, date,
  To/Cc, reply-to-differs banner, expandable technical headers), action row
  (Reply, Forward, Star, Delete, original-colors, fullscreen, ⋮: reply-all,
  archive, move, purge, find similar, save-eml, show headers, show remote
  images), WebView body via core `readerDocument` (paint/palette/fit from
  0e, remote gated + show-once), plain-text path with reader scale.
- Attachments card (open via FileProvider, save, save-all through SAF,
  on-demand download with retry), ICS `EventCard` (open-in-calendar, save),
  inline-image banner, link examine dialog (else direct per setting),
  `markReadPlan` (now/after-delay/off, cancelled on close).
- Delete the Flutter delegation (`ACTION_READER`, `DelegateScreen`,
  `native_reader.dart` on the Dart side) when reply/forward/similar are
  native (step 6 for the first two, 4c for similar).
- Verify: HTML + plain + ICS mails, remote-images gate, dark mode paint,
  undo bar after delete/archive/move, narrow-width action wrap.

## Step 6 — Composer (fullscreen, all entry modes)

- Entries: blank, reply / reply-all / forward (`answerDraft` quote +
  signature placement), draft (`draftForm`, pinned account, server-draft
  delete). From = editable local part + locked `@domain`, display name;
  To/Cc/Bcc with contact autocomplete (`recipientSegment`), Reply-To row,
  subject, reply-to-mismatch banner.
- Body: Markdown editor (Flutter precedent: `**bold**`, `> quote`, lists,
  links, `![name](inline:N)`) + preview toggle; quote card (collapsible,
  removable, above/below per setting); toolbar (bold/italic/quote/bullet,
  link, image, attach); attachments tray + file picker; inline images via
  `imageDataUrl`.
- Footer: Send (validation inline, optimistic close at SMTP accept, reopen
  on failure), Save draft (stays open + lock), Discard (dirty guard),
  delete-draft (confirm), send-format label.
- Verify: all 5 entries, dirty-guard paths, double-send lock, attachment +
  inline round-trip, Drafts-folder reopen.

## Step 7 — Contacts

- Manager: prefix search, alias edit, remove (confirm), auto-collected
  explainer; cleanup-review mode (automated/stale candidates, multi-select,
  bulk remove with confirm). Autocomplete already in step 6.
- Verify: alias flows into composer suggestions; cleanup only removes
  what it lists.

## Step 8 — Settings (all sections + per-account overrides)

- Sections: Interface (UI scale, reader text size), Mailbox (sort, density,
  confirm-delete), Reading (mark-read + delay, remote images, link action),
  Composing (send format, include-plain, quote placement, signature, MDN),
  Accounts & sync (scope picker all/one, interval, push, sent-copy,
  contacts, notifications, quiet hours + times, scheduler, background
  status block with permission flows + run history), Maintenance (storage
  stats, DB export, temp cleanup, attachment eviction, cache trim — each
  confirmed), About (version, license, DB path, per-account capabilities +
  refresh).
- Draft-edit + single Save (changed keys only); narrow rail → dropdown.
- Verify: every key round-trips through `settingsJson`; quiet-hours plan
  changes scheduler behaviour; maintenance actions report correct stats.

## Step 9 — Outbox + background integration

- Outbox screen (queued/sending/failed rows, error text, sync-now retry,
  dismiss dead rows); status pill live from `outboxStatusJson` + job
  events; send flow ends here on SMTP failure.
- Wire existing native background (worker/alarm/push/notifier — already
  JNI-done) to the new UI: resume reload, `backgroundMarkSeen`,
  notification actions refresh list/reader, test-notification from
  Settings.
- Verify: offline send → outbox → airplane-off → flush; notification
  mark-read updates list without opening.

## Step 10 — Responsive + tablet pass

- 2-pane list+reader (and 3-pane on large tablets if it earns its keep),
  pane-width memory, fullscreen reader, multi-window/split-screen sanity,
  foldable posture change without losing selection.
- Full sweep at 360dp, short heights, 150% text scale, dark/light,
  TalkBack labels on icon buttons.
- Verify: no `RenderFlex`-class overflows (Compose: no clipped text or
  pushed-off-screen actions), dialogs become fullscreen pages on small
  screens.

## Step 11 — Parity audit + Flutter removal (the finish line)

- Walk the QML and Flutter inventories (attached in chat during planning:
  shell, toolbar, folders, list, reader, composer, contacts, accounts,
  outbox, settings, search, notifications) feature by feature; every gap
  becomes a step-4–9 sub-item or a `SHARED-CORE.md` deliberate exception.
- `flutter test`, `cargo test --workspace` green; `./build.sh --android`
  produces the signed APK; live-mailbox pass with per-run consent.
- Flip: drop the `.native` `applicationId` suffix, remove `DelegateScreen`
  + reader delegation, remove the Flutter Android embedding, update
  `PROJECT.md` milestone 14 → done, `AGENTS.md`, `android/README.md`,
  `flutter/README.md` ("Shared code still to promote" + Android chapter).
- Definition of done for the branch: a reviewer can uninstall Flutter's
  APK, install only the native APK on a phone, and do everything the
  Flutter app does — same core, same data, no delegation left.
