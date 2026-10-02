/// Loading the native library and the one place JSON turns into models.
///
/// Everything above this file works with [Account], [Folder] and friends and
/// never sees a JSON string or a generated binding. The generated code under
/// `generated/` is the raw surface; this is the seam.
library;

import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart'
    show AnyhowException;
import 'package:path_provider/path_provider.dart';

import '../models/account_settings.dart';
import '../models/models.dart';
import '../models/settings.dart';
import 'generated/api/accounts.dart' as rust_accounts;
import 'generated/api/attachments.dart' as rust_attachments;
import 'generated/api/composer.dart' as rust_composer;
import 'generated/api/contacts.dart' as rust_contacts;
import 'generated/api/events.dart' as rust_events;
import 'generated/api/folders.dart' as rust_folders;
import 'generated/api/init.dart' as rust_init;
import 'generated/api/maintenance.dart' as rust_maintenance;
import 'generated/api/messages.dart' as rust_messages;
import 'generated/api/mutate.dart' as rust_mutate;
import 'generated/api/search.dart' as rust_search;
import 'generated/api/settings.dart' as rust_settings;
import 'generated/api/sync.dart' as rust_sync;
import 'generated/frb_generated.dart';

// Imported as well as re-exported: `export` alone publishes the names to
// importers of this library without binding them inside it.
import 'generated/api/events.dart' show JobEvent;
import 'generated/api/folders.dart' show FolderCounts;
import 'generated/api/init.dart' show AppInfo;
import 'generated/api/messages.dart' show LinkInfo;
import 'generated/api/reader.dart' as rust_reader;
import 'generated/api/reader.dart'
    show ReaderPaint, ReaderPalette, ReaderDocumentOptions;
import 'generated/api/search.dart' show SearchPlan;

export 'generated/api/accounts.dart' show Selection;
export 'generated/api/events.dart' show JobEvent, JobPhase;
export 'generated/api/folders.dart' show FolderCounts, OlderState;
export 'generated/api/init.dart' show AppInfo;
export 'generated/api/messages.dart' show LinkInfo;
export 'generated/api/mutate.dart' show Hit, MoveResult;
export 'generated/api/reader.dart'
    show ReaderPaint, ReaderPalette, ReaderDocumentOptions;
export 'generated/api/search.dart' show SearchMode, SearchPlan;

/// Status-line text for an error from the core: the message alone, without
/// the Rust backtrace flutter_rust_bridge appends to an `anyhow` error.
String coreErrorText(Object e) {
  final text = switch (e) {
    AnyhowException(:final message) => message,
    Exception() => e.toString().replaceFirst('Exception: ', ''),
    _ => '$e',
  };
  final cut = text.indexOf('Stack backtrace:');
  return (cut < 0 ? text : text.substring(0, cut)).trim();
}

/// The mail core, in process.
///
/// One instance for the app's lifetime. Calls run on flutter_rust_bridge's
/// worker pool, so even the SQLite reads here never block the UI isolate, and
/// anything that touches the network is queued inside Rust and answered on
/// [jobEvents] instead of being awaited.
class MailCore {
  MailCore._(this.info);

  /// Where the database ended up, and which core version opened it.
  final AppInfo info;

  static MailCore? _instance;

  /// Stand in a fake core for widget tests that build panes reading
  /// [instance] directly.
  @visibleForTesting
  static set debugInstance(MailCore core) => _instance = core;

  /// The loaded core. Throws if [load] has not completed — which is a bug in
  /// startup order, not a condition to handle.
  static MailCore get instance {
    final i = _instance;
    if (i == null) {
      throw StateError('MailCore.load() must complete before the UI starts');
    }
    return i;
  }

  /// Load the shared library, open the database and run migrations.
  ///
  /// Idempotent, because a Flutter hot restart re-runs `main` against a
  /// native library that is still loaded and a database that is still open.
  static Future<MailCore> load() async {
    if (_instance != null) return _instance!;
    await MailCoreApi.init();
    // Desktop lets `mailcore` pick its own platform path, deliberately the
    // same file the Qt frontend uses. Android has no such path, so it gets
    // the app's private support directory.
    final dataDir = Platform.isAndroid || Platform.isIOS
        ? (await getApplicationSupportDirectory()).path
        : null;
    final info = await rust_init.initApp(dataDir: dataDir);
    return _instance = MailCore._(info);
  }

  /// Drop pooled IMAP sessions. Call when the app is going away for good.
  Future<void> shutdown() => rust_init.shutdown();

  /// Everything that finishes on the network thread.
  ///
  /// Subscribe once, at the top of the app: a second subscription replaces
  /// the first on the Rust side rather than fanning out.
  Stream<JobEvent> jobEvents() => rust_events.jobEvents();

  // --- accounts ------------------------------------------------------------

  Future<List<Account>> accounts() async =>
      _decodeList(await rust_accounts.accountsJson(), Account.fromJson);

  /// The account and folder to open on, restored from the last session.
  /// Either id is `-1` when there is nothing to select.
  Future<rust_accounts.Selection> initialSelection() =>
      rust_accounts.initialSelection();

  Future<rust_accounts.Selection> selectAccount(int id) =>
      rust_accounts.selectAccount(id: id);

  /// The edit dialog's form. Never contains a password.
  Future<Map<String, dynamic>> accountForm(int id) async =>
      _decodeMap(await rust_accounts.accountForm(id: id));

  /// A new account form's starting values and the security choices
  /// (`security_choices`), from the core.
  Map<String, dynamic> accountFormDefaults() =>
      jsonDecode(rust_accounts.accountFormDefaults()) as Map<String, dynamic>;

  /// Host and user guesses for a typed address; empty while it is partial.
  Map<String, dynamic> accountGuess(String email) =>
      jsonDecode(rust_accounts.accountGuess(email: email))
          as Map<String, dynamic>;

  /// The port field after `protocol`'s (`imap`/`smtp`) security changed.
  String accountPortForSecurity(
    String protocol,
    String oldSec,
    String newSec,
    String port,
  ) => rust_accounts.accountPortForSecurity(
    protocol: protocol,
    oldSec: oldSec,
    newSec: newSec,
    port: port,
  );

  /// Per-field errors and warnings for the account form — the check
  /// [saveAccount] runs too.
  ({Map<String, String> errors, Map<String, String> warnings}) accountFormCheck(
    Map<String, dynamic> form, {
    required bool editing,
  }) {
    final raw = jsonDecode(
      rust_accounts.accountFormCheck(form: jsonEncode(form), editing: editing),
    ) as Map<String, dynamic>;
    Map<String, String> strings(Object? m) => {
      for (final e in ((m as Map<String, dynamic>?) ?? const {}).entries)
        e.key: e.value as String,
    };
    return (errors: strings(raw['errors']), warnings: strings(raw['warnings']));
  }

  /// Create or update an account; returns its id. Keyed by email address, so
  /// re-saving a known address edits rather than duplicates.
  Future<int> saveAccount(Map<String, dynamic> form) =>
      rust_accounts.saveAccount(form: jsonEncode(form));

  /// Delete an account; returns the account to show instead, or `-1`.
  Future<int> deleteAccount(int id) => rust_accounts.deleteAccount(id: id);

  // --- folders -------------------------------------------------------------

  Future<List<Folder>> folders(int accountId) async => _decodeList(
    await rust_folders.foldersJson(accountId: accountId),
    Folder.fromJson,
  );

  Future<String> folderPath(int folderId) =>
      rust_folders.folderPath(folderId: folderId);

  Future<FolderCounts> folderCounts(int folderId) =>
      rust_folders.folderCounts(folderId: folderId);

  /// Resolve a folder path to its local id, for a UI that navigated by path
  /// (search results carry paths, reads take ids).
  Future<int> folderIdForPath(int accountId, String path) async =>
      (await rust_folders.folderIdForPath(
        accountId: accountId,
        path: path,
      )).toInt();

  Future<void> setFolderSubscribed(int folderId, bool subscribed) =>
      rust_folders.setFolderSubscribed(
        folderId: folderId,
        subscribed: subscribed,
      );

  // --- messages ------------------------------------------------------------

  Future<List<MessageSummary>> messages(
    int folderId, {
    int limit = 200,
    int offset = 0,
  }) async => _decodeList(
    await rust_messages.messagesJson(
      folderId: folderId,
      limit: limit,
      offset: offset,
    ),
    MessageSummary.fromJson,
  );

  Future<MessageBody> message(int folderId, int uid) async =>
      MessageBody.fromJson(
        await _decodeMap(
          await rust_messages.messageJson(folderId: folderId, uid: uid),
        ),
      );

  /// Re-sanitized HTML with remote images kept — the "show once" path.
  Future<String> messageHtmlWithRemoteImages(int folderId, int uid) =>
      rust_messages.messageHtml(
        folderId: folderId,
        uid: uid,
        allowRemote: true,
      );

  Future<MessageHeaders> messageHeaders(int folderId, int uid) async =>
      MessageHeaders.fromJson(
        await _decodeMap(
          await rust_messages.headersJson(folderId: folderId, uid: uid),
        ),
      );

  Future<void> markRead(int accountId, int folderId, int uid, bool read) =>
      rust_messages.markRead(
        accountId: accountId,
        folderId: folderId,
        uid: uid,
        read: read,
      );

  /// Returns how many rows actually changed. Rust counts these as `u64`,
  /// which crosses as a `BigInt`; a folder never holds enough messages for
  /// that to matter, so the UI gets a plain int.
  Future<int> markReadMany(
    int accountId,
    int folderId,
    List<int> uids,
    bool read,
  ) async => (await rust_messages.markReadMany(
    accountId: accountId,
    folderId: folderId,
    uids: uids,
    read: read,
  )).toInt();

  Future<bool> toggleStar(int accountId, int folderId, int uid) => rust_messages
      .toggleStar(accountId: accountId, folderId: folderId, uid: uid);

  Future<int> setStarMany(
    int accountId,
    int folderId,
    List<int> uids,
    bool starred,
  ) async => (await rust_messages.setStarMany(
    accountId: accountId,
    folderId: folderId,
    uids: uids,
    starred: starred,
  )).toInt();

  // --- search hits across folders (`mailcore::bulk`) -----------------------

  Future<int> markReadHits(
    int accountId,
    List<rust_mutate.Hit> hits,
    bool read,
  ) async => (await rust_messages.markReadHits(
    accountId: accountId,
    hits: hits,
    read: read,
  )).toInt();

  Future<int> setStarHits(
    int accountId,
    List<rust_mutate.Hit> hits,
    bool starred,
  ) async => (await rust_messages.setStarHits(
    accountId: accountId,
    hits: hits,
    starred: starred,
  )).toInt();

  /// One Undo for every folder that goes to Trash; the destroying shares
  /// start one purge job (`purging`).
  Future<rust_mutate.MoveResult> deleteHits(
    int accountId,
    List<rust_mutate.Hit> hits,
  ) => rust_mutate.deleteHits(accountId: accountId, hits: hits);

  Future<rust_mutate.MoveResult> archiveHits(
    int accountId,
    List<rust_mutate.Hit> hits,
  ) => rust_mutate.archiveHits(accountId: accountId, hits: hits);

  Future<rust_mutate.MoveResult> moveHits(
    int accountId,
    List<rust_mutate.Hit> hits,
    String destPath,
  ) => rust_mutate.moveHits(
    accountId: accountId,
    hits: hits,
    destPath: destPath,
  );

  /// Destroy hits across folders in one job.
  Future<void> purgeHits(int accountId, List<rust_mutate.Hit> hits) =>
      rust_mutate.purgeHits(accountId: accountId, hits: hits);

  // --- moving and deleting (undoable, or queued when destroying) ---------------------------------------

  Future<rust_mutate.MoveResult> deleteMessages(
    int accountId,
    int folderId,
    List<int> uids,
  ) => rust_mutate.deleteMessages(
    accountId: accountId,
    folderId: folderId,
    uids: uids,
  );

  Future<void> purgeMessages(int accountId, int folderId, List<int> uids) =>
      rust_mutate.purgeMessages(
        accountId: accountId,
        folderId: folderId,
        uids: uids,
      );

  Future<rust_mutate.MoveResult> archiveMessages(
    int accountId,
    int folderId,
    List<int> uids,
  ) => rust_mutate.archiveMessages(
    accountId: accountId,
    folderId: folderId,
    uids: uids,
  );

  Future<rust_mutate.MoveResult> moveMessages(
    int accountId,
    int folderId,
    List<int> uids,
    String destPath,
  ) => rust_mutate.moveMessages(
    accountId: accountId,
    folderId: folderId,
    uids: uids,
    destPath: destPath,
  );

  /// Take back a queued delete/archive/move; returns the status line text.
  Future<String> undoMove(String batch) => rust_mutate.undoMove(batch: batch);

  /// Seconds a delete/archive/move stays undoable.
  int undoGraceSecs() => rust_mutate.undoGraceSecs();

  Future<void> createFolder(int accountId, String path) =>
      rust_mutate.createFolder(accountId: accountId, path: path);

  // --- sync (queued) -------------------------------------------------------

  Future<void> syncAccount(int accountId) =>
      rust_sync.syncAccount(accountId: accountId);

  Future<void> syncFolder(int accountId, int folderId) =>
      rust_sync.syncFolder(accountId: accountId, folderId: folderId);

  Future<void> loadOlderMessages(int accountId, int folderId) =>
      rust_sync.loadOlderMessages(accountId: accountId, folderId: folderId);

  Future<void> refreshFolders(int accountId) =>
      rust_sync.refreshFolders(accountId: accountId);

  Future<void> refreshServerCapabilities(int accountId) =>
      rust_sync.refreshServerCapabilities(accountId: accountId);

  /// The user has the app open: background checks neither alert for nor
  /// list what the inbox cache holds now.
  Future<void> backgroundMarkSeen() => rust_sync.backgroundMarkSeen();

  /// Recent background ticks, newest first (`started_at`, `finished_at`,
  /// `trigger`, `skipped`, `new`, `errors`, `outcome`).
  Future<List<Map<String, dynamic>>> backgroundRunHistory() async =>
      (jsonDecode(await rust_sync.backgroundRunHistory()) as List<dynamic>)
          .whereType<Map<String, dynamic>>()
          .toList(growable: false);

  // --- search --------------------------------------------------------------

  /// Local FTS only, newest first, at most [SearchPlan.hitLimit] hits.
  /// Cheap enough to run on every keystroke.
  Future<List<SearchHit>> search(
    int accountId,
    String query, {
    String folder = '',
  }) async => _decodeList(
    await rust_search.searchJson(
      accountId: accountId,
      query: query,
      folder: folder,
    ),
    SearchHit.fromJson,
  );

  // --- reader document (`mailcore::html::reader`) -------------------------

  /// The paint for one HTML mail.
  ReaderPaint readerPaint(bool colored, bool dark, bool keepOriginal) =>
      rust_reader.readerPaint(
        colored: colored,
        dark: dark,
        keepOriginal: keepOriginal,
      );

  /// The colours a page is written in, for [paint] and the theme's colours.
  ReaderPalette readerPalette(ReaderPaint paint, ReaderPalette theme) =>
      rust_reader.readerPalette(paint: paint, theme: theme);

  /// Pages narrower than this loosen the mail's fixed widths; 0 when it
  /// has none. Once per mail.
  int readerFitBelow(String body) => rust_reader.readerFitBelow(body: body);

  /// The body as [paint] shows it, for the desktop widget renderer.
  String readerBody(String body, ReaderPaint paint, {bool fit = false}) =>
      rust_reader.readerBody(body: body, paint: paint, fit: fit);

  /// The full document for a web view: CSP, base CSS, spacer, body.
  String readerDocument(String body, ReaderDocumentOptions options) =>
      rust_reader.readerDocument(body: body, options: options);

  /// A link split for the examine dialog, and whether it may be opened at
  /// all (`mailcore::html::link_info`).
  LinkInfo linkInfo(String url) => rust_messages.linkInfo(url: url);

  /// How the search field runs `query` (`mailcore::search::plan`).
  SearchPlan searchPlan(String query) => rust_search.searchPlan(query: query);

  /// The short-input filter over one list row
  /// (`mailcore::search::filter_matches`).
  bool searchFilterMatches(String query, MessageSummary m) =>
      rust_search.searchFilterMatches(
        query: query,
        subject: m.subject,
        from: m.from,
        fromName: m.fromName,
        snippet: m.snippet,
      );

  /// Top up thin local results from the server. Queued; re-run [search] when
  /// the `"Search"` job finishes.
  Future<void> searchServer(
    int accountId,
    String query, {
    String folder = '',
  }) => rust_search.searchServer(
    accountId: accountId,
    query: query,
    folder: folder,
  );

  // --- composer ------------------------------------------------------------

  /// Validate, build and queue a message. Throws on anything the user can
  /// still fix, with the composer still open.
  Future<void> sendMail(
    int accountId,
    int folderId,
    Map<String, dynamic> form,
  ) => rust_composer.sendMail(
    accountId: accountId,
    folderId: folderId,
    form: jsonEncode(form),
  );

  Future<void> saveDraft(int accountId, Map<String, dynamic> form) =>
      rust_composer.saveDraft(accountId: accountId, form: jsonEncode(form));

  /// An image file as a `data:` URL the composer can send inline.
  Future<String> imageDataUrl(String path) =>
      rust_composer.imageDataUrl(path: path);

  /// Whether a file would be offered as an inline image (by type).
  bool isInlineImage(String path) => rust_composer.isInlineImage(path: path);

  /// An address split for the From field: the editable local part and the
  /// locked domain with its `@` (`compose::sender_parts`).
  ({String local, String domain}) senderParts(String address) {
    final p = rust_composer.senderParts(address: address);
    return (local: p.local, domain: p.domain);
  }

  /// The address a From field sends as: `local` on the account's domain, or
  /// the account address when blank (`compose::effective_from`).
  String effectiveFrom(String local, String accountEmail) =>
      rust_composer.effectiveFrom(local: local, accountEmail: accountEmail);

  /// Reply (`reply`, `reply_all`) or `forward` draft for one message.
  Future<AnswerDraft> answerDraft(int folderId, int uid, String mode) async =>
      AnswerDraft.fromJson(
        await _decodeMap(
          await rust_composer.answerDraft(
            folderId: folderId,
            uid: uid,
            mode: mode,
          ),
        ),
      );

  /// New-mail draft: the signature alone.
  Future<AnswerDraft> blankDraft() async =>
      AnswerDraft.fromJson(await _decodeMap(await rust_composer.blankDraft()));

  Future<Map<String, dynamic>> draftForm(int accountId, int uid) async =>
      _decodeMap(await rust_composer.draftForm(accountId: accountId, uid: uid));

  Future<void> deleteDraft(int accountId, int uid) =>
      rust_composer.deleteDraft(accountId: accountId, uid: uid);

  // --- attachments ---------------------------------------------------------

  /// Cached bytes, or `null` when they have not been downloaded yet.
  Future<List<int>?> attachmentBytes(int attachmentId) =>
      rust_attachments.cachedAttachmentBytes(attachmentId: attachmentId);

  Future<void> downloadAttachments(int accountId, int folderId, int uid) =>
      rust_attachments.downloadAttachments(
        accountId: accountId,
        folderId: folderId,
        uid: uid,
      );

  Future<String> saveAttachmentTo(int attachmentId, String path) =>
      rust_attachments.saveAttachmentTo(attachmentId: attachmentId, path: path);

  /// Write the copy a system viewer opens into [dir]; returns its path.
  Future<String> writeAttachmentCopy(int attachmentId, String dir) =>
      rust_attachments.writeAttachmentCopy(
        attachmentId: attachmentId,
        dir: dir,
      );

  /// Write every non-inline attachment of a message into `dir`.
  /// Returns how many files were written.
  Future<int> saveAllAttachmentsTo(int folderId, int uid, String dir) async =>
      (await rust_attachments.saveAllAttachmentsTo(
        folderId: folderId,
        uid: uid,
        dir: dir,
      )).toInt();

  // --- maintenance ---------------------------------------------------------

  /// Local storage statistics (database/message/cached/temp sizes).
  Future<Map<String, dynamic>> storageStats(
    String dbPath,
    String tempDir,
  ) async => _decodeMap(
    await rust_maintenance.storageStatsJson(dbPath: dbPath, tempDir: tempDir),
  );

  /// Delete every staged viewer copy plus stale draft dirs.
  /// Returns what went away, including the status line to show.
  Future<Map<String, dynamic>> cleanupTempFiles(String tempDir) async =>
      _decodeMap(await rust_maintenance.cleanupTempFilesJson(tempDir: tempDir));

  /// Delete cached messages past the newest N per folder (local-only).
  /// Returns how many rows went away.
  Future<int> trimLocalCache() async =>
      (await rust_maintenance.trimLocalCache()).toInt();

  /// Status line for a trim count, worded once in the core.
  Future<String> trimStatus(int removed) =>
      rust_maintenance.trimStatus(removed: BigInt.from(removed));

  /// Drop cached attachment bytes, keeping names and sizes.
  /// Returns what went away, including the status line to show.
  Future<Map<String, dynamic>> evictCachedAttachments() async =>
      _decodeMap(await rust_maintenance.evictCachedAttachmentsJson());

  /// Write a consistent snapshot of the database into `dir`.
  /// Returns where it went.
  Future<String> exportDatabaseTo(String dir) =>
      rust_maintenance.exportDatabaseTo(path: dir);

  // --- contacts ------------------------------------------------------------

  Future<List<Contact>> contacts({String prefix = ''}) async => _decodeList(
    await rust_contacts.contactsJson(prefix: prefix),
    Contact.fromJson,
  );

  Future<void> setContactAlias(String address, String alias) =>
      rust_contacts.setContactAlias(address: address, alias: alias);

  Future<void> deleteContact(String address) =>
      rust_contacts.deleteContact(address: address);

  // --- settings ------------------------------------------------------------

  Future<AppSettings> settings() async => AppSettings.fromJson(
    await _decodeMap(await rust_settings.settingsJson()),
  );

  /// Every preference's default and offered values
  /// (`mailcore::store::settings::choices`). Fixed for the app's lifetime.
  late final SettingChoices settingChoices = SettingChoices.fromJson(
    jsonDecode(rust_settings.settingChoicesJson()) as Map<String, dynamic>,
  );

  /// A quiet-hours time as hour and minute, read the way the core reads it;
  /// null when it does not read as one.
  ({int hour, int minute})? quietTime(String text) {
    final t = rust_settings.quietTime(text: text);
    return t == null ? null : (hour: t.hour, minute: t.minute);
  }

  /// The stored form (`HH:MM`) of a picked time.
  String quietTimeAt(int hour, int minute) =>
      rust_settings.quietTimeAt(hour: hour, minute: minute) ?? '';

  Future<void> setSetting(String key, String value) =>
      rust_settings.setSetting(key: key, value: value);

  /// Several settings in one transaction: all apply or none do.
  Future<void> setSettings(Map<String, String> values) =>
      rust_settings.setSettings(values: values);

  Future<void> setSort(String field, bool descending) =>
      rust_settings.setSort(field: field, descending: descending);

  Future<AccountSettings> accountSettings(int accountId) async =>
      AccountSettings.fromJson(
        await _decodeMap(
          await rust_settings.accountSettingsJson(accountId: accountId),
        ),
      );

  /// Several of one account's overrides at once; an empty value inherits
  /// the app-wide setting again.
  Future<void> setAccountSettings(int accountId, Map<String, String> values) =>
      rust_settings.setAccountSettings(accountId: accountId, values: values);

  Future<BackgroundPlan> backgroundPlan() async => BackgroundPlan.fromJson(
    await _decodeMap(await rust_settings.backgroundPlanJson()),
  );
}

// The core hands back JSON it built itself, so a decode failure means the two
// sides disagree about a contract — an assertion, not a user-facing error.

List<T> _decodeList<T>(String raw, T Function(Map<String, dynamic>) fromJson) =>
    (jsonDecode(raw) as List<dynamic>)
        .whereType<Map<String, dynamic>>()
        .map(fromJson)
        .toList(growable: false);

Future<Map<String, dynamic>> _decodeMap(String raw) async =>
    jsonDecode(raw) as Map<String, dynamic>;
