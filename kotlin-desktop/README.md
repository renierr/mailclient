# kotlin-desktop — Kotlin Frontend (Compose Multiplatform, Desktop)

Desktop-Frontend neben der QML-App (`crates/mailapp`), gleiche SQLite-Daten:
Die Compose Multiplatform-UI kommuniziert über `mailfeed` (Qt-freie JSON-CLI
über `mailcore`, siehe `../crates/mailfeed/`) mit derselben SQLite-Datenbank.

## Starten

```sh
./kotlin-desktop/run.sh   # oder aus dem Root: ./dev-kotlin.sh
./kotlin-desktop/build.sh # nur bauen (cargo + gradle :composeApp:build)
```

**Lokale Entwicklung & DB-Isolation:**
- `.env` im Repo-Root wird automatisch geladen (`MAILCLIENT_DB=.../data/mailclient.sqlite`).
- `mailfeed`, `run.sh` und `CliMailRepository` garantieren, dass lokale Testläufe ausschließlich die Dev-DB in `data/` nutzen und niemals die Produktions-DB berühren.

## Architektur & Komponenten

```
kotlin-desktop/ (Compose Multiplatform Desktop)
  │
  │ [ProcessBuilder stdout/stdin JSON on Dispatchers.IO]
  ▼
crates/mailfeed/ (Rust CLI Tool)
  │
  │ [Direct Rust API calls]
  ▼
crates/mailcore/ (SQLite Cache, IMAP Sync Engine, SMTP Sender, Keyring)
  │
  ▼
data/mailclient.sqlite (Lokale Entwicklungsdatenbank via .env)
```

### Rollen der Dateien:
1. **`crates/mailcore/` (Rust Engine):**
   - SQLite Schema & CRUD (`db/`, `store/`).
   - IMAP-Synchronisation (`sync/imap/`).
   - SMTP-Versand (`sync/sender/`).
   - OS Keyring Anbindung (`auth.rs`).
2. **`crates/mailfeed/` (CLI Bridge):**
   - Befehle: `accounts`, `folders`, `messages`, `message`, `search`, `mark-read`, `mark-star`, `status`, `sync`, `send`, `delete`, `archive`, `open-attachment`.
   - Gibt maschinenlesbares JSON auf stdout aus.
3. **`composeApp/src/commonMain/` (Plattformunabhängiges UI):**
   - `models/MailModels.kt`: Typisierte Datenklassen für Accounts, Folders, Messages, Attachments.
   - `repo/MailRepository.kt`: Backend-Interface für alle Daten- und Netzwerkaktionen.
   - `ui/App.kt`: Haupt-Shell mit 3 Spalten (Sidebar, Liste, Reader) und Toolbar.
   - `ui/Sidebar.kt`: Ordnerbaum mit Unread-Pills und Rollensortierung.
   - `ui/MessageList.kt`: Nachrichtenliste mit Unread-Indikator, Star-Toggle, FTS-Suche.
   - `ui/MessageView.kt`: Nachrichtenleser mit Aktionsleiste (Reply, Forward, Star, Archive, Delete), Anhangliste, Headers-Dialog.
   - `ui/Dialogs.kt`: Composer-Dialog (Send, To/Cc/Bcc, Subject, Body) und Accounts-Dialog.
   - `ui/Theme.kt`: Farbschemata (Light/Dark), Design-Tokens.
4. **`composeApp/src/desktopMain/` (Desktop-spezifisch):**
   - `repo/CliMailRepository.kt`: Implementiert `MailRepository` durch Aufruf von `mailfeed`.
   - `Main.kt`: JVM Desktop Entry-Point und Fenstererstellung.

## Features & Stand
- **3-Pane Layout:** Responsive Sidebar, Nachrichtenliste, Leseansicht.
- **Synchronisation:** `⟳ Sync` Button triggert IMAP-Synchronisation über `mailfeed sync`.
- **Nachrichtenaktionen:**
  - `↩ Reply`, `↩↩ Reply All`, `↪ Forward` öffnen den Composer mit vorgefüllten Empfängern, Betreff und Zitat.
  - `★ Star / ☆ Unstar`: Sofortige lokale Umschaltung + Synchronisation.
  - `📁 Archive`: Verschiebt Nachrichten in den Archive-Ordner.
  - `🗑 Delete`: Verschiebt in den Papierkorb (Trash) oder löscht dauerhaft.
- **Composer & Versand:** Vollständig angebundener SMTP-Versand via `mailfeed send` inkl. Sent-Kopie.
- **Anhänge:** Metadatenanzeige + Klick zum Herunterladen und Öffnen im System-Standardbetrachter.
- **Suche:** Lokaler Ordnerfilter (<3 Zeichen) und Volltextsuche über alle Ordner (>=3 Zeichen).
