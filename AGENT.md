# AGENT.md — Coding Agent Rules & Boundaries

This file is normative for all coding agents (human or AI) working in this repo.
`PROJECT.md` describes _what_ we build; this file describes _how_ we work.

## 1. Stack & Targets

- Primary OS: **Omarchy Linux (Arch-based, "quattro")**, Wayland, Qt **6.11+**.
- Backend: **Rust (stable, edition 2021)**, workspace in `crates/`.
  - `mailcore`: pure-Rust library — SQLite storage, models, IMAP/SMTP sync engine. **No Qt dependency.**
  - `mailapp`: thin Qt/QML bridge binary using **cxx-qt 0.9** + `mailcore`. Only crate allowed to depend on Qt.
- Frontend: **QML (QtQuick + QtQuick.Controls)**, single source in
  `crates/mailapp/qml/`, embedded via the `Mailclient` QML module
  (`CxxQtBuilder::new_qml_module`). HTML mail rendered via `QtWebEngine`.
- Storage: **SQLite** via `rusqlite` (bundled). One DB file per user: `~/.local/share/mailclient/mailclient.sqlite`.
- Mail protocols: IMAP (`imap` / `async-imap` + TLS), SMTP send (`lettre`), MIME parse/build (`mail-parser`, `mail-builder`). Future protocols (POP3/JMAP/EWS/Graph) must go behind traits in `mailcore::sync`.

## 2. Boundaries — what agents MUST / MUST NOT do

### MUST
- Keep `mailcore` UI-free and unit-testable. All DB access goes through `mailcore::db` / `store::*`.
- Evolve the SQLite schema **only via versioned migrations** in `crates/mailcore/src/db/migrations.rs` + `schema.sql`. Never edit a released migration in place; add a new one. Bump `SCHEMA_VERSION`.
- Keep passwords/secrets **out of SQLite and git**. DB stores only `auth_vault_key`; actual secrets live in the OS keyring (`keyring` crate) or memory.
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test -p mailcore` before finishing a backend change. Run `qmllint`/`qmlformat` (from `/usr/lib/qt6/bin`) on changed QML when available (`qmllint` needs `-I` for the built `Mailclient` import; unresolved-`Mailclient` warnings are expected pre-build).
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
- Do **not** invent new top-level directories without updating this file and `PROJECT.md`.

## 3. Code Style

- Rust: `rustfmt` defaults, `clippy` clean, `thiserror` for errors, `serde` for JSON fields, `chrono` (UTC/RFC3339) for times, `log` + `env_logger` for logging. Async runtime: `tokio` (single global choice).
- SQL: lowercase keywords, `snake_case` tables/columns, explicit `FOREIGN KEY … ON DELETE CASCADE`, indexes for every `(account_id, folder_id, uid)`-style lookup and for date-descending list queries. Every table has `created_at`/`updated_at` (UTC ISO8601 text) unless it is a pure FTS/virtual table.
- QML: one component per file in `crates/mailapp/qml/`, `PascalCase.qml` filenames, `qmllint`-clean, no inline JS business logic beyond formatting. All user-visible strings ready for `qsTr()`. Rust objects reach QML via `#[qml_element]` in the `Mailclient` module — never duplicate QML outside the crate.
- No emojis in code unless the user asks. Concise comments only.

## 4. Dependency Policy

Allowed without asking (pinned in `Cargo.toml`):
`rusqlite`, `tokio`, `thiserror`, `anyhow` (binaries only), `serde`/`serde_json`, `chrono`, `uuid`, `log`/`env_logger`, `imap`, `async-imap`, `tokio-rustls`/`native-tls`, `lettre`, `mail-parser`, `mail-builder`, `keyring`, `directories`, `cxx-qt`/`cxx-qt-lib`/`cxx-qt-build`, `cxx`.
Anything else (new crypto, new runtime, new Qt modules beyond Core/Gui/Qml/Quick/QuickControls2/Network/WebEngine) → ask first.

## 5. Workflows

- Build: `./scripts/build.sh` (release bundle into `dist/`). Dev loop: `./scripts/dev.sh`. Install locally: `./scripts/install-local.sh` (`~/.local`). Never hand-roll `cargo build` output paths in docs; point to the scripts.
- Tests: `cargo test --workspace`. QML smoke: `qml6 qml/Main.qml` or `qmllint qml/*.qml` if no display.
- Debugging crashes on Omarchy: load the `diagnose-crash` skill path (systemd-coredump) — do not guess.
- Desktop integration files live in `resources/` (`.desktop`, icons). Install script links them; do not hardcode `$HOME` in code — use `directories`.

## 6. Security & Privacy

- HTML mail is untrusted: render in WebEngine with remote content blocked by default; no JS bridge into Rust except an explicit allow-list.
- Never log message bodies, passwords, or tokens. Log IDs and subjects at `debug` at most.
- Network: TLS required by default; plaintext IMAP/SMTP only with explicit per-account opt-in.
- Test sending is allowlist-only: `mailcore` refuses any recipient outside
  `MAILCLIENT_TEST_SEND_ALLOWLIST` (unset = deny all, configured locally via
  gitignored `.env`, never committed). Real sending requires
  `MAILCLIENT_ALLOW_ANY_RECIPIENT=1`. Never add ad-hoc bypasses.
- **Privacy (hard rule): never write or comment any real account or mail
  information.** No real addresses, credentials, hosts, passwords, subjects,
  bodies, or sender/recipient data in docs, comments, tests, examples, or
  commit messages — nowhere that could be committed. Test fixtures use
  `@example.com` / `@example.org` (RFC 2606) only. Real values live solely in
  the local gitignored `.env` and the OS keyring.

## 7. Definition of Done (per step)

1. `cargo fmt --check`, `cargo clippy -p mailcore -- -D warnings`, `cargo test -p mailcore` green.
2. `qmllint` clean on touched QML (or noted as skipped headless with reason).
3. `./scripts/build.sh` produces a runnable `dist/mailclient/bin/mailapp` (or current milestone binary).
4. `PROJECT.md` status table updated; no secrets/binaries/`dist/` staged.
