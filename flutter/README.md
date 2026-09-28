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
    models/
      models.dart               accounts, folders, messages, contacts
      settings.dart             preferences, search hits, header details
    state/mail_state.dart       selection, search, multi-select, and routing
                                job events back into what is on screen
    theme/app_theme.dart        themes and layout breakpoints
    ui/
      shell/                    three / two / one pane by window width,
                                search field, menus, shortcuts
      sidebar/                  accounts and folder tree
      message_list/             the compact list feed, selection + bulk bar,
                                sort menu, search results
      reader/                   message view, HTML rendering, attachments,
                                reply/forward/headers
      accounts/                 account setup + accounts manager
      composer/                 plain-text composer (reply/forward/drafts)
      settings/                 all preferences + About/capabilities
      contacts/                 contacts manager
      folders/                  folder manager
      move_to/                  move picker
cmake/mailffi.cmake             desktop: builds and bundles the core
android/app/mailffi.gradle.kts  Android: cargo-ndk into jniLibs
test/                           model decoding tests
```

File dialogs go through `file_picker` (pick, save, save-all) and `open_filex`
(open in the system viewer). On Linux those need zenity, kdialog or qarma —
without one the action reports it on the status line instead of failing
silently.

## Building

From the repository root:

```sh
./dev.sh --flutter   # debug loop: flutter run -d linux against ./data/dev.sqlite
./build.sh --flutter # release bundle → dist/mailclient-flutter/
./build.sh --apk     # signed Android APK → dist/mailclient-apk/ (see Android below)
```

`flutter run -d windows` and `flutter run -d linux` build the Rust core as
part of the CMake build and drop the shared library into the bundle — nothing
extra to run first. Local runs use `./data/dev.sqlite` via `MAILCLIENT_DB`
(the same file `./dev.sh` gives the Qt frontend), never the real mailbox.

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

The renderer depends on the platform:

- **Android** uses the system WebView through `webview_flutter`
  (`ui/reader/mail_web_view.dart`). JavaScript, file and content access are
  off, every navigation is stopped and handed to the link handler, and the
  document carries a Content-Security-Policy that allows no network load
  except remote images the user allowed for that message.
- **Linux and Windows** use the pure-Dart `flutter_widget_from_html_core`,
  since `webview_flutter` has no backend for either. The body is built as a
  sliver list and kept while its inputs are unchanged, so a header toggle or
  link hover does not rebuild the mail.

Both frontends paint HTML mail on a light sheet in every theme: the
sanitizer keeps the sender's inline styles and colours, which assume a
white background.

## Android

`./build.sh --apk` produces a signed release APK in
`dist/mailclient-apk/mailclient-release.apk`. The Gradle build compiles the
Rust core for `arm64-v8a`, `armeabi-v7a` and `x86_64` via cargo-ndk
(`android/app/mailffi.gradle.kts`) before packaging.

Needed once:

- the Android NDK (SDK Manager → SDK Tools → NDK), `cargo install cargo-ndk`,
  and the Rust targets (`aarch64-linux-android`,
  `armv7-linux-androideabi`, `x86_64-linux-android`);
- a release keystore plus `flutter/android/key.properties` (gitignored) with
  `storeFile`, `storePassword`, `keyAlias` and `keyPassword`. Without it the
  release build falls back to the debug signing config — `build.sh` refuses
  to build in that case, because a debug-signed "release" APK is exactly what
  Play Protect flags as harmful.

Installing the APK sideloads it: it is self-signed with your local key, has
no Play Store reputation, and Play Protect blocks it with a generic
"harmful app" warning on first install. That verdict is about the unknown
signature, not the code — the manifest requests only `INTERNET` (plus
`ACCESS_NETWORK_STATE`), and "Trotzdem installieren" is safe for a build
from this repo. Only a Play Store listing (Play App Signing plus reputation)
removes the warning. If a previous install was signed with a *different* key
(debug vs release), uninstall it first: Android refuses the update with a
signature mismatch, which is separate from the Play Protect dialog.

The NDK is located as `ANDROID_NDK_HOME` first, otherwise
`<sdk.dir>/ndk/<flutter.ndkVersion>` — the same version
`ndkVersion = flutter.ndkVersion` pins in `build.gradle.kts` (handed to the
mailffi script as an extra property). No machine-specific path is baked in.

Two things differ from desktop. TLS is rustls-only (`lettre` with
`rustls-tls`, `tokio-rustls` with `ring`): there is no OpenSSL in the
dependency tree, which is what makes the Android cross-compile possible. And
secrets do not use the OS keyring: on Android `mailcore::auth` keeps them in
an app-private `auth_vault.json` inside the data directory `init_app`
receives (`set_vault_dir`) — deliberately *not* the Android Keystore, a file
the sandbox already protects with no platform-channel round trip. Storage is
the same SQLite file in that directory.

## Shared code still to promote

The composer form (`api/composer.rs` mirrors `mailapp`'s composer) is a
near-copy. It belongs in `mailcore`; that is the one place where a fix will
otherwise have to be made twice. Account saving already lives once, in
`mailcore::store::account_form`.

Done before: the IMAP session pool, the lease, and the panic guard lived in
both bridges and now live once in `mailcore::sync::pool`.
