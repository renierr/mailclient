import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../ffi/mail_core.dart';
import '../models/models.dart';
import '../models/settings.dart';

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

  AppSettings _settings = AppSettings.defaults;

  // --- search ------------------------------------------------------------
  String _searchQuery = '';
  bool _searchFolderOnly = false;
  List<SearchHit> _searchHits = const [];
  Timer? _searchBackfillTimer;
  bool _serverSearchPending = false;

  // --- multi-select ------------------------------------------------------
  bool _selectionMode = false;
  final Set<int> _selectedUids = {};

  /// The reader takes the whole window, like the Qt fullscreen view. Only
  /// the wide layout uses it — narrower ones already give the reader every
  /// pixel they have.
  bool _readerFullscreen = false;
  bool get readerFullscreen => _readerFullscreen;

  void toggleReaderFullscreen() {
    _readerFullscreen = !_readerFullscreen;
    notifyListeners();
  }

  // --- list paging and counts --------------------------------------------
  int _messageLimit = _pageSize;
  static const _pageSize = 200;
  int _cachedCount = 0;
  int _serverTotal = -1;

  Timer? _markReadTimer;
  Timer? _autoSyncTimer;

  /// Last capabilities payload per account id, from the `"Capabilities"` job.
  final Map<int, Map<String, dynamic>> _capabilities = {};

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

  AppSettings get settings => _settings;

  // --- search ------------------------------------------------------------

  String get searchQuery => _searchQuery;
  bool get searchFolderOnly => _searchFolderOnly;
  List<SearchHit> get searchHits => _searchHits;
  bool get searching => _searchQuery.length >= 3;

  // --- multi-select ------------------------------------------------------

  bool get selectionMode => _selectionMode;
  Set<int> get selectedUids => _selectedUids;
  int get selectedCount => _selectedUids.length;

  bool get allSelected =>
      _selectedUids.isNotEmpty &&
      _messages.every((m) => _selectedUids.contains(m.uid));

  // --- list --------------------------------------------------------------

  int get cachedCount => _cachedCount;
  int get serverTotal => _serverTotal;

  /// Whether the folder is the account's Drafts: tapping a row edits the
  /// draft instead of previewing it.
  bool get isDraftsFolder => folder?.role == FolderRole.drafts;

  /// Deleting from Junk, from Trash itself, or without a Trash folder destroys
  /// outright — the confirm dialog says which, because only one is undoable.
  bool get deleteIsPermanent {
    final f = folder;
    if (f == null) return true;
    if (f.role == FolderRole.junk || f.role == FolderRole.trash) return true;
    return !_folders.any((o) => o.role == FolderRole.trash);
  }

  bool get hasTrashFolder =>
      _folders.any((f) => f.role == FolderRole.trash);

  Map<String, dynamic>? capabilitiesFor(int accountId) =>
      _capabilities[accountId];

  /// Load accounts and restore where the last session left off.
  Future<void> start() async {
    _loading = true;
    notifyListeners();
    await _reloadSettings();
    await _reloadAccounts();
    if (_accounts.isNotEmpty) {
      final sel = await _core.initialSelection();
      await _openAccount(sel.accountId, folderId: sel.folderId);
      // Deferred, never awaited: the cache is already on screen, and a slow
      // server must not hold the first paint.
      unawaited(syncAccount());
    }
    _loading = false;
    _rescheduleAutoSync();
    notifyListeners();
  }

  Future<void> selectAccount(int id) async {
    exitSearch();
    final sel = await _core.selectAccount(id);
    await _openAccount(sel.accountId, folderId: sel.folderId);
    unawaited(syncAccount());
  }

  /// Open a folder. Cache-only and instant by design — the server fill is a
  /// separate, queued job, so clicking through folders never waits on IMAP.
  Future<void> selectFolder(int id) async {
    if (id == _folderId) return;
    exitSearch();
    _folderId = id;
    _openUid = -1;
    _openMessage = null;
    _messageLimit = _pageSize;
    notifyListeners();
    await _reloadMessages();
    unawaited(_core.syncFolder(_accountId, id).catchError(_ignoreBusy));
  }

  /// Open a message: load its body, and mark it read on the viewer's terms.
  ///
  /// The read flag is a local write plus a background push, so this returns as
  /// soon as SQLite has it — reading a message can never wait on the network.
  /// When the mark-read delay is set, the message only counts as read if it is
  /// still open when the timer fires.
  Future<void> openMessage(int uid) async {
    _openUid = uid;
    _openMessage = null;
    _markReadTimer?.cancel();
    notifyListeners();

    final body = await _core.message(_folderId, uid);
    // The user may have moved on while the body loaded.
    if (_openUid != uid) return;
    _openMessage = body;

    final row = _messages.where((m) => m.uid == uid).firstOrNull;
    if (row != null &&
        row.unread &&
        _settings.autoMarkRead &&
        _folderId >= 0) {
      final delay = _settings.markReadDelaySecs;
      if (delay <= 0) {
        await _applyRead(uid, true);
      } else {
        _markReadTimer = Timer(Duration(seconds: delay), () {
          // Still looking at it: Thunderbird-style, closing it early means
          // it stays unread.
          if (_openUid == uid) unawaited(_applyRead(uid, true));
        });
      }
    }
    notifyListeners();
  }

  Future<void> _applyRead(int uid, bool read) async {
    await _core.markRead(_accountId, _folderId, uid, read);
    _patchRow(uid, (m) => m.copyWith(unread: !read));
    unawaited(_reloadFolders());
    notifyListeners();
  }

  void closeMessage() {
    _openUid = -1;
    _openMessage = null;
    _markReadTimer?.cancel();
    notifyListeners();
  }

  Future<void> toggleStar(int uid) async {
    final starred = await _core.toggleStar(_accountId, _folderId, uid);
    _patchRow(uid, (m) => m.copyWith(starred: starred));
    notifyListeners();
  }

  Future<void> setRead(int uid, bool read) async {
    await _applyRead(uid, read);
  }

  /// Delete a selection — Trash, or destroyed where Trash does not apply.
  /// Queued; the list refreshes when the job reports back.
  Future<void> deleteMessages(List<int> uids) =>
      _queue('Delete', () => _core.deleteMessages(_accountId, _folderId, uids),
          clearSelection: true);

  /// Destroy a selection server-side. No undo; the UI always confirms first.
  Future<void> purgeMessages(List<int> uids) =>
      _queue('Purge', () => _core.purgeMessages(_accountId, _folderId, uids),
          clearSelection: true);

  Future<void> archiveMessages(List<int> uids) => _queue('Archive',
      () => _core.archiveMessages(_accountId, _folderId, uids),
      clearSelection: true);

  Future<void> moveMessages(List<int> uids, String destPath) => _queue('Move',
      () => _core.moveMessages(_accountId, _folderId, uids, destPath),
      clearSelection: true);

  Future<void> markReadMany(List<int> uids, bool read) async {
    await _core.markReadMany(_accountId, _folderId, uids, read);
    for (final uid in uids) {
      _patchRow(uid, (m) => m.copyWith(unread: !read));
    }
    notifyListeners();
    unawaited(_reloadFolders());
  }

  Future<void> setStarMany(List<int> uids, bool starred) async {
    await _core.setStarMany(_accountId, _folderId, uids, starred);
    for (final uid in uids) {
      _patchRow(uid, (m) => m.copyWith(starred: starred));
    }
    notifyListeners();
  }

  Future<void> syncAccount() =>
      _queue('Sync', () => _core.syncAccount(_accountId));

  Future<void> loadOlderMessages() {
    // Show the next page of what is already cached immediately; the server
    // batch lands through the job event and extends it further.
    _messageLimit += _pageSize;
    unawaited(_reloadMessages());
    return _queue('Sync', () => _core.loadOlderMessages(_accountId, _folderId));
  }

  Future<void> refreshFolders() =>
      _queue('Folders', () => _core.refreshFolders(_accountId));

  Future<void> setFolderSubscribed(int folderId, bool subscribed) async {
    await _core.setFolderSubscribed(folderId, subscribed);
    await _reloadFolders();
  }

  Future<void> createFolder(String path) =>
      _queue('Folders', () => _core.createFolder(_accountId, path));

  Future<void> setSort(String field, bool descending) async {
    await _core.setSort(field, descending);
    await _reloadSettings();
    await _reloadMessages();
  }

  // --- search ------------------------------------------------------------

  /// Run the local FTS index. At 3+ letters a thin result also schedules a
  /// debounced server backfill; the `"Search"` job re-runs this when done.
  Future<void> runSearch(String query, {bool? folderOnly}) async {
    _searchQuery = query;
    if (folderOnly != null) _searchFolderOnly = folderOnly;
    _searchBackfillTimer?.cancel();
    if (!searching) {
      _searchHits = const [];
      _serverSearchPending = false;
      notifyListeners();
      return;
    }
    final scope =
        _searchFolderOnly ? (folder?.path ?? '') : '';
    _searchHits = await _core.search(_accountId, query, folder: scope);
    notifyListeners();
    if (_searchHits.length < 50 && !_serverSearchPending) {
      _searchBackfillTimer = Timer(const Duration(milliseconds: 800), () {
        _serverSearchPending = true;
        unawaited(_core
            .searchServer(_accountId, _searchQuery, folder: scope)
            .catchError(_ignoreBusy));
      });
    }
  }

  void exitSearch() {
    if (_searchQuery.isEmpty && _searchHits.isEmpty) return;
    _searchBackfillTimer?.cancel();
    _searchQuery = '';
    _searchHits = const [];
    _serverSearchPending = false;
    notifyListeners();
  }

  /// Open a search hit: leave search, land in its folder, open the message.
  Future<void> jumpToHit(SearchHit hit) async {
    final folderId = hit.folderId >= 0
        ? hit.folderId
        : await _core.folderIdForPath(_accountId, hit.folder);
    exitSearch();
    _folderId = folderId;
    _openUid = -1;
    _openMessage = null;
    _messageLimit = _pageSize;
    notifyListeners();
    await _reloadMessages();
    await openMessage(hit.uid);
  }

  // --- multi-select ------------------------------------------------------

  void enterSelectionMode() {
    _selectionMode = true;
    notifyListeners();
  }

  void exitSelectionMode() {
    _selectionMode = false;
    _selectedUids.clear();
    notifyListeners();
  }

  void toggleSelect(int uid) {
    if (!_selectedUids.remove(uid)) _selectedUids.add(uid);
    if (_selectedUids.isEmpty) {
      _selectionMode = false;
    } else {
      _selectionMode = true;
    }
    notifyListeners();
  }

  /// Toggle a contiguous range, for shift-click. Uids are list-ordered.
  void selectRange(int anchorUid, int uid) {
    final order = [for (final m in _messages) m.uid];
    final a = order.indexOf(anchorUid);
    final b = order.indexOf(uid);
    if (a < 0 || b < 0) {
      toggleSelect(uid);
      return;
    }
    final lo = a < b ? a : b;
    final hi = a < b ? b : a;
    _selectedUids.addAll(order.sublist(lo, hi + 1));
    _selectionMode = true;
    notifyListeners();
  }

  void selectAllVisible() {
    _selectedUids.addAll(_messages.map((m) => m.uid));
    _selectionMode = true;
    notifyListeners();
  }

  void selectUnread() {
    _selectedUids
        .addAll(_messages.where((m) => m.unread).map((m) => m.uid));
    _selectionMode = _selectedUids.isNotEmpty;
    notifyListeners();
  }

  void selectStarred() {
    _selectedUids
        .addAll(_messages.where((m) => m.starred).map((m) => m.uid));
    _selectionMode = _selectedUids.isNotEmpty;
    notifyListeners();
  }

  void invertSelection() {
    final all = _messages.map((m) => m.uid).toSet();
    final inverted = all.difference(_selectedUids);
    _selectedUids
      ..clear()
      ..addAll(inverted);
    _selectionMode = _selectedUids.isNotEmpty;
    notifyListeners();
  }

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

  /// Delete an account. The core reports which account to show instead.
  Future<void> removeAccount(int id) async {
    final next = await MailCore.instance.deleteAccount(id);
    await accountsChanged(select: next >= 0 ? next : null);
    if (next < 0) {
      _accountId = -1;
      _folderId = -1;
      notifyListeners();
    }
  }

  // --- settings ----------------------------------------------------------

  Future<void> _reloadSettings() async {
    _settings = await _core.settings();
    notifyListeners();
  }

  Future<void> reloadSettings() => _reloadSettings();

  Future<void> setSetting(String key, String value) async {
    await _core.setSetting(key, value);
    await _reloadSettings();
    if (key == SettingKeys.syncInterval) _rescheduleAutoSync();
  }

  Future<void> refreshCapabilities(int accountId) =>
      _queue('Capabilities', () => _core.refreshServerCapabilities(accountId));

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
        // The Send job's first progress means SMTP accepted the message.
        if (e.status.isNotEmpty || e.kind == 'Send') {
          showStatus(e.status.isNotEmpty ? e.status : 'Sending…');
        }
      case JobPhase.finished:
        _busyKinds.remove(e.kind);
        if (e.status.isNotEmpty) showStatus(e.status, isError: !e.ok);
        _refreshFor(e);
    }
    notifyListeners();
  }

  void _refreshFor(JobEvent e) {
    if (e.kind == 'Capabilities') {
      _storeCapabilities(e);
      return;
    }
    if (e.kind == 'Attachments') {
      // Nothing the list shows changed; the reader re-reads its message.
      if (e.accountId == _accountId) unawaited(_reloadOpenMessage());
      return;
    }
    if (e.kind == 'Search') {
      _serverSearchPending = false;
      // The backfill pulled server hits into the cache; show them if the
      // query is still what the job ran for.
      if (searching) unawaited(_rerunSearch());
      return;
    }
    if (e.kind == 'Send' && e.ok) {
      // SMTP accepted it and the Sent copy is filed server-side, but the
      // local cache only learns about it from a sync. The generic reload
      // below is not enough — pull the Sent folder so it appears.
      final sent = _folders
          .where((f) => f.role == FolderRole.sent)
          .firstOrNull;
      // A refusal just means a sync is already running; its own event will
      // refresh the list when it lands.
      if (sent != null) {
        unawaited(
            _core.syncFolder(_accountId, sent.id).catchError(_ignoreBusy));
      }
    }
    // `-1` for the account means the job changed nothing worth re-reading.
    if (e.accountId < 0 || e.accountId != _accountId) return;
    unawaited(_reloadFolders());
    // A folder of `-1` alongside a real account is a full sync: everything
    // may have changed, including the folder we are looking at.
    if (e.folderId < 0 || e.folderId == _folderId) {
      unawaited(_reloadMessages());
      unawaited(_reloadOpenMessage());
    }
  }

  /// The capabilities arrive as the finishing event's status, JSON-encoded —
  /// the one place a job's status line is a payload rather than prose.
  void _storeCapabilities(JobEvent e) {
    try {
      final payload = jsonDecode(e.status) as Map<String, dynamic>;
      final id = (payload['account_id'] as num?)?.toInt() ?? -1;
      if (id >= 0) _capabilities[id] = payload;
    } catch (_) {
      // A failed capabilities job reports its error as plain status text,
      // which is already on the status line.
    }
  }

  Future<void> _rerunSearch() async {
    final scope =
        _searchFolderOnly ? (folder?.path ?? '') : '';
    _searchHits = await _core.search(_accountId, _searchQuery, folder: scope);
    notifyListeners();
  }

  Future<void> _reloadOpenMessage() async {
    if (_openUid < 0 || _folderId < 0) return;
    final uid = _openUid;
    try {
      final body = await _core.message(_folderId, uid);
      if (_openUid == uid) {
        _openMessage = body;
        notifyListeners();
      }
    } catch (_) {
      // The message is gone server-side; the list refresh drops the row and
      // the reader keeps showing what it has.
    }
  }

  void _rescheduleAutoSync() {
    _autoSyncTimer?.cancel();
    final minutes = _settings.syncIntervalMinutes;
    if (minutes <= 0 || _accountId < 0) return;
    _autoSyncTimer =
        Timer.periodic(Duration(minutes: minutes), (_) {
      if (!isSyncing && hasAccounts) unawaited(syncAccount());
    });
  }

  Future<void> _openAccount(int accountId, {required int folderId}) async {
    _accountId = accountId;
    _folderId = folderId;
    _openUid = -1;
    _openMessage = null;
    _messageLimit = _pageSize;
    exitSearch();
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
    _pruneSelection();
    notifyListeners();
  }

  Future<void> _reloadMessages() async {
    if (_folderId < 0) {
      _messages = const [];
      _cachedCount = 0;
      _serverTotal = -1;
    } else {
      _messages =
          await _core.messages(_folderId, limit: _messageLimit);
      try {
        final counts = await _core.folderCounts(_folderId);
        _cachedCount = counts.cached.toInt();
        _serverTotal = counts.server.toInt();
      } catch (_) {
        _cachedCount = _messages.length;
        _serverTotal = -1;
      }
      _pruneSelection();
    }
    notifyListeners();
  }

  /// Drop selected uids that are no longer in the list, and leave selection
  /// mode when nothing remains.
  void _pruneSelection() {
    if (_selectedUids.isEmpty) return;
    final live = _messages.map((m) => m.uid).toSet();
    _selectedUids.retainAll(live);
    if (_selectedUids.isEmpty) _selectionMode = false;
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
  Future<void> _queue(String kind, Future<void> Function() start,
      {bool clearSelection = false}) async {
    try {
      await start();
      _busyKinds.add(kind);
      if (clearSelection) {
        _selectedUids.clear();
        _selectionMode = false;
      }
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
    _searchBackfillTimer?.cancel();
    _markReadTimer?.cancel();
    _autoSyncTimer?.cancel();
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
