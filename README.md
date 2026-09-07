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
./scripts/dev.sh            # debug build + run (uses ./qml live)
./scripts/build.sh          # release bundle -> dist/mailclient/
./scripts/install-local.sh  # install to ~/.local (+ .desktop entry)

cargo test -p mailcore      # backend unit tests
qmllint qml/*.qml qml/components/*.qml  # QML lint (qmllint in /usr/lib/qt6/bin)
```

Prerequisites (already present on Omarchy): Rust stable, CMake, Qt 6
(base + declarative + webengine). `scripts/*.sh` set `QMAKE` automatically;
if you build by hand: `QMAKE=/usr/lib/qt6/bin/qmake cargo build -p mailapp`.

## Layout

```text
crates/mailcore/   pure-Rust core: db, models, store, sync, search
crates/mailapp/    cxx-qt bridge binary (Qt models + main)
qml/               QtQuick UI (3-pane shell, composer, dialogs)
resources/         .desktop entry + icon
scripts/           build.sh / dev.sh / install-local.sh
dist/              gitignored build output
```

DB lives at `~/.local/share/mailclient/mailclient.sqlite`
(override with `MAILCLIENT_DB`); passwords live in the OS keyring, never in git.
