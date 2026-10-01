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

On Windows, from PowerShell, pass the Windows twin of the config explicitly
(the tool compares paths textually, so they need backslashes there):

```powershell
flutter_rust_bridge_codegen generate --config-file '\\?\D:\<repo>\flutter_rust_bridge.windows.yaml'
```

The config lives in `flutter_rust_bridge.yaml` at the repository root rather
than here: the tool joins the config file's directory onto the paths without
normalising, so a leading `..` never matches the Rust root it computes. Keep
it and `flutter_rust_bridge.windows.yaml` in sync.

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

### Background checks on Android

Three schedulers, chosen in Settings → Accounts & sync and stored in the
shared `background_scheduler` Rust setting (default `workmanager`, which Qt
never reads or writes). Every account can override the interval and
whether it uses push (`mailcore::store::account_settings`), so the core
plans what runs (`sync::background::schedule::plan`): the push service
for the push accounts, and one poller — the alarm when the app-wide method
is `alarm`, otherwise WorkManager — ticking at the shortest interval among
the rest. Each tick only syncs the accounts that are due, so a 60-minute
account is not checked at a 15-minute one's cadence. `MailSchedule.kt`
starts what the plan names and stops the others; with every account on
Manually nothing runs. Per-account push is the way out for servers that
send IDLE heartbeats (`* OK Still here`) every few minutes: each one wakes
the radio and CPU, so such an account is cheaper polled. The IDLE loop
measures the heartbeat gap (`IdleStats`, wall clock) and stores it per
account (`account_settings::record_idle_heartbeats`); when it is shorter
than the keep-alive alarm, the account's push setting shows a hint
suggesting polling.

- **Battery-saving (WorkManager, default).** A periodic worker, deferrable
  by design: in Doze it only runs in maintenance windows, so notifications
  may wait for unlock.
- **On-time alarm.** A self-rearming exact one-shot
  (`setExactAndAllowWhileIdle`, `MailAlarm.kt`) that fires in standby and
  honours 5/10-minute intervals, at the cost of a wakeup per check. Its
  receiver hands the check to WorkManager as *expedited* work (Android
  12+), which Doze does not defer the way it defers plain jobs. The alarm
  re-arms natively after every shot, a reboot or an app update.
- **Push (IMAP IDLE).** `MailPushService.kt` keeps `mailcore::sync::push`
  running in a foreground service (type `specialUse`: `dataSync` is capped
  at a few hours a day on recent Android). One connection per account
  waits in IDLE on the inbox; when the server announces a change, that
  account syncs its inbox over the same session — no reconnect, no TLS
  handshake — and the notification goes out within seconds. Between
  arrivals the CPU sleeps: the service holds a wake lock only while the
  monitor reports busy. Only accounts that use push get a connection; an
  account switched away from push is dropped on the next keep-alive or
  settings change. Tokio's timers stand still in suspend, so a
  keep-alive alarm (`MailPush.kt`, every 15 minutes) re-issues each IDLE,
  keeping the connection and the carrier's NAT mapping alive, retries
  accounts that are backing off, and restarts the service if Android
  killed it. A change of the default network reconnects every account at
  once. A server without IDLE gets its check on every keep-alive instead.
  Android requires a notification for a foreground service; it sits in its
  own "Mail monitor" channel at minimum importance (no status-bar icon, no
  sound), and switching that channel off in the system settings hides it
  without stopping push.

The alarm scheduler and the push keep-alive need `SCHEDULE_EXACT_ALARM`
(Android 12+; denied by default since 14 — Settings sends the user to
"Alarms & reminders"); ungranted, they are armed inexact via
AllowWhileIdle and still fire, just not at the exact minute.

None of the three starts a Flutter engine. The worker, the alarm and the
push service call the Rust core directly over JNI (`MailNative.kt` ↔
`crates/mailffi/src/android.rs`, the same `libmailffi.so` and database the
Dart side uses). What the notification says and does is decided in
`mailcore::sync::background::notify`; `MailNotifier.kt` only reads back
what is on screen, posts the plan and commits it. Taps reach Dart through
the `mailclient/background_power` channel (`openPayload`,
`takeLaunchPayload`), and new mail found while the app is open reloads the
list (`mailChanged`) instead of alerting. Release builds keep the
JNI-only callbacks through `android/app/proguard-rules.pro`.

Either way, the battery-optimisation exemption matters most: without it
Android withholds network access in Doze. Vendor battery savers (Samsung,
Xiaomi, …) stop apps on their own terms and need the app set to
unrestricted there as well.

There is one new-mail notification, replaced on every post. It lists all
unread inbox mail since the user last had the app open (`pending` in the
report), alerts only for mail new since the previous check, is updated
quietly or removed when that mail gets read elsewhere, and is cleared when
the app comes to the foreground. Opening the app also marks the cache as
seen (`background_mark_seen`), and resuming it reloads the list and syncs
unless auto-sync is off or a sync just ran. Settings keeps the last ten
checks (scheduler, result, what happened to the notification) under
"Recent checks".

Opening a mail makes no network request. Inline (`cid:`) images are part
of the message: sync keeps their bytes, and the core embeds them as
`data:` URIs before the body reaches either frontend. Mail synced before
that shows their alt text and a Download banner, which fetches the parts
from the user's own server on tap. Remote images stay blocked unless
allowed, and the WebView's Safe Browsing and metrics are off in the
manifest.

Both frontends pick one of three paints per HTML mail (`ui/reader/mail_paint.dart`,
`paintMode` in `MessageView.qml`). A mail that sets no colours of its own
takes the app theme, like plain text. A designed mail (the core reports
`html_colored`) keeps the light sheet it was made for in a light theme; in a
dark theme it is inverted as a whole (`invert(1) hue-rotate(180deg)`, hues
kept) with images inverted back, and a reader toggle shows the original
colours for that message.

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

Nothing known. The composer (send, drafts) lives once in `mailcore::compose`,
account saving in `mailcore::store::account_form`, and the IMAP session pool,
the lease and the panic guard in `mailcore::sync::pool`. Add an entry here
before making any new copy between `mailapp` and `mailffi`.
