/// Loading the native library and the one place JSON turns into models.
///
/// Everything above this file works with [Account], [Folder] and friends and
/// never sees a JSON string or a generated binding. The generated code under
/// `generated/` is the raw surface; this is the seam.
library;

import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/models.dart';
import '../models/settings.dart';
import 'generated/api/accounts.dart' as rust_accounts;
import 'generated/api/attachments.dart' as rust_attachments;
import 'generated/api/composer.dart' as rust_composer;
import 'generated/api/contacts.dart' as rust_contacts;
import 'generated/api/events.dart' as rust_events;
import 'generated/api/folders.dart' as rust_folders;
import 'generated/api/init.dart' as rust_init;
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

export 'generated/api/accounts.dart' show Selection;
export 'generated/api/events.dart' show JobEvent, JobPhase;
export 'generated/api/folders.dart' show FolderCounts;
export 'generated/api/init.dart' show AppInfo;

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

  /// Create or update an account; returns its id. Keyed by email address, so
  /// re-saving a known address edits rather than duplicates.
  Future<int> saveAccount(Map<String, dynamic> form) =>
      rust_accounts.saveAccount(form: jsonEncode(form));

  /// Delete an account; returns the account to show instead, or `-1`.
  Future<int> deleteAccount(int id) => rust_accounts.deleteAccount(id: id);

  // --- folders -------------------------------------------------------------

  Future<List<Folder>> folders(int accountId) async =>
      _decodeList(await rust_folders.foldersJson(accountId: accountId),
          Folder.fromJson);

  Future<String> folderPath(int folderId) =>
      rust_folders.folderPath(folderId: folderId);

  Future<FolderCounts> folderCounts(int folderId) =>
      rust_folders.folderCounts(folderId: folderId);

  /// Resolve a folder path to its local id, for a UI that navigated by path
  /// (search results carry paths, reads take ids).
  Future<int> folderIdForPath(int accountId, String path) async =>
      (await rust_folders.folderIdForPath(accountId: accountId, path: path))
          .toInt();

  Future<void> setFolderSubscribed(int folderId, bool subscribed) =>
      rust_folders.setFolderSubscribed(
          folderId: folderId, subscribed: subscribed);

  // --- messages ------------------------------------------------------------

  Future<List<MessageSummary>> messages(
    int folderId, {
    int limit = 200,
    int offset = 0,
  }) async =>
      _decodeList(
        await rust_messages.messagesJson(
            folderId: folderId, limit: limit, offset: offset),
        MessageSummary.fromJson,
      );

  Future<MessageBody> message(int folderId, int uid) async => MessageBody
      .fromJson(await _decodeMap(
          await rust_messages.messageJson(folderId: folderId, uid: uid)));

  /// Re-sanitized HTML with remote images kept — the "show once" path.
  Future<String> messageHtmlWithRemoteImages(int folderId, int uid) =>
      rust_messages.messageHtml(
          folderId: folderId, uid: uid, allowRemote: true);

  Future<MessageHeaders> messageHeaders(int folderId, int uid) async =>
      MessageHeaders.fromJson(await _decodeMap(
          await rust_messages.headersJson(folderId: folderId, uid: uid)));

  Future<void> markRead(int accountId, int folderId, int uid, bool read) =>
      rust_messages.markRead(
          accountId: accountId, folderId: folderId, uid: uid, read: read);

  /// Returns how many rows actually changed. Rust counts these as `u64`,
  /// which crosses as a `BigInt`; a folder never holds enough messages for
  /// that to matter, so the UI gets a plain int.
  Future<int> markReadMany(
          int accountId, int folderId, List<int> uids, bool read) async =>
      (await rust_messages.markReadMany(
              accountId: accountId,
              folderId: folderId,
              uids: uids,
              read: read))
          .toInt();

  Future<bool> toggleStar(int accountId, int folderId, int uid) =>
      rust_messages.toggleStar(
          accountId: accountId, folderId: folderId, uid: uid);

  Future<int> setStarMany(
          int accountId, int folderId, List<int> uids, bool starred) async =>
      (await rust_messages.setStarMany(
              accountId: accountId,
              folderId: folderId,
              uids: uids,
              starred: starred))
          .toInt();

  // --- moving and deleting (queued) ---------------------------------------

  Future<void> deleteMessages(int accountId, int folderId, List<int> uids) =>
      rust_mutate.deleteMessages(
          accountId: accountId, folderId: folderId, uids: uids);

  Future<void> purgeMessages(int accountId, int folderId, List<int> uids) =>
      rust_mutate.purgeMessages(
          accountId: accountId, folderId: folderId, uids: uids);

  Future<void> archiveMessages(int accountId, int folderId, List<int> uids) =>
      rust_mutate.archiveMessages(
          accountId: accountId, folderId: folderId, uids: uids);

  Future<void> moveMessages(
          int accountId, int folderId, List<int> uids, String destPath) =>
      rust_mutate.moveMessages(
          accountId: accountId,
          folderId: folderId,
          uids: uids,
          destPath: destPath);

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

  // --- search --------------------------------------------------------------

  /// Local FTS only, in rank order. Cheap enough to run on every keystroke.
  Future<List<SearchHit>> search(
    int accountId,
    String query, {
    String folder = '',
    int limit = 100,
  }) async =>
      _decodeList(
        await rust_search.searchJson(
            accountId: accountId, query: query, folder: folder, limit: limit),
        SearchHit.fromJson,
      );

  /// Top up thin local results from the server. Queued; re-run [search] when
  /// the `"Search"` job finishes.
  Future<void> searchServer(int accountId, String query,
          {String folder = ''}) =>
      rust_search.searchServer(
          accountId: accountId, query: query, folder: folder);

  // --- composer ------------------------------------------------------------

  /// Validate, build and queue a message. Throws on anything the user can
  /// still fix, with the composer still open.
  Future<void> sendMail(
          int accountId, int folderId, Map<String, dynamic> form) =>
      rust_composer.sendMail(
          accountId: accountId, folderId: folderId, form: jsonEncode(form));

  Future<void> saveDraft(int accountId, Map<String, dynamic> form) =>
      rust_composer.saveDraft(accountId: accountId, form: jsonEncode(form));

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
          accountId: accountId, folderId: folderId, uid: uid);

  Future<String> saveAttachmentTo(int attachmentId, String path) =>
      rust_attachments.saveAttachmentTo(attachmentId: attachmentId, path: path);

  /// Write every non-inline attachment of a message into `dir`.
  /// Returns how many files were written.
  Future<int> saveAllAttachmentsTo(int folderId, int uid, String dir) async =>
      (await rust_attachments.saveAllAttachmentsTo(
              folderId: folderId, uid: uid, dir: dir))
          .toInt();

  // --- contacts ------------------------------------------------------------

  Future<List<Contact>> contacts({String prefix = ''}) async => _decodeList(
      await rust_contacts.contactsJson(prefix: prefix), Contact.fromJson);

  Future<void> setContactAlias(String address, String alias) =>
      rust_contacts.setContactAlias(address: address, alias: alias);

  Future<void> deleteContact(String address) =>
      rust_contacts.deleteContact(address: address);

  // --- settings ------------------------------------------------------------

  Future<AppSettings> settings() async =>
      AppSettings.fromJson(await _decodeMap(await rust_settings.settingsJson()));

  Future<void> setSetting(String key, String value) =>
      rust_settings.setSetting(key: key, value: value);

  Future<void> setSort(String field, bool descending) =>
      rust_settings.setSort(field: field, descending: descending);
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
