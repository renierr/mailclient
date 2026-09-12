# Mailclient

Full-featured desktop mail client for **Omarchy Linux** (Windows later):
Rust backend + SQLite cache, QML frontend.

- Multiple IMAP/SMTP accounts, folder trees, background sync
- HTML + text mail, rich-text composer, attachments, drafts
- Offline-first SQLite cache with full-text search

Details: [`PROJECT.md`](PROJECT.md) (goal, architecture, roadmap) and
[`AGENT.md`](AGENT.md) (agent rules, dependency policy, Definition of Done).

## Quick start

```sh
./dev.sh                # debug build + run (uses ./crates/mailapp/qml live)
/build.sh              # release bundle -> dist/mailclient/
/scripts/install-local.sh  # install to ~/.local (+ .desktop entry)

cargo test -p mailcore      # backend unit tests
qmllint crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml  # QML lint (qmllint in /usr/lib/qt6/bin)
```

Prerequisites (already present on Omarchy): Rust stable, CMake, Qt 6
(base + declarative + webengine). `scripts/*.sh` set `QMAKE` automatically;
if you build by hand: `QMAKE=/usr/lib/qt6/bin/qmake cargo build -p mailapp`.

### Building on Windows

Linux stays the primary target; Windows is a supported dev build. The build
script itself is OS-agnostic -- cxx-qt only ever needs `QMAKE` (or a `qmake` on
PATH), so the platform knowledge lives in the wrapper scripts:

```powershell
.\scripts\dev.ps1     # debug build + run against .\crates\mailapp\qml
.\scripts\build.ps1   # release bundle -> dist\mailclient\ (+ windeployqt)
```

`scripts\qt-env.ps1` (dot-sourced by both) resolves Qt in this order: `QMAKE`,
`qmake.exe` on PATH, then the newest matching kit under `QT_ROOT_DIR`, `QTDIR`,
`C:\Qt`, `D:\Qt`, `%USERPROFILE%\Qt`. It also prepends Qt's `bin\` to PATH,
which Windows needs to load the Qt DLLs at runtime.

Needed once:

- **MSVC build tools** + Windows SDK (the C++ workload) -- cxx-qt compiles C++.
- **Qt 6** with Quick, QuickControls2, Network and WebEngineQuick, in a kit
  whose ABI matches the Rust host toolchain: `msvc*_64` for the default
  `x86_64-pc-windows-msvc`. A MinGW Qt cannot be linked into an MSVC build.
  Either the Qt Online Installer (kit "MSVC 2022 64-bit" + "Qt WebEngine") or
  `pip install --user aqtinstall` and
  `aqt install-qt windows desktop <version> win64_msvc2022_64 -m qtwebengine qtwebchannel qtpositioning -O D:\Qt`.

  No admin rights are required for either: Qt is a self-contained tree, so any
  user-writable target directory works (aqt just unpacks archives; the online
  installer needs a Qt account but not elevation unless you point it at
  `Program Files`). Pick the target with disk space -- base plus WebEngine is
  several GB -- and either name it so discovery finds it (`C:\Qt`, `D:\Qt`,
  `%USERPROFILE%\Qt`) or set `QT_ROOT_DIR` / `QMAKE`.

`Could not find Qt installation: Could not find Qt` from the `cxx-qt` build
script means exactly this: no Qt was found. Nothing in the build script can
substitute for installing it.

Platform differences that are already handled in-tree: the OS keyring backend
is selected per target in `crates/mailcore/Cargo.toml` (Secret Service on
Linux, Credential Manager on Windows, Keychain on macOS), and app data paths
come from the `directories` crate, so on Windows the SQLite cache lands under
`%APPDATA%` instead of `~/.local/share`.

## Layout

```text
crates/mailcore/   pure-Rust core: db, models, store, sync, search
crates/mailapp/    cxx-qt bridge binary (Qt models + main) + qml/ UI
resources/         .desktop entry + icon
scripts/           helpers: install-local.sh, qt-env.sh (+ *.ps1 for Windows)
dist/              gitignored build output
```

DB lives at `~/.local/share/mailclient/mailclient.sqlite`
(override with `MAILCLIENT_DB`); passwords live in the OS keyring, never in git.

## Deliverability: DKIM and DMARC

Mailclient submits mail to the SMTP server configured for the account; it does
not hold or use a DKIM private key. Configure the FEBAS SMTP submission server
for the same domain as the account email address and have FEBAS sign outgoing
mail there. Keeping the private key on the provider server prevents it from
being exposed on the desktop machine.

The composer and sending backend only permit a `From:` address in the account
domain. This preserves SPF and DKIM domain alignment for DMARC. Mailclient also
uses that domain in its SMTP `EHLO` name to avoid dotless-hostname spam signals.

After saving the account with FEBAS's supplied SMTP host, port, encryption, and
login, send a message to Gmail and use **Show original** to confirm all of:

- `SPF: PASS`
- `DKIM: PASS` with `d=your-domain.example`
- `DMARC: PASS`

If DKIM is absent, fails, or has FEBAS's domain in `d=`, contact FEBAS: the
provider must install and use the private key for your domain. Your published
DMARC DNS record applies automatically and needs no client-side configuration.
