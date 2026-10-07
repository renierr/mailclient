# AGENT.md — Coding Agent Rules & Boundaries

This file is normative for all coding agents (human or AI) working in this repo.
`PROJECT.md` describes _what_ we build; this file describes _how_ we work.

## 1. Stack & Targets

- Primary OS: **Omarchy Linux (Arch-based, "quattro")**, Wayland, Qt **6.11+**.
- Backend: **Rust (stable, edition 2021)**, workspace in `crates/`.
  - `mailcore`: pure-Rust library — SQLite storage, models, IMAP/SMTP sync engine. **No Qt dependency.**
  - `mailapp`: thin Qt/QML bridge binary using **cxx-qt 0.9** + `mailcore`. Only crate allowed to depend on Qt.
  - `mailffi`: `cdylib` exposing `mailcore` to the Android frontends — JNI
    for native Android (`src/android.rs`, the `jni` crate) and
    **flutter_rust_bridge 2** (`dart:ffi`, in process) for Flutter. No Qt,
    no mail logic — a translation layer only, same rule as `mailapp`.
- Frontends: all over `mailcore`, which is the base for everything. A
  behaviour change belongs in `mailcore` so every frontend gets it; a
  change made in one frontend's adapter alone must be a deliberate, stated
  choice. Two frontends are active and kept in sync:
  - **Qt/QML** (`crates/mailapp/qml/`) — the desktop client: Linux
    (primary) and Windows.
  - **Native Android** (`android/`) — Kotlin + Jetpack Compose, the
    Android client. `./build.sh --android`. See `android/README.md`. The
    Kotlin package `de.renier.mailclient` is JNI-bound (Rust `Java_*`
    symbols) and must not be renamed; only the `applicationId` is free.
  - **Flutter** (`flutter/`) — **retired.** It stays in the tree and keeps
    building (`./build.sh --apk`, `--flutter`), but gets no new features:
    a feature added to Qt and native does not need a Flutter twin, and
    Flutter's column in the parity table may fall behind. Obvious bugs may
    still be fixed there. Do not remove it or its build wiring without
    asking. See `flutter/README.md`.
  - **Features stay comparable between Qt and native; UI may differ.**
    Every user-facing feature exists in both. Layout, placement of elements
    and interaction patterns may drift where touch and small screens call
    for it (e.g. bottom actions, fullscreen forms, swipe/long-press instead
    of hover or context menus). A UI-only change in one frontend needs no
    mirror in the other; a feature change does.
  - **Shared logic is built once, in `mailcore`.** If both frontends would
    compute the same thing, it is a `mailcore` function and the result
    reaches them through the feed or the bridge — never a QML helper plus a
    Kotlin twin kept in step by matching tests. This includes logic that is
    UI-flavoured but frontend-neutral: what a sender's avatar shows (letters,
    colour per theme — `mailcore::badge`), parsed address parts, display
    decisions derived from data. Return a plain struct or feed fields that
    say what to show; the frontend only decides how (sizes, fonts, layout,
    theme lookups, widgets). Toolkit-bound code (Qt/Compose APIs, gestures,
    rendering) stays in its frontend. Heavily UI-dependent logic may live
    in a frontend, but avoid it where possible: when something is half
    common, half toolkit, move the common half. `SHARED-CORE.md` tracks
    what is still duplicated and the deliberate exceptions.
- Qt frontend: **QML (QtQuick + QtQuick.Controls)**, single source in
  `crates/mailapp/qml/`, embedded via the `Mailclient` QML module
  (`CxxQtBuilder::new_qml_module`). HTML mail rendered via `QtWebEngine`.
- Storage: **SQLite** via `rusqlite` (bundled). One DB file per user: `~/.local/share/mailclient/mailclient.sqlite`.
- Mail protocols: IMAP on `imap-next` + `imap-types`, async over `tokio` with `tokio-rustls` for TLS (rustls-only everywhere, no OpenSSL — the Android cross-compile depends on it). SMTP send via `lettre`, which also builds the outgoing MIME; incoming MIME is parsed by `mail-parser`. Future protocols (POP3/JMAP/EWS/Graph) must go behind traits in `mailcore::sync`.

## 2. Boundaries — what agents MUST / MUST NOT do

### MUST
- Keep `mailcore` UI-free and unit-testable. All DB access goes through `mailcore::db` / `store::*`.
- Evolve the SQLite schema **only via versioned migrations** in `crates/mailcore/src/db/migrations.rs` + `schema.sql`. Never edit a released migration in place; add a new one. Bump `SCHEMA_VERSION`.
- Keep passwords/secrets **out of SQLite and git**. DB stores only `auth_vault_key`; actual secrets live in the OS keyring (`keyring` crate) or memory — on Android, in the app-private `auth_vault.json` (not the Android Keystore; see `flutter/README.md`).
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test -p mailcore` before finishing a backend change. After changing anything in `crates/mailffi/src/api/`, re-run `flutter_rust_bridge_codegen generate` from the repo root and commit both generated files.
- Run `qmllint` (from `/usr/lib/qt6/bin`) and `scripts/qml-format.sh` on changed QML when available (`qmllint` needs `-I` for the built `Mailclient` import; unresolved-`Mailclient` warnings are expected pre-build).
- Run `flutter analyze` (must be clean) and `flutter test` in `flutter/` before finishing a Flutter change.
- Update `PROJECT.md` ("Where we stand") when a milestone step completes.
- Put build artefacts only in `dist/` (gitignored). Never commit binaries, `.sqlite` files, or secrets.

### MUST NOT
- Do **not** install/upgrade/remove system or language packages (`pacman`, `cargo install`, `npm`, `pip`, …) without explicit user consent. Running and building with already-installed tools is fine; if something is missing, stop and ask.
- Do **not** commit to git, push, create PRs, or change git config unless the user explicitly asks.
- **Consent is per-action: never auto-commit or auto-push.** Ask the user for
  consent before every commit and every push. One consent covers exactly that
  one action — it never carries over to newer commits or pushes. When in doubt,
  leave changes uncommitted and say so.
- **Live-server runs need explicit per-run consent.** Never run the sync
  harness, test sends, or anything else that contacts a real mailbox or uses
  live credentials without the user explicitly asking for that exact run.
  The harness enforces this mechanically (requires `-- --live`); do not
  bypass or remove that guard. `cargo test` must stay offline (in-memory
  SQLite only) — never add a test that dials out.
- **Never uninstall the app from a device or emulator.** `./scripts/android-dev.sh --uninstall`
  and `adb uninstall` wipe the app's data (accounts, settings, cached mail,
  seeded databases) with no way back. If an install fails (e.g. a version
  downgrade that would need a reinstall), stop and ask instead of
  uninstalling — data loss is never an acceptable side effect of deploying
  a test build.
- Do **not** add broad new external dependencies without justification. Prefer: std → small well-scoped crate → large framework. Large additions (new Qt modules, new async runtime, new DB) require user approval.
- Do **not** put business logic in QML. QML is view-only; logic lives in Rust and is exposed via explicit bridge types.
- Do **not** invent new top-level directories without updating this file and `PROJECT.md`. Current ones: `crates/`, `flutter/`, `android/`, `qml` (inside `mailapp`), `resources/`, `scripts/`, `dist/` (gitignored). Root docs: `AGENTS.md`, `PROJECT.md`, `SHARED-CORE.md`.
- Do **not** let Qt and native Android drift in features or behaviour (UI
  layout may differ, see §1). Before copying anything out of
  `mailapp` into `mailffi` (or back), check whether it belongs in `mailcore`
  instead. Where a copy already exists it is listed in `SHARED-CORE.md` —
  add to that list rather than quietly making a third.
- Do **not** add features to the retired Flutter app (§1); fix obvious bugs
  only.

## 3. Code Style

- Formatting is pinned in committed config so Linux and Windows produce the same bytes: `rustfmt.toml` (style edition), `flutter/analysis_options.yaml` `formatter:` (the style follows the language version, i.e. the `environment: sdk` lower bound in `flutter/pubspec.yaml`, not the installed Flutter), and the qmlformat version in `scripts/qml-format.sh`. Only qmlformat has no such style versioning, hence its version pin (a machine without it skips formatting). Raise a style version or the pin on purpose and reformat once.
- Rust: `rustfmt` defaults (style edition pinned), `clippy` clean, `thiserror` for errors, `serde` for JSON fields, `chrono` (UTC/RFC3339) for times, `log` + `env_logger` for logging.
- Networking is async and never runs on the Qt GUI thread. `mailapp` owns one
  dedicated `mailclient-net` thread holding a current-thread Tokio runtime, and
  every IMAP/SMTP job is queued onto it; the GUI hears back through
  `CxxQtThread`. Keep it that way — a blocking call on the GUI thread freezes
  the window, and a second runtime is a dependency decision (see §4).
- SQL: lowercase keywords, `snake_case` tables/columns, explicit `FOREIGN KEY … ON DELETE CASCADE`, indexes for every `(account_id, folder_id, uid)`-style lookup and for date-descending list queries. Every table has `created_at`/`updated_at` (UTC ISO8601 text) unless it is a pure FTS/virtual table.
- QML: formatted by `scripts/qml-format.sh [file.qml ...]`, never by a bare `qmlformat -i`. qmlformat's output changes between Qt minor versions, so the script pins one (`PINNED`, the Omarchy system Qt) and refuses any other version; the repo-root `.qmlformat.ini` supplies indent, line endings and column width. On Windows point `QT_BIN_DIR` (or `QMLFORMAT`) at a kit of the pinned version — without one, leave formatting to the Linux machine rather than reflowing with another version. Pass no style flags, and do not hand-align what it would reflow. One component per file in `crates/mailapp/qml/`, `PascalCase.qml` filenames, `qmllint`-clean, no inline JS business logic beyond formatting. Pure QML logic that only Qt needs (parsing bridge payloads, Qt-side decisions) lives in `pragma Singleton` helpers (cf. `FeedJson`, `AccountOverrides`) — logic native Android needs too belongs in `mailcore` (§1) with `tst_*.qml` coverage run by `scripts/qml-check.sh`. All user-visible strings ready for `qsTr()`. Rust objects reach QML via `#[qml_element]` in the `Mailclient` module — never duplicate QML outside the crate.
- QML must stay responsive: dialogs are resizable (`AppDialog` with geometry memory) and windows vary in width, so every pane has to adapt instead of clipping. Rules: wrapping text gets `wrapMode` + a width bound (`Layout.fillWidth`); content inside a `ScrollView` binds its width to the ScrollView's own `availableWidth` via an explicit `id` (never `parent.availableWidth` — ScrollView reparents its children, so `parent` is not the ScrollView and the column falls back to its implicit width, which disables wrapping and pushes trailing controls off-screen); items in a `RowLayout` that must yield get `Layout.minimumWidth: 0` (e.g. a ComboBox next to a button); `Flow` only wraps when its own width is constrained. Verify resizable dialogs at narrow widths, not just the default size.
- Native Android must stay usable at 360dp widths, short (~400dp) heights, and 150% text scale — clipped, overlapping or unreachable content is a bug. Verify small width *and* small height, not just the default emulator.

### Native Android (`android/`)

How the Compose app is written. `android/README.md` covers the build and the JNI seam.

- **Where code lives.** One feature directory per area under `ui/` (`shell/`, `folders/`, `list/`, `reader/`, `composer/`, `accounts/`, `contacts/`, `settings/`, `outbox/`); pieces used by more than one feature go in `ui/common/`, not copied. `ui/state/MailState.kt` holds what the shell shows; JNI calls go through `MailNative`, and screens subscribe to `JobEvents` (never `MailNative.setJobListener`) and close the subscription on dispose. A file past ~500 lines took a second job — split it by responsibility.
- **No Kotlin twins of core logic.** A decision, format or rule the Qt side also needs is a `mailcore` function reached through a `MailNative` extern in `crates/mailffi/src/android.rs`. Kotlin keeps toolkit code: layout, navigation, gestures, theme, WebView hosting, SAF pickers, notifications.
- **Layout keys on width and text scale**, never on device type: below 700dp one pane, below 1100dp folders + list (the reader takes the list's place), else three (`ui/shell/MailPanes.kt`). Labels sit *above* inputs on narrow layouts. No fixed content widths.
- The composer and Settings are always full pages, on every width — never dialogs. Qt keeps them as resizable dialogs; that is a deliberate UI difference (§1).
- The shell bar mirrors the Qt toolbar: back (one pane) or sidebar toggle (three panes), Compose, a width-capped search field, then Sync and the tools (Manage folders, Contacts, Accounts, Settings); in one pane the scope toggle and tools collapse into an overflow menu. No folder name in the bar: the list header already shows it. The sidebar account chip only switches accounts; adding/managing lives under Accounts. Do not add a second Compose, Manage-folders or Accounts entry.
- The reader is one scrolling page in both frontends: header, attachments card and notices scroll away with the body; nothing is pinned above it. HTML bodies keep their own scroller (a WebEngine/WebView sized to a whole newsletter is one enormous surface) — the header overlays the top of the page, follows its scroll position, and the document starts with an `#mc-top` spacer of the header's height. Rarely used actions (show remote images, delete permanently) live in the reader's ⋮ menu, not in banners.
- Touch targets: pane dividers keep a ~24dp hit area (`ui/common/PaneDivider.kt`); do not shrink a drag handle to the 1px line. System back walks the route stack (reader → list → folders) and never closes the app from a nested pane.
- Bottom chrome (status strip, bulk bar, composer actions) stays above the gesture inset. A hover-only affordance needs a tap equivalent — phones have no hover.
- No emojis in code unless the user asks. Concise comments only.

### Flutter (`flutter/lib/`) — retired

Flutter gets bug fixes only (§1). These rules keep a fix consistent with the code around it; they are not a reason to extend the app. `flutter/README.md` explains the FFI seam.

**Where a widget lives.** One public widget per file, named after what it does (`MessageTile`, `ReaderHeader`, `PaneDivider`), in the feature directory that owns it (`ui/shell/`, `ui/message_list/`, `ui/reader/`, `ui/composer/`). A widget used by more than one feature goes in `ui/dialogs/mail_dialog.dart` (`MailDialog`, `MailFormPage`, `MailDialogShell`, `folderIcon`, `avatarColor`, `senderInitial`) — not copied. Do not nest a second widget class in the same file, and do not name widgets `_Private`. `State` classes stay private; the widget does not.

**Rebuilds.** `MailState` notifies on every job, status line and selection change. Subscribe to the fields the widget paints: `context.select<MailState, T>((s) => s.field)` for one value, `context.read<MailState>()` in callbacks. Never `watch` in a shell, list, reader or dialog that only needs a slice; a `ListView` item takes the row model as a constructor argument.

**Layout.** It must stay overflow-free at 360px widths, short (~400px) heights, and 150% text scale — `RenderFlex` overflow is a bug. Key on width and text scale (`Breakpoints`, `MailDialog.isNarrow`, `settings.uiScale`), never on `Platform.isAndroid`. A `Row` of text plus buttons overflows at 360px: use `Wrap`, or `Expanded`/`Flexible` with `overflow: TextOverflow.ellipsis`. Dialog actions are a `Wrap`. Any flow with a `TextField` routes through `MailDialog.showForm` (fullscreen page on narrow or short screens); never pad a dialog by `viewInsets` yourself. `SafeArea` on every dialog and on the status bar; `PopScope` walks the pane stack.

### File size & where tests live

A file that keeps growing is usually a module that has taken on a second
responsibility. Treat these as prompts to look, not as hard gates:

- **Past roughly 500 lines of non-test code**, split by responsibility rather
  than by line count — the way `imap.rs` and `sender.rs` became directories of
  focused modules. Name the parts after what they do, not `utils`/`helpers`.
- **Before splitting or relocating anything, check for non-test callers.**
  Code whose only callers are its own tests is dead: delete it and the tests
  with it, and retarget any coverage worth keeping at the live function. That
  is cheaper than carefully rehoming code nothing runs.
- **Tests stay colocated** (`#[cfg(test)] mod tests` at the foot of the file)
  by default — being next to what they cover is worth a lot, and a high test
  ratio in a small file is a well-tested small file, not a problem. Extract
  only when the file is genuinely hard to move around in: past roughly 400
  lines *and* more than about 40% tests, or a test block over ~250 lines on
  its own. Then move the block to a sibling submodule and leave
  `#[cfg(test)] mod tests;` behind: `src/feed.rs` + `src/feed/tests.rs`, or a
  directory of focused files when there are several themes, as in
  `sync/imap/tests/`. Pure move, no behaviour change — `super::*` still
  reaches the parent's private items.
- **Never relocate unit tests into `crates/<crate>/tests/` to shrink a file.**
  That directory is a separate crate that can only see `pub` items, so moving
  them there forces visibility to be widened for testing alone. It is for
  genuine end-to-end tests against the public API; everything else stays a
  `#[cfg(test)]` submodule inside the crate, where private items are reachable.

## 4. Dependency Policy

Allowed without asking: **whatever is already pinned in the workspace's
`Cargo.toml` files.** Those are the source of truth — read them rather than a
list here, which goes stale the moment a dependency is swapped. Broadly they
cover storage (`rusqlite`, bundled), errors (`thiserror`), serialisation
(`serde`/`serde_json`), time and ids (`chrono`, `uuid`), logging
(`log`/`env_logger`), the IMAP stack (`imap-next`, `imap-types`, `tokio`,
`tokio-rustls` and its `rustls-*` / `webpki-roots` trust roots), `lettre`,
`mail-parser`, `keyring`, `directories`, `tempfile`, the Qt bridge (`cxx`,
`cxx-qt`, `cxx-qt-lib`, `cxx-qt-build`) and `winresource` for the Windows
executable resources. The Flutter bridge adds
`flutter_rust_bridge` (pinned with `=`), `anyhow` and `android_logger`, plus
`jni` for the Kotlin entry points on Android (`mailffi/src/android.rs`).

The `=` pin on `flutter_rust_bridge` is deliberate: the codegen tool, the Rust
crate and the Dart package must be the same version, so a range would let
`cargo update` silently desync them. Changing it means changing all three and
regenerating.

Dart packages are the same kind of decision as a crate, and `flutter/pubspec.yaml`
is their source of truth — read it rather than a list here. Broadly: the
bridge (`flutter_rust_bridge`), state (`provider`), platform paths and files
(`path_provider`, `file_picker`, `open_filex`, `url_launcher`, and
`desktop_drop` for files dropped onto the composer), formatting and
HTML (`intl`, `flutter_widget_from_html_core`, and `webview_flutter` for the
Android reader), and the notification permission prompt
(`flutter_local_notifications`; background checks themselves are native
Kotlin over `androidx.work`), plus the
dev-only `flutter_launcher_icons`. Anything else → ask.

Anything else (new crypto, a second async runtime, new Qt modules beyond
Core/Gui/Qml/Quick/QuickControls2/Network/WebEngine) → ask first. Removing or
replacing a pinned dependency is the same kind of decision as adding one, so
it needs the same ask.

## 5. Workflows

- Build: `./build.sh --qt` (Qt release bundle into `dist/`), `./build.sh --flutter`
  (Flutter Linux bundle into `dist/mailclient-flutter/`), `./build.sh --apk`
  (signed Android APK into `dist/mailclient-apk/`), `./build.sh --android`
  (signed native Compose APK into `dist/mailclient-android/`), `./build.sh --all` (both
  desktop bundles). Dev loop: `./dev.sh`
  (Qt) or `./dev.sh --flutter`. Both dev loops use `./data/dev.sqlite`
  (`MAILCLIENT_DB` overrides). Install locally: `./scripts/install-local.sh` (`~/.local`). Never hand-roll `cargo build` output paths in docs; point to the scripts.
- Flutter: `flutter run -d windows` / `-d linux` from `flutter/` (the Rust core builds as part of it). Regenerate FFI glue with `flutter_rust_bridge_codegen generate` from the repo root.
- Native Android: `./scripts/android-dev.sh --run` (asks to boot the emulator when none is online, `installDebug`, launches; `--log` tails logcat, `--build` / `--dist` / `--emulator` / `--clean` / `--uninstall` are separate tasks; bare invocation prints help and runs nothing). Machine config via `ANDROID_SDK_ROOT` / `ANDROID_AVD` / `JAVA_HOME` env or gitignored `android/local.properties` (`sdk.dir`, `avd.name`) — never committed, so Linux and Windows checkouts differ safely. Raw form: `android/gradlew installDebug` on a device/emulator (the Rust core builds as part of it via cargo-ndk). Compose BOM / WorkManager / core-ktx versions are pinned in `android/app/build.gradle.kts`; anything beyond them needs approval like any other dependency.
- Versions: the product version lives once in the workspace root `Cargo.toml` (`[workspace.package]`); all crates use `version.workspace = true`. Qt About and Flutter About both read `CARGO_PKG_VERSION` from their adapter crate, so they follow automatically. The Flutter `pubspec.yaml` versionName mirrors the workspace version; the `+N` suffix is Android-only (`versionCode`) and increments on every shipped APK/AAB, independently of the versionName. The native Android `android/app/build.gradle.kts` `versionName` mirrors the workspace version too, and its `versionCode` mirrors the Flutter `+N` number (own `applicationId`, so the tracks are independent — the numbers just move together for comparability). Bump order: workspace version (+ the three workspace entries in `Cargo.lock`: `mailcore`/`mailapp`/`mailffi`, nothing else) → pubspec versionName → +N → native `versionName` + native `versionCode` (= +N). Never bump per-crate.
- Android launcher icons are generated, never hand-drawn: SVG masters plus rendered PNGs live in `flutter/assets/icon-src/` (brand blue `#3B82F6`); the `flutter_launcher_icons` section of `flutter/pubspec.yaml` selects adaptive background/foreground/monochrome and the output goes to `android/app/src/main/res/`. Regenerate with `dart run flutter_launcher_icons` from `flutter/` after touching the sources. Never list `icon-src` under `flutter: assets:` — build-time sources must not ship inside the app bundle.
- New feature (Qt and native Android): build it **core-first**, in this order.
  1. Write the `mailcore` API: logic, decisions, error/edge cases, and its tests.
  2. Expose it through the thin adapters (`mailapp` bridge, `mailffi`'s JNI
     externs in `src/android.rs` + `MailNative.kt`). No FRB API or codegen
     for Flutter is needed. An adapter only translates types. It takes explicit ids
     (account, `folder_id`, uid) from the caller and never fills in its own
     "current" state, so the same call does the same thing from either
     frontend.
  3. Write the two UIs last.

  Multi-step operations ("make sure the bytes are cached, then build and
  write the file", "is this row safe to delete") are one `mailcore` call, not
  a sequence each adapter assembles itself. If you notice you are writing the
  same `if`/loop/format in QML and Kotlin, or in `mailapp` and `mailffi`, stop
  and move it down.
- Tests: `cargo test --workspace` (plus `flutter test` in `flutter/` when Flutter changed). QML smoke: `qml6 qml/Main.qml` or `qmllint qml/*.qml` if no display.
- Debugging crashes on Omarchy: load the `diagnose-crash` skill path (systemd-coredump) — do not guess.
- Desktop integration files live in `resources/` (`.desktop`, icons). Install script links them; do not hardcode `$HOME` in code — use `directories`.

## 6. Security & Privacy

- HTML mail is untrusted: render in WebEngine with remote content blocked by default; no JS bridge into Rust except an explicit allow-list.
- Never log message bodies, passwords, or tokens. Log IDs and subjects at `debug` at most.
- Network: TLS required by default; plaintext IMAP/SMTP only with explicit per-account opt-in.
- Test sending is allowlist-only: automated sends (harness, workers, tests)
  are refused for any recipient outside `MAILCLIENT_TEST_SEND_ALLOWLIST`
  (unset = deny all, configured locally via gitignored `.env`, never
  committed). `MAILCLIENT_ALLOW_ANY_RECIPIENT=1` is a harness-only escape
  hatch — never export it globally. An interactive Send click in the composer is explicit user
  consent (`SendPolicy::Unrestricted`). Never add ad-hoc bypasses.
- **Privacy (hard rule): never write or comment any real account or mail
  information.** No real addresses, credentials, hosts, passwords, subjects,
   bodies, or sender/recipient data in docs, comments, tests, examples, or
   commit messages — nowhere that could be committed. Test fixtures use
   `@example.com` / `@example.org` (RFC 2606) only. Real values live solely in
   the local gitignored `.env`, the OS keyring (app-private vault file on
   Android), and the gitignored `flutter/android/key.properties` and `android/key.properties`.

## 7. Definition of Done (per step)

Verify only what the change can affect — check `git diff --stat` first.
Changes that cannot alter compiled code or runtime behaviour need no test or
build runs: docs (`*.md`), ignore files, comment-only edits, and local-only
gitignored config (`.env`, `flutter/android/key.properties`,
`android/key.properties`, `local.properties`). State that verification was
skipped and why instead of running suites "just in case". Anything else gets
the matching check: Rust → item 1, QML → item 2, Dart → `flutter analyze` +
`flutter test`, Kotlin/Compose → the affected `./build.sh --android` (or
`installDebug` on a device), build
scripts / manifests / Gradle / dependencies → the affected `./build.sh`
target (`--qt` / `--flutter` / `--apk` / `--android`).

1. `cargo fmt --check`, `cargo clippy -p mailcore -- -D warnings`, `cargo test -p mailcore` green.
2. `scripts/qml-check.sh` green on touched QML (lint gate + headless QML tests + format check; or noted as skipped headless with reason — the format check skips itself without the pinned qmlformat). Qt/WebEngine enum and API names verified against the installed headers or Qt docs — QML misspellings of them fail silently.
3. The affected `./build.sh` target produces a runnable bundle in `dist/` (`--qt` → `dist/mailclient/bin/mailapp`, `--flutter` → `dist/mailclient-flutter/`, `--apk` → `dist/mailclient-apk/`, `--android` → `dist/mailclient-android/`).
4. `PROJECT.md` status table updated; no secrets/binaries/`dist/` staged.
5. Cross-frontend features get a duplication check: read both adapters' new
   functions and both UIs' new code side by side. Do they show any logic
   beyond toolkit code (widgets, gestures, layout, theme lookups)? Then that
   logic moves into `mailcore` (§1, §5 core-first). Any deliberate exception
   is added to `SHARED-CORE.md`.
