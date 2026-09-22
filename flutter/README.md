# Flutter frontend

An alternative UI for mailclient that replaces Qt/QML with Flutter, over the
same Rust core. It is a peer of `crates/mailapp`, not a replacement: both
frontends sit on `crates/mailcore`, and on a desktop where both are installed
they open the same database file.

```text
flutter/            Dart app (this directory)
   │  dart:ffi, in process — no subprocess, no socket, no IPC
   ▼
crates/mailffi/     cdylib: flutter_rust_bridge glue over mailcore
   ▼
crates/mailcore/    db / store / sync / queue  ── shared with crates/mailapp
```

## Why the layers look like this

**In-process, not out-of-process.** The Dart side loads `mailffi.dll` /
`libmailffi.so` and calls into it directly. A call is a function call across
the FFI boundary; there is no serialisation round-trip to another process and
no CLI to shell out to.

**`mailffi` holds no mail logic.** Every function in `crates/mailffi/src/api/`
translates arguments into a `mailcore` call and its result into something Dart
can hold. Anything that needs a decision belongs in `mailcore`, where both
frontends get it.

**Nothing blocks the UI isolate.** SQLite reads answer inline (fast enough,
and flutter_rust_bridge runs them on a worker pool anyway). Everything that
touches IMAP or SMTP is queued onto one `mailclient-net` thread inside Rust
and reported back on a single event stream — the same shape `mailapp` uses,
with a Dart `Stream<JobEvent>` in place of the Qt signal.

**Dart owns the selection.** This is the one real divergence from the Qt
bridge. `mailapp` keeps the current account and folder inside the `Bridge`
QObject and pushes rebuilt JSON feeds into QML properties, which forces it to
reconcile a job that finishes after the user has moved on. Here every read
takes ids explicitly and a finished job only reports *what* changed, so
`MailState` re-reads whatever it is actually showing and stale jobs refresh
nothing. That also means `mailffi` has no `busy` latch: jobs are deduped per
kind, so a running sync does not refuse an attachment download.

**Big payloads cross as JSON.** Folder rows, message rows and reader payloads
are produced by `mailcore::feed`, which is what the Qt frontend consumes too.
Mirroring every field through the FFI type system a second time would give two
definitions that can disagree. Small structural values (`JobEvent`, `AppInfo`,
`Selection`) cross as generated structs, where a typed value beats a parse.

## Layout

```text
lib/
  main.dart                     loads the core, then starts the UI
  src/
    app.dart                    providers, theme, lifecycle
    ffi/
      mail_core.dart            the seam: library loading, JSON → models
      generated/                flutter_rust_bridge output (committed)
    models/models.dart          Dart views of mailcore's JSON
    state/mail_state.dart       selection, and routing job events back into it
    theme/app_theme.dart        themes and layout breakpoints
    ui/
      shell/                    three / two / one pane by window width
      sidebar/                  accounts and folder tree
      message_list/             the compact list feed
      reader/                   message view, HTML rendering, attachments
      accounts/                 account setup dialog
cmake/mailffi.cmake             desktop: builds and bundles the core
android/app/mailffi.gradle.kts  Android: cargo-ndk into jniLibs
test/                           model decoding tests
```

## Building

`flutter run -d windows` and `flutter run -d linux` build the Rust core as
part of the CMake build and drop the shared library into the bundle — nothing
extra to run first.

The core is built optimised even for a debug Flutter build: it is not the code
being debugged, and an unoptimised bundled SQLite, rustls and MIME parser make
sync slow enough to change how the app behaves. Pass
`-DMAILFFI_DEBUG=ON` through CMake when the Rust itself needs stepping
through.

### Regenerating the FFI glue

Both generated files are committed, so a plain `cargo build` and a plain
`flutter run` work without the codegen tool. After changing anything in
`crates/mailffi/src/api/`, from the repository root:

```sh
flutter_rust_bridge_codegen generate
```

The config lives in `flutter_rust_bridge.yaml` at the repository root rather
than here: the tool joins the config file's directory onto the paths without
normalising, so a leading `..` never matches the Rust root it computes.

## Reading HTML mail

`mailcore::html` sanitizes every body before it reaches Dart, with remote
images stripped unless the user asked for them. `MailHtmlView` renders that
and nothing else — it never fetches, never executes, and hands link taps back
to the platform rather than following them.

It currently uses the pure-Dart `flutter_widget_from_html_core` on every
platform. The obvious alternative, `flutter_inappwebview`, ships backends for
android / ios / macos / web / windows and **not Linux**, which is this
project's primary OS, so it cannot be the single answer. A real engine renders
table-heavy marketing mail better; if that becomes the deciding factor, the
swap is confined to `ui/reader/mail_html_view.dart` and should be
platform-conditional rather than wholesale.

## Android

The Gradle wiring is in place, but **the core does not compile for Android
yet**. `mailcore::auth` uses the `keyring` crate unconditionally, while
`crates/mailcore/Cargo.toml` only depends on it for Linux, Windows and macOS —
so an Android build has no keyring backend to compile against. Android has no
Secret Service either; the equivalent is the Android Keystore, reached through
a platform channel or a Rust binding, which is a `mailcore` design decision
rather than a build fix.

Beyond that, an Android build needs the NDK, `cargo install cargo-ndk`, and
the Rust targets (`aarch64-linux-android`, `armv7-linux-androideabi`,
`x86_64-linux-android`). Storage is already handled: `init_app` takes a data
directory and the Dart side passes the app's private support directory there.

## Shared code still to promote

`crates/mailffi/src/session.rs` is a near-copy of
`crates/mailapp/src/bridge/session.rs` — the IMAP session pool, the lease, and
the panic guard around background jobs. The logic is Qt-free and just lives on
the wrong side of the Qt boundary today. It belongs in `mailcore::sync`, with
both copies deleted, once the Flutter frontend is far enough along that
changing `mailapp` is worth the risk.

The same is true of the account-form handling (`api/accounts.rs::save_account`
mirrors `mailapp`'s `add_account`) and the composer form. Those are the three
places where a fix will otherwise have to be made twice.
