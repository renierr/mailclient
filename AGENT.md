# AGENT.md — Coding Agent Rules & Boundaries

This file is normative for all coding agents (human or AI) working in this repo.
`PROJECT.md` describes _what_ we build; this file describes _how_ we work.

## 1. Stack & Targets

- Primary OS: **Omarchy Linux (Arch-based, "quattro")**, Wayland, Qt **6.11+**.
- Backend: **Rust (stable, edition 2021)**, workspace in `crates/`.
  - `mailcore`: pure-Rust library — SQLite storage, models, IMAP/SMTP sync engine. **No Qt dependency.**
  - `mailapp`: thin Qt/QML bridge binary using **cxx-qt 0.9** + `mailcore`. Only crate allowed to depend on Qt.
  - `mailffi`: `cdylib` exposing `mailcore` to the Flutter frontend via
    **flutter_rust_bridge 2** (`dart:ffi`, in process). No Qt, no mail logic —
    a translation layer only, same rule as `mailapp`.
- Frontends: two, both over `mailcore`, neither authoritative over the other.
  A behaviour change belongs in `mailcore` so both get it; a change made in
  one frontend's adapter alone must be a deliberate, stated choice.
  - **Qt/QML** (`crates/mailapp/qml/`) — the mature one.
  - **Flutter** (`flutter/`) — Dart app over `mailffi`, targeting Linux and
    Windows desktop with Android planned. See `flutter/README.md`.
- Qt frontend: **QML (QtQuick + QtQuick.Controls)**, single source in
  `crates/mailapp/qml/`, embedded via the `Mailclient` QML module
  (`CxxQtBuilder::new_qml_module`). HTML mail rendered via `QtWebEngine`.
- Storage: **SQLite** via `rusqlite` (bundled). One DB file per user: `~/.local/share/mailclient/mailclient.sqlite`.
- Mail protocols: IMAP on `imap-next` + `imap-types`, async over `tokio` with `tokio-rustls` for TLS. SMTP send via `lettre`, which also builds the outgoing MIME; incoming MIME is parsed by `mail-parser`. Future protocols (POP3/JMAP/EWS/Graph) must go behind traits in `mailcore::sync`.

## 2. Boundaries — what agents MUST / MUST NOT do

### MUST
- Keep `mailcore` UI-free and unit-testable. All DB access goes through `mailcore::db` / `store::*`.
- Evolve the SQLite schema **only via versioned migrations** in `crates/mailcore/src/db/migrations.rs` + `schema.sql`. Never edit a released migration in place; add a new one. Bump `SCHEMA_VERSION`.
- Keep passwords/secrets **out of SQLite and git**. DB stores only `auth_vault_key`; actual secrets live in the OS keyring (`keyring` crate) or memory.
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test -p mailcore` before finishing a backend change. After changing anything in `crates/mailffi/src/api/`, re-run `flutter_rust_bridge_codegen generate` from the repo root and commit both generated files.
- Run `qmllint`/`qmlformat` (from `/usr/lib/qt6/bin`) on changed QML when available (`qmllint` needs `-I` for the built `Mailclient` import; unresolved-`Mailclient` warnings are expected pre-build).
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
- Do **not** add broad new external dependencies without justification. Prefer: std → small well-scoped crate → large framework. Large additions (new Qt modules, new async runtime, new DB) require user approval.
- Do **not** put business logic in QML. QML is view-only; logic lives in Rust and is exposed via explicit bridge types.
- Do **not** invent new top-level directories without updating this file and `PROJECT.md`. Current ones: `crates/`, `flutter/`, `qml` (inside `mailapp`), `resources/`, `scripts/`, `dist/` (gitignored).
- Do **not** let the two frontends drift. Before copying anything out of
  `mailapp` into `mailffi` (or back), check whether it belongs in `mailcore`
  instead. Where a copy already exists it is listed in `flutter/README.md`
  ("Shared code still to promote") — add to that list rather than quietly
  making a third.

## 3. Code Style

- Rust: `rustfmt` defaults, `clippy` clean, `thiserror` for errors, `serde` for JSON fields, `chrono` (UTC/RFC3339) for times, `log` + `env_logger` for logging.
- Networking is async and never runs on the Qt GUI thread. `mailapp` owns one
  dedicated `mailclient-net` thread holding a current-thread Tokio runtime, and
  every IMAP/SMTP job is queued onto it; the GUI hears back through
  `CxxQtThread`. Keep it that way — a blocking call on the GUI thread freezes
  the window, and a second runtime is a dependency decision (see §4).
- SQL: lowercase keywords, `snake_case` tables/columns, explicit `FOREIGN KEY … ON DELETE CASCADE`, indexes for every `(account_id, folder_id, uid)`-style lookup and for date-descending list queries. Every table has `created_at`/`updated_at` (UTC ISO8601 text) unless it is a pure FTS/virtual table.
- QML: one component per file in `crates/mailapp/qml/`, `PascalCase.qml` filenames, `qmllint`-clean, no inline JS business logic beyond formatting. All user-visible strings ready for `qsTr()`. Rust objects reach QML via `#[qml_element]` in the `Mailclient` module — never duplicate QML outside the crate.
- QML must stay responsive: dialogs are resizable (`AppDialog` with geometry memory) and windows vary in width, so every pane has to adapt instead of clipping. Rules: wrapping text gets `wrapMode` + a width bound (`Layout.fillWidth`); content inside a `ScrollView` binds its width to the ScrollView's own `availableWidth` via an explicit `id` (never `parent.availableWidth` — ScrollView reparents its children, so `parent` is not the ScrollView and the column falls back to its implicit width, which disables wrapping and pushes trailing controls off-screen); items in a `RowLayout` that must yield get `Layout.minimumWidth: 0` (e.g. a ComboBox next to a button); `Flow` only wraps when its own width is constrained. Verify resizable dialogs at narrow widths, not just the default size.
- No emojis in code unless the user asks. Concise comments only.

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
`flutter_rust_bridge` (pinned with `=`), `anyhow` and `android_logger`.

The `=` pin on `flutter_rust_bridge` is deliberate: the codegen tool, the Rust
crate and the Dart package must be the same version, so a range would let
`cargo update` silently desync them. Changing it means changing all three and
regenerating.

Dart packages are the same kind of decision as a crate, and `flutter/pubspec.yaml`
is their source of truth. Currently: `flutter_rust_bridge`, `provider`,
`path_provider`, `intl`, `flutter_widget_from_html_core`. Anything else → ask.

Anything else (new crypto, a second async runtime, new Qt modules beyond
Core/Gui/Qml/Quick/QuickControls2/Network/WebEngine) → ask first. Removing or
replacing a pinned dependency is the same kind of decision as adding one, so
it needs the same ask.

## 5. Workflows

- Build: `./build.sh` (Qt release bundle into `dist/`), `./build.sh --flutter`
  (Flutter release bundle into `dist/mailclient-flutter/`). Dev loop: `./dev.sh`
  (Qt) or `./dev.sh --flutter`. Both dev loops use `./data/dev.sqlite`
  (`MAILCLIENT_DB` overrides). Install locally: `./scripts/install-local.sh` (`~/.local`). Never hand-roll `cargo build` output paths in docs; point to the scripts.
- Flutter: `flutter run -d windows` / `-d linux` from `flutter/` (the Rust core builds as part of it). Regenerate FFI glue with `flutter_rust_bridge_codegen generate` from the repo root.
- Tests: `cargo test --workspace`, plus `flutter test` in `flutter/`. QML smoke: `qml6 qml/Main.qml` or `qmllint qml/*.qml` if no display.
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
  the local gitignored `.env` and the OS keyring.

## 7. Definition of Done (per step)

1. `cargo fmt --check`, `cargo clippy -p mailcore -- -D warnings`, `cargo test -p mailcore` green.
2. `qmllint` clean on touched QML (or noted as skipped headless with reason).
3. `./build.sh` produces a runnable `dist/mailclient/bin/mailapp` (or current milestone binary).
4. `PROJECT.md` status table updated; no secrets/binaries/`dist/` staged.
