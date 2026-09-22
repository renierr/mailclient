import 'dart:async';

import 'package:flutter/foundation.dart';

import '../ffi/mail_core.dart';
import '../models/models.dart';

/// What the app is showing, and how it reacts to the core changing underneath.
///
/// The Qt frontend keeps the selection in Rust and has the bridge push rebuilt
/// feeds into QML properties. Here it is the other way round: the selection
/// lives in Dart, reads take explicit ids, and a finished job only says *what*
/// changed so this class can re-read the part it is actually showing. That is
/// why there is no stale-selection reconciliation anywhere — a job that
/// finishes after the user has moved on simply refreshes nothing visible.
class MailState extends ChangeNotifier {
  MailState(this._core) {
    _jobs = _core.jobEvents().listen(_onJobEvent);
  }

  final MailCore _core;
  late final StreamSubscription<JobEvent> _jobs;

  List<Account> _accounts = const [];
  List<Folder> _folders = const [];
  List<MessageSummary> _messages = const [];
  MessageBody? _openMessage;

  int _accountId = -1;
  int _folderId = -1;
  int _openUid = -1;

  bool _loading = true;
  String _status = '';
  bool _statusIsError = false;

  /// Jobs currently running, by kind, so the UI can show a spinner on the one
  /// control that started it rather than locking the whole window.
  final Set<String> _busyKinds = {};

  List<Account> get accounts => _accounts;

  /// Only the folders the user wants to see. Unsubscribed ones keep syncing;
  /// they are just not in the sidebar.
  List<Folder> get visibleFolders =>
      _folders.where((f) => f.subscribed).toList(growable: false);

  /// Every folder, for the folder manager and the move picker.
  List<Folder> get allFolders => _folders;
  List<MessageSummary> get messages => _messages;
  /// The body of the open message, or null while it loads.
  MessageBody? get openBody => _openMessage;

  int get accountId => _accountId;
  int get folderId => _folderId;
  int get openUid => _openUid;

  Account? get account =>
      _accounts.where((a) => a.id == _accountId).firstOrNull;
  Folder? get folder => _folders.where((f) => f.id == _folderId).firstOrNull;

  bool get loading => _loading;
  bool get hasAccounts => _accounts.isNotEmpty;
  bool get isSyncing => _busyKinds.contains('Sync');
  bool get isBusy => _busyKinds.isNotEmpty;

  /// The status bar line. Empty when there is nothing to say.
  String get status => _status;
  bool get statusIsError => _statusIsError;

  /// Load accounts and restore where the last session left off.
  Future<void> start() async {
    _loading = true;
    notifyListeners();
    await _reloadAccounts();
    if (_accounts.isNotEmpty) {
      final sel = await _core.initialSelection();
      await _openAccount(sel.accountId, folderId: sel.folderId);
      // Deferred, never awaited: the cache is already on screen, and a slow
      // server must not hold the first paint.
      unawaited(syncAccount());
    }
    _loading = false;
    notifyListeners();
  }

  Future<void> selectAccount(int id) async {
    final sel = await _core.selectAccount(id);
    await _openAccount(sel.accountId, folderId: sel.folderId);
    unawaited(syncAccount());
  }

  /// Open a folder. Cache-only and instant by design — the server fill is a
  /// separate, queued job, so clicking through folders never waits on IMAP.
  Future<void> selectFolder(int id) async {
    if (id == _folderId) return;
    _folderId = id;
    _openUid = -1;
    _openMessage = null;
    notifyListeners();
    await _reloadMessages();
    unawaited(_core.syncFolder(_accountId, id).catchError(_ignoreBusy));
  }

  /// Open a message: load its body, and mark it read locally.
  ///
  /// The read flag is a local write plus a background push, so this returns as
  /// soon as SQLite has it — reading a message can never wait on the network.
  Future<void> openMessage(int uid) async {
    _openUid = uid;
    _openMessage = null;
    notifyListeners();

    final body = await _core.message(_folderId, uid);
    // The user may have moved on while the body loaded.
    if (_openUid != uid) return;
    _openMessage = body;

    final row = _messages.where((m) => m.uid == uid).firstOrNull;
    if (row != null && row.unread) {
      await _core.markRead(_accountId, _folderId, uid, true);
      _patchRow(uid, (m) => m.copyWith(unread: false));
      unawaited(_reloadFolders());
    }
    notifyListeners();
  }

  void closeMessage() {
    _openUid = -1;
    _openMessage = null;
    notifyListeners();
  }

  Future<void> toggleStar(int uid) async {
    final starred = await _core.toggleStar(_accountId, _folderId, uid);
    _patchRow(uid, (m) => m.copyWith(starred: starred));
    notifyListeners();
  }

  Future<void> setRead(int uid, bool read) async {
    await _core.markRead(_accountId, _folderId, uid, read);
    _patchRow(uid, (m) => m.copyWith(unread: !read));
    notifyListeners();
    unawaited(_reloadFolders());
  }

  /// Delete a selection — Trash, or destroyed where Trash does not apply.
  /// Queued; the list refreshes when the job reports back.
  Future<void> deleteMessages(List<int> uids) =>
      _queue('Delete', () => _core.deleteMessages(_accountId, _folderId, uids));

  Future<void> archiveMessages(List<int> uids) => _queue(
      'Archive', () => _core.archiveMessages(_accountId, _folderId, uids));

  Future<void> moveMessages(List<int> uids, String destPath) => _queue('Move',
      () => _core.moveMessages(_accountId, _folderId, uids, destPath));

  Future<void> syncAccount() =>
      _queue('Sync', () => _core.syncAccount(_accountId));

  Future<void> loadOlderMessages() =>
      _queue('Sync', () => _core.loadOlderMessages(_accountId, _folderId));

  Future<void> refreshFolders() =>
      _queue('Folders', () => _core.refreshFolders(_accountId));

  /// Re-read accounts after the setup dialog wrote one.
  Future<void> accountsChanged({int? select}) async {
    await _reloadAccounts();
    final id = select ?? (_accounts.isNotEmpty ? _accounts.first.id : -1);
    if (id >= 0) {
      await selectAccount(id);
    } else {
      _accountId = -1;
      _folderId = -1;
      _folders = const [];
      _messages = const [];
      notifyListeners();
    }
  }

  void showStatus(String message, {bool isError = false}) {
    _status = message;
    _statusIsError = isError;
    notifyListeners();
  }

  // --- internals -----------------------------------------------------------

  /// Route a finished background job back into what is on screen.
  void _onJobEvent(JobEvent e) {
    switch (e.phase) {
      case JobPhase.progress:
        // A milestone, not a completion: the job is still holding its slot.
        if (e.status.isNotEmpty) showStatus(e.status);
      case JobPhase.finished:
        _busyKinds.remove(e.kind);
        if (e.status.isNotEmpty) showStatus(e.status, isError: !e.ok);
        _refreshFor(e);
    }
    notifyListeners();
  }

  void _refreshFor(JobEvent e) {
    // `-1` for the account means the job changed nothing worth re-reading.
    if (e.accountId < 0 || e.accountId != _accountId) return;
    unawaited(_reloadFolders());
    // A folder of `-1` alongside a real account is a full sync: everything
    // may have changed, including the folder we are looking at.
    if (e.folderId < 0 || e.folderId == _folderId) {
      unawaited(_reloadMessages());
    }
  }

  Future<void> _openAccount(int accountId, {required int folderId}) async {
    _accountId = accountId;
    _folderId = folderId;
    _openUid = -1;
    _openMessage = null;
    await _reloadFolders();
    await _reloadMessages();
    notifyListeners();
  }

  Future<void> _reloadAccounts() async {
    _accounts = await _core.accounts();
    notifyListeners();
  }

  Future<void> _reloadFolders() async {
    if (_accountId < 0) return;
    _folders = await _core.folders(_accountId);
    notifyListeners();
  }

  Future<void> _reloadMessages() async {
    if (_folderId < 0) {
      _messages = const [];
    } else {
      _messages = await _core.messages(_folderId);
    }
    notifyListeners();
  }

  void _patchRow(int uid, MessageSummary Function(MessageSummary) f) {
    _messages = [
      for (final m in _messages) if (m.uid == uid) f(m) else m,
    ];
  }

  /// Start a queued job and surface the reason if the core refuses it.
  ///
  /// `kind` must match what the Rust side reports, because that is how the
  /// finishing event clears the spinner again. A refusal is normal (the same
  /// job is already in flight) and does not deserve a dialog — the core
  /// dedupes, so the status line is the whole story.
  Future<void> _queue(String kind, Future<void> Function() start) async {
    try {
      await start();
      _busyKinds.add(kind);
      notifyListeners();
    } catch (e) {
      showStatus(_message(e), isError: true);
    }
  }

  void _ignoreBusy(Object _) {}

  static String _message(Object e) =>
      e is Exception ? e.toString().replaceFirst('Exception: ', '') : '$e';

  @override
  void dispose() {
    _jobs.cancel();
    super.dispose();
  }
}

extension<T> on Iterable<T> {
  /// `iterator` hands out a fresh iterator on every read, so it has to be
  /// taken once and then asked both questions.
  T? get firstOrNull {
    final it = iterator;
    return it.moveNext() ? it.current : null;
  }
}
