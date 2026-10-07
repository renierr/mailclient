# Native Android frontend (Kotlin + Jetpack Compose)

The Android client. Native Kotlin + Compose over `mailcore` through JNI
(`MailNative` ↔ `crates/mailffi/src/android.rs`); background checks, push
and notifications run natively too. It is kept feature-for-feature in step
with the Qt desktop client (AGENTS.md §1, PROJECT.md §9); the Flutter
Android app is retired.

## Layout

```
android/
  settings.gradle.kts / build.gradle.kts / gradle.properties
  gradlew{,.bat} + gradle/wrapper/   (jar is gitignored, like flutter/android/)
  key.properties                     (gitignored release keystore, same format as flutter's)
  local.properties                   (gitignored: sdk.dir on machines without ANDROID_HOME)
  app/
    build.gradle.kts                 (pins: AGP/Kotlin/SDK/NDK/Compose BOM)
    mailffi.gradle.kts               (cargo-ndk builds libmailffi.so into jniLibs/)
    src/main/
      AndroidManifest.xml
      kotlin/de/renier/mailclient/
        MainActivity.kt              (the one activity: Compose shell, notification taps)
        MailNative.kt + Mail*.kt     (core JNI, background, push, notify)
        JobEvents.kt                 (process-lifetime job-event fan-out)
        ui/
          MailApp.kt                 (root: theme around the shell)
          shell/                     (route stack, search bar, status strip)
          state/MailState.kt         (what the shell shows, over JNI)
          folders/ list/ reader/     (panes: tree + manager, list, reader)
          composer/                  (WYSIWYG editor page, fields, attachments)
          accounts/ contacts/        (account manager + setup form, contacts)
          settings/ outbox/          (settings pages, outbox list)
          common/                    (avatar and other shared pieces)
          theme/Theme.kt             (Material You, brand blue #3B82F6 below Android 12)
      res/                           (launcher icons, vector icons, FileProvider paths)
```

## Prerequisites (nothing here installs them for you)

- JDK 17, the Android SDK, and NDK **28.2.13676358** (pinned in
  `app/build.gradle.kts`; `ANDROID_NDK_HOME` wins, else `<sdk>/ndk/<pin>`).
- `cargo install cargo-ndk` plus the `aarch64 / armv7 / x86_64` Rust targets
  (see `app/mailffi.gradle.kts`).
- `key.properties` next to `settings.gradle.kts` for signed releases
  (same four keys as `flutter/android/key.properties`:
  `storeFile`/`storePassword`/`keyAlias`/`keyPassword`). Without it Gradle
  signs with the public debug key and `./build.sh --android` refuses.

## Build & run

```sh
./scripts/android-dev.sh --run         # emulator + install + launch
./scripts/android-dev.sh --seed-db --run  # + copy data/dev.sqlite in first (offline reads)
./scripts/android-dev.sh --run --log   # + tail logcat for the app
./scripts/android-dev.sh --build       # debug build only, no device needed
./scripts/android-dev.sh --dist        # signed release APK → dist/mailclient-android/
./scripts/android-dev.sh --emulator    # just boot the emulator and wait
```

Bare `./scripts/android-dev.sh` prints its help and runs nothing. When no
device is online the device tasks ask whether to boot the resolved AVD
(`--yes` answers yes). The native app is standalone: it shares nothing
with the Flutter embedding at runtime, so there is no data import — a fresh
install has no accounts until the account-setup screen lands.

`android-dev.sh` works on Linux and MSYS2/Windows alike. Machine config
(`ANDROID_SDK_ROOT` / `ANDROID_AVD` / `JAVA_HOME` env, or gitignored
`android/local.properties` with `sdk.dir=` / `avd.name=`) never touches git,
so checkouts on different machines just differ locally. Raw form:
`cd android && ./gradlew installDebug` on a device/emulator.

The Rust core builds as part of the Gradle build (debug keeps arm64 +
x86_64 only, so the loop stays fast).

## Hard constraints

- **Namespace `de.renier.mailclient` never changes.** The Rust JNI exports
  (`Java_de_renier_mailclient_MailNative_*`) encode it. The `applicationId`
  is free — `de.renier.mailclient.native`, so it installs next to the
  retired Flutter app.
- Behaviour changes belong in `mailcore` (AGENTS.md core-first rule); this
  app only translates ids and renders. New screens that need core data add
  `MailNative` externs in `crates/mailffi/src/android.rs` next to the reader
  ones, never a second source of truth.
- New dependencies (beyond the pinned Compose BOM / WorkManager / core-ktx
  already here) need user approval, same as crates and pubspec.

## Parity with Qt

Every screen exists; `PLAN.md` is the build history. What still differs
from the Qt client is tracked in PROJECT.md §9 — a feature that lands in
one of the two lands in the other.
