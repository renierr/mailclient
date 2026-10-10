# egui desktop client — investigation notes & spike spec

Goal under discussion: replace the Qt/QML desktop frontend with an
egui/eframe frontend (same stack as WordCraft), keeping `mailcore` and the
native Android client untouched. No decision taken yet; this file records
what was examined and the proposed path forward.

Status: investigation only. No code changed, nothing committed.

## 1. Repo map (as found)

- Workspace `Cargo.toml`: crates `mailcore`, `mailapp`, `mailffi`;
  workspace version `0.11.0`, edition 2021, release `lto = "fat"`.
- `crates/mailcore` — pure-Rust library: SQLite storage (`rusqlite`,
  bundled), models, IMAP/SMTP sync engine. No Qt dependency.
  DB: `~/.local/share/mailclient/mailclient.sqlite`, schema via versioned
  migrations. Protocols: `imap-next` + `imap-types`, async tokio +
  `tokio-rustls` (rustls-only, no OpenSSL — Android cross-compile depends
  on it); SMTP/MIME build via `lettre`, incoming MIME via `mail-parser`.
- `crates/mailapp` — thin Qt/QML bridge binary (`cxx-qt 0.9`) + QML in
  `crates/mailapp/qml/`. Only crate allowed to depend on Qt.
  Networking never on the GUI thread: dedicated `mailclient-net` thread
  with a current-thread Tokio runtime, GUI hears back via `CxxQtThread`.
- `crates/mailffi` — `cdylib` for Android frontends: JNI (`src/android.rs`,
  `jni` crate) + `flutter_rust_bridge 2`. Translation layer only.
- Frontends: **Qt/QML desktop** (Linux primary + Windows) and **native
  Android** (Kotlin + Jetpack Compose, 74 `.kt` files, package
  `de.renier.mailclient`) are active and kept at feature parity (UI may
  differ). **Flutter is retired** (builds, no new features).
  `kotlin-desktop/` contains only Compose build artifacts — no sources,
  ignore it.
- House rules (`AGENTS.md`): core-first features (mailcore API → thin
  adapters → two UIs); no business logic in QML (view-only, `qsTr()`
  strings, one component per file); shared logic built once in `mailcore`
  (`SHARED-CORE.md` tracks duplication); `PROJECT.md` tracks milestones.

## 2. QML port surface (~12.6k lines total)

Biggest files (lines):

| File | Lines | What it is |
|---|---|---|
| `Main.qml` | 1995 | shell, toolbar, panes, status |
| `MessageList.qml` | 1596 | virtualized message list |
| `MessageView.qml` | 1476 | reader: header + WebEngine body |
| `Settings.qml` | 1363 | settings forms |
| `Composer.qml` | 1100 | compose, WebEngine-backed `EditorFrame` |
| `Contacts.qml` | 564 | contacts |
| `AccountSetup.qml` | 396 | account wizard |
| `Sidebar.qml` | 349 | folders/accounts sidebar |
| `components/AppDialog.qml` | 341 | resizable dialogs, geometry memory |
| `Accounts.qml` | 228 | account management |
| others (`Folders`, `Outbox`, `MoveTo`, `AccountSetup`, components, `Theme`, `Icons`, `FeedJson`, `ModelSync`) | ~2.7k | |

HTML mail is rendered in **QtWebEngine** (reader + composer `EditorFrame`),
remote content blocked by default, no JS bridge except an allow-list.

## 3. Bridge & feed: the seam an egui client would use

- `crates/mailapp/src/bridge.rs` (1169 lines) + per-area modules
  (`accounts`, `capabilities`, `composer`, `folders`, `messages` (+ subdir),
  `outbox`, `settings`, `sync`, `worker` — ≈3.5k+ lines): translates core
  types for QML over cxx-qt. Adapters take explicit ids, never fill in
  "current" state.
- `crates/mailcore/src/feed.rs` (992 + 1289 tests): JSON feeds for QML,
  exposed as strings, `JSON.parse`d into `ListModels`.
- An egui frontend would **not** need the JSON string round-trip: same
  process, same language — call `mailcore` (`store::*`, `feed` shaping,
  `html`) directly. The bridge's translation decisions (what fields a view
  needs) are reusable as a spec even though cxx-qt itself is dropped.
- Threading gets simpler than today's `CxxQtThread` pattern: background
  jobs post to `mpsc` channels, UI polls + `request_repaint()`; tokio can
  run as a normal multi-thread runtime instead of the current-thread
  workaround that Qt's GUI thread forces.

## 4. `mailcore::html`: what the core already does

Pipeline (`crates/mailcore/src/html/`, std only, DoS caps
`MAX_HTML_BYTES` 512k in / 768k out):

`tags` (tokenize + allow-list) → `entities` → `urls` (which
`href`/`src` survive) → `css` (which inline styles survive) → `inline`
(`cid:` → message's own image bytes) → `sanitize` (walk + serialise) →
`text` (HTML↔plain, "needs HTML?" heuristics) → `reader` (document
assembly for both frontends).

Sanitizer guarantees (why a renderer parser stays small): ~55 allowed
tags (`tags.rs`), ~50 CSS properties (`css.rs`), normalized output
(`prop:value;`, `name="…"`), text escaped so `<` always opens a tag, no
scripts/style blocks/forms/positioning/`url()`, widths clamped (≤1200px,
≤100%), `had_remote` flag when a remote image was stripped (drives the
"show once" banner).

`reader/` output shape — **HTML strings, no block/widget model**:

- `Paint` (`Theme` / `Original` / `Darkened`) + `paint_for(colored, dark,
  keep_original)`; dark mails are color-rewritten up front (no CSS filter,
  so no per-frame re-render stutter; images keep real colors).
- `Palette` (`paper/ink/link/quote/rule`) + `palette()` + `LIGHT` sheet —
  maps 1:1 onto egui colors. `has_own_colors()` picks themed vs. designed
  rendering.
- `body(sanitized, paint, fit)` — sanitized HTML string with widths
  loosened + colors dark-rewritten. **Directly consumable by an egui
  renderer; dark mode + narrow-fit come along for free.**
- `document()` — full page (CSP + base CSS + `#mc-top` header spacer) for
  web engines only; irrelevant to egui.
- `fit_below()` / `layout_width()` — narrow-layout threshold; reuse for
  responsive behavior. Consts: `PAGE_MARGIN_PX` 16, `BASE_FONT_PX` 14.

## 5. Renderer sizing (egui side)

Parser over the constrained subset: days-scale. All malformed input was
already handled upstream.

- **Straightforward**: inline marks, headings, paragraphs, `hr`, lists,
  `pre` (pre-wrap), `blockquote` (border in `rule`, text in `quote`),
  links (in `link`), alignment/indent — all native egui.
- **Moderate (the bulk)**: images (`data:` inline; remote gated behind the
  existing `had_remote` flow; `max-width:100%` scaling) and **tables**
  (nested `table/tr/td` + `bgcolor/valign`/clamped sizes +
  `border-collapse`) — newsletters are built from these; fidelity is won
  or lost here. `float`/`clear` approximated.
- **Deliberately lossy**: multi-column table layouts on narrow screens
  (render simplified), `display:none` preheaders (keep skipping), exotic
  `display` values. Accepted fidelity gap vs. WebEngine: reads perfectly,
  looks slightly plainer.

Rejected alternative: embedding a web view (Wry/WebKitGTK) just for the
reader — reintroduces a browser engine dependency, against the point of
the exercise and the dependency policy (large additions need approval).

## 6. Assessment

- **Fits well**: UI-free core, command-style API discipline, feed/bridge
  specs to build against, HTML sanitize+dark+fit already in core, single
  binary with no Qt 6.11 system dep / cxx-qt wiring / qmlformat pin dance /
  WebEngine bundle weight. Reference point: an eframe app starts in
  ~150 ms warm on this machine (measured on WordCraft, same stack,
  Wayland-native).
- **Two product decisions, not technical ones**: (a) newsletter pixel
  fidelity — subset rendering vs. WebEngine perfection; (b) the composer —
  `Composer.qml` + WebEngine `EditorFrame` is the hardest widget (egui
  `TextEdit` is plain-text at heart; rich compose needs a custom editor
  feeding the existing `lettre` send path).
- **Treat egui as a replacement for QML, not a third frontend** — the
  Qt↔native parity rule would otherwise become a three-way tax. Keep QML
  building until the egui client reaches named parity (same pattern as the
  retired Flutter), Android untouched throughout.
- **Watch**: accessibility depth (egui + accesskit is younger than Qt's;
  verify screen-reader/CJK early if those users matter), and the
  `mailto:`/tray/notification/desktop-integration pieces currently owned
  by Qt + `resources/` + install scripts.

## 7. Proposed sequencing

1. **Spike A — reader**: message list + reading pane against the real
   core; `reader::body` → prototype renderer → screenshots. Grade the
   top ~20 real newsletters for fidelity. Answers: renderer effort +
   whether the team likes immediate-mode ergonomics.
2. **Spike B — composer**: rich-editing approach (formatted send behind
   constrained editing vs. full rich editor). The estimate hangs on this.
3. **Go/no-go** on the full port only after A+B.
4. eframe-on-Wayland risk is already retired (proven on this machine).

## 8. Open items (not yet examined)

- How `MessageView.qml` consumes the feed end-to-end: banner flows
  (remote images, `had_remote`), header-overlay + `#mc-top` spacer scroll
  arrangement, scroller setup — needed to spec Spike A faithfully.
- `Composer.qml` / `EditorFrame.qml` editing mechanics — needed for
  Spike B.
- `SHARED-CORE.md` duplication list — check before designing the egui
  crate's core API usage so nothing gets tripled.
- If a new crate is created, `AGENTS.md`/`PROJECT.md` must be updated
  (no new top-level dirs without it).

## 9. Agent handoff guide (experiment branch)

You are an AI coding agent picking this up in a fresh checkout on a new
experiment branch (suggested name: `experiment/egui-desktop-spike`).
Everything below is binding for the experiment; `AGENTS.md` stays
normative wherever it does not contradict this section.

### 9.1 Start here (read order)

1. `AGENTS.md` — stack, boundaries, dependency policy, workflows.
2. `PROJECT.md` ("Where we stand") — current milestone context.
3. `SHARED-CORE.md` — what is still duplicated between frontends.
4. This file, sections 1–8 — the investigation this experiment builds on.

### 9.2 Branch and working rules

- Work on `experiment/egui-desktop-spike`, branched from `main`. Never
  push it and **never commit without being explicitly asked** (consent is
  per-action per `AGENTS.md` §2). End-of-session state is uncommitted
  changes + a written report.
- The spike lives in one new crate, `crates/maildesk-spike`. It is
  throwaway scaffolding: do not wire it into `build.sh`, `dist/`,
  version bumps, or desktop integration files. If it graduates to a real
  frontend, the `AGENTS.md`/`PROJECT.md` updates from §8 apply then —
  not during the experiment.
- `mailcore` is read-only during Spike A. If you find yourself wanting a
  core change, stop: record it in your report as a "core change request"
  with justification instead of making it.
- New dependencies need the same approval as anywhere else (`AGENTS.md`
  §4): `eframe`/`egui` (and only them, same major as WordCraft uses) are
  pre-approved for the spike crate. Anything beyond that → ask.

### 9.3 Safety rules (no exceptions)

- **Offline only.** Never contact a real mailbox, never use live
  credentials, never run the sync harness. Point the spike at the dev
  database (`MAILCLIENT_DB=./data/dev.sqlite`, cf. `./dev.sh`) or
  seeded fixtures — never `~/.local/share/mailclient/mailclient.sqlite`.
- **Privacy hard rule** (`AGENTS.md` §6): no real addresses, hosts,
  subjects, or bodies in code, tests, screenshots, or reports.
  Fixtures use `@example.com` / `@example.org` only. Screenshots in
  reports must show fixture data.
- No secrets in code or logs; no binaries/`dist/` output committed or
  left outside `target/`.

### 9.4 Spike A tasks (in order)

1. Resolve open item §8.1 first: read `MessageView.qml` (+ `FeedJson.qml`)
   and write down the reader contract — banner flows (`had_remote`),
   header arrangement, scroller setup. This is the fidelity baseline.
2. Scaffold `maildesk-spike`: `eframe` window opening the dev DB's
   folder list + message list (virtualized rows) for one account.
3. Feed `reader::body()` output for ~20 representative mails (plain,
   simple HTML, table newsletters, dark-mode) through a prototype
   renderer and screenshot each (headless via `ui_shot`-style harness
   or `ui.screenshot` equivalent — do not drive the GUI with keystrokes
   on this shared machine).
4. Grade every screenshot against the QML/WebEngine rendering:
   `match` / `readable-differs` / `broken`, with one line each on what
   differs. This table is the deliverable Spike B and the go/no-go
   depend on.

### 9.5 Spike B tasks (only after A reports)

1. Resolve open item §8.2: read `Composer.qml` / `EditorFrame.qml`,
   note what rich-editing operations the send path actually needs.
2. Prototype the smallest editor that serves the existing `lettre` send
   path and report where it falls short.

### 9.6 Verification and done

- Fast checks only: `cargo fmt --check`, `cargo clippy -p maildesk-spike
  -- -D warnings`, `cargo test -p maildesk-spike`. No full `./build.sh`
  (packaging proves nothing here). QML/Flutter/Android rows do not apply
  — say they were skipped and why.
- Definition of done: uncommitted spike crate + grading table + written
  report containing (a) renderer effort estimate, (b) composer approach
  recommendation, (c) core change requests if any, (d) go/no-go
  recommendation. Update nothing in `PROJECT.md` (not a milestone step).
