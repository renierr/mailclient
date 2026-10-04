import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../ffi/mail_core.dart';
import '../models/account_settings.dart';
import '../models/models.dart';
import '../models/settings.dart';
import '../sync/background_sync.dart';

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

  /// Open message a queued move/trash/archive should leave behind, once that
  /// job finishes successfully, with the job it waits for. Only that job's
  /// finish acts on it — an unrelated sync or a failed search must neither
  /// close the reader nor cancel the wait.
  ({String kind, int accountId, int folderId, int uid})? _pendingClose;

  /// The open account's outbox counts (`mailcore::outbox::status`), for the
  /// status-bar pill. Null before the first read.
  OutboxStatus? _outboxStatus;

  /// Callers waiting for the next finish of a job kind — a form that has to
  /// know whether *its* job worked, which the shared status line cannot say.
  final Map<String, List<Completer<JobEvent>>> _finishWaiters = {};

  AppSettings _settings = AppSettings.placeholder;
  int _autoSyncMinutes = 0;

  // --- search ------------------------------------------------------------

  /// Bumped by every search, similar search and exit: a result whose await
  /// outlived a newer request is dropped instead of overwriting it.
  int _searchGen = 0;
  String _searchQuery = '';

  /// How [_searchQuery] runs, decided by the core (`search::plan`).
  SearchPlan _searchPlan = _noSearch;
  static const _noSearch = SearchPlan(
    mode: SearchMode.off,
    query: '',
    hitLimit: 0,
    debounceMs: 0,
  );
  bool _searchFolderOnly = false;
  List<SearchHit> _searchHits = const [];
  Timer? _searchBackfillTimer;
  bool _serverSearchPending = false;
  String? _similarSubject;
  ({int folderId, int uid})? _similarTarget;

  // --- list quick filters ------------------------------------------------
  // Each checked entry narrows the visible rows (AND-combined), in the
  // folder list and in search results alike — the Qt `MessageList`
  // `filterUnread/filterStarred/filterAttachments` twin, plus a date
  // quick-filter (`filterAfter` inclusive, `filterBefore` exclusive,
  // both set = the days between).
  bool _filterUnread = false;
  bool _filterStarred = false;
  bool _filterAttachments = false;
  String _filterAfter = '';
  String _filterBefore = '';

  bool get filterUnread => _filterUnread;
  bool get filterStarred => _filterStarred;
  bool get filterAttachments => _filterAttachments;

  /// `YYYY-MM-DD` lower bound (inclusive), `""` = unset.
  String get filterAfter => _filterAfter;

  /// `YYYY-MM-DD` upper bound (exclusive), `""` = unset.
  String get filterBefore => _filterBefore;

  bool get hasDateFilter => _filterAfter.isNotEmpty || _filterBefore.isNotEmpty;
  bool get hasListFilter =>
      _filterUnread ||
      _filterStarred ||
      _filterAttachments ||
      hasDateFilter;

  /// The words for the active date filter, phrased once by the core.
  String get dateFilterLabel =>
      _core.dateFilterLabel(_filterAfter, _filterBefore);

  bool _passesQuick(
    bool unread,
    bool starred,
    bool hasAttachments,
    String dateRaw,
  ) =>
      (!_filterUnread || unread) &&
      (!_filterStarred || starred) &&
      (!_filterAttachments || hasAttachments) &&
      (!hasDateFilter ||
          _core.dateFilterMatches(dateRaw, _filterAfter, _filterBefore));

  /// Whether a folder row is on screen: the quick filters, plus the core's
  /// instant filter for short input (`search::filter_matches`; longer input
  /// searches the index instead). The list pane and every selection entry
  /// use this, so a bulk action never reaches a row the user cannot see.
  bool isMessageShown(MessageSummary m) {
    if (!_passesQuick(m.unread, m.starred, m.hasAttachments, m.dateRaw)) {
      return false;
    }
    if (_searchPlan.query.isEmpty) return true;
    return _core.searchFilterMatches(_searchPlan.query, m);
  }

  /// Whether a search hit is on screen (the quick filters).
  bool isHitShown(SearchHit h) =>
      _passesQuick(h.unread, h.starred, h.hasAttachments, h.dateRaw);

  Iterable<MessageSummary> get _shownMessages =>
      _messages.where(isMessageShown);

  Iterable<SearchHit> get _shownHits => _searchHits.where(isHitShown);

  void setFilterUnread(bool on) {
    _filterUnread = on;
    _filtersChanged();
  }

  void setFilterStarred(bool on) {
    _filterStarred = on;
    _filtersChanged();
  }

  void setFilterAttachments(bool on) {
    _filterAttachments = on;
    _filtersChanged();
  }

  void setFilterAfter(String day) {
    _filterAfter = day.trim();
    _filtersChanged();
  }

  void setFilterBefore(String day) {
    _filterBefore = day.trim();
    _filtersChanged();
  }

  /// A named date preset (`today` | `week` | `month` | `older_month`),
  /// resolved by the core against today.
  void applyDatePreset(String preset) {
    final r = _core.datePresetRange(preset);
    _filterAfter = r.after;
    _filterBefore = r.before;
    _filtersChanged();
  }

  void clearDateFilter() {
    _filterAfter = '';
    _filterBefore = '';
    _filtersChanged();
  }

  void clearListFilters() {
    _filterUnread = false;
    _filterStarred = false;
    _filterAttachments = false;
    _filterAfter = '';
    _filterBefore = '';
    _filtersChanged();
  }

  void _filtersChanged() {
    _pruneSelection();
    notifyListeners();
  }

  // --- multi-select ------------------------------------------------------
  bool _selectionMode = false;
  final Set<int> _selectedUids = {};

  /// The checkbox set while searching. Hits span folders, and a UID is only
  /// unique within one, so they are keyed on the folder too.
  final Set<HitKey> _selectedHits = {};

  /// Undo offers collected while one bulk action runs folder by folder,
  /// merged into a single offer when it is done. Null outside such a run.

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
  int _cachedCount = 0;
  int _serverTotal = -1;
  OlderState? _olderState;
  bool _canLoadOlder = false;

  Timer? _markReadTimer;
  Timer? _autoSyncTimer;

  /// When the last account sync was asked for, so coming back to the app
  /// does not sync again right after one.
  DateTime? _lastSyncRequest;

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

  /// The open account's outbox counts, null before the first read.
  OutboxStatus? get outboxStatus => _outboxStatus;

  AppSettings get settings => _settings;

  /// The open account's automatic check interval (0 = manually).
  int get autoSyncMinutes => _autoSyncMinutes;

  // --- search ------------------------------------------------------------

  String get searchQuery => _searchQuery;
  bool get searchFolderOnly => _searchFolderOnly;
  List<SearchHit> get searchHits => _searchHits;
  bool get searching =>
      _searchPlan.mode == SearchMode.indexed || _similarSubject != null;
  bool get isSimilarSearch => _similarSubject != null;
  String? get similarSubject => _similarSubject;

  // --- multi-select ------------------------------------------------------

  bool get selectionMode => _selectionMode;
  Set<int> get selectedUids => _selectedUids;
  Set<HitKey> get selectedHits => _selectedHits;

  /// The checkbox set of whichever list is showing.
  int get selectedCount =>
      searching ? _selectedHits.length : _selectedUids.length;

  /// Every selected row is starred (the bulk bar offers Unstar then).
  bool get selectionAllStarred {
    if (searching) {
      return _selectedHits.isNotEmpty &&
          _searchHits
              .where((h) => _selectedHits.contains(h.key))
              .every((h) => h.starred);
    }
    return _selectedUids.isNotEmpty &&
        _selectedUids.every(
          (u) =>
              _messages.where((m) => m.uid == u).firstOrNull?.starred ?? false,
        );
  }

  /// Whether a bulk delete destroys: in search mode, when any selected
  /// hit's folder would.
  bool get selectionDeleteIsPermanent {
    if (!searching) return deleteIsPermanent;
    return _selectedHits.any(
      (k) => deleteIsPermanentIn(_folderIdByPath(k.folder) ?? -1),
    );
  }

  // --- list --------------------------------------------------------------

  int get cachedCount => _cachedCount;
  int get serverTotal => _serverTotal;

  /// The shown folder's "Show older" state, decided by the core
  /// (`feed::older_state`); null without a folder.
  OlderState? get olderState => _olderState;
  bool get canLoadOlder => _canLoadOlder;

  /// Whether the folder is the account's Drafts: tapping a row edits the
  /// draft instead of previewing it.
  bool get isDraftsFolder => folder?.role == FolderRole.drafts;

  /// Deleting from Junk, from Trash itself, or without a Trash folder destroys
  /// outright — the confirm dialog says which, because only one is undoable.
  bool get deleteIsPermanent => deleteIsPermanentIn(_folderId);

  /// [deleteIsPermanent] for any folder of this account, e.g. a search
  /// hit's.
  /// The folder feed says (`undo::delete_is_permanent`); an unknown folder
  /// asks as if it did.
  bool deleteIsPermanentIn(int folderId) =>
      _folders.where((o) => o.id == folderId).firstOrNull?.deleteIsPermanent ??
      true;

  bool get hasTrashFolder => _folders.any((f) => f.role == FolderRole.trash);

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
    unawaited(rescheduleBackgroundSync());
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
    // Re-picking the shown folder returns to its list, like Qt: in the
    // two-pane layout the reader covers the list, and the folder is the way
    // back to it.
    if (id == _folderId) {
      if (_openUid >= 0) closeMessage();
      return;
    }
    exitSearch();
    _folderId = id;
    _openUid = -1;
    _openMessage = null;
    notifyListeners();
    await _reloadMessages();
    unawaited(_core.syncFolder(_accountId, id).catchError(_ignoreBusy));
  }

  /// Open a message: load its body, and mark it read on the viewer's terms.
  ///
  /// The read flag is a local write plus a background push, so this returns as
  /// soon as SQLite has it — reading a message can never wait on the network.
  /// Whether and when viewing counts as read is the core's call
  /// (`store::settings::mark_read_plan`): already-read rows stay untouched
  /// instead of issuing a no-op flag write, as in Qt.
  Future<void> openMessage(int uid) async {
    _openUid = uid;
    _openMessage = null;
    _markReadTimer?.cancel();
    notifyListeners();

    final MessageBody body;
    try {
      body = await _core.message(_folderId, uid);
    } catch (_) {
      // Not in the cache (any more): a notification or search hit can
      // name mail a sync has since dropped. Back to the list, not a
      // spinner that never ends.
      if (_openUid == uid) _openMessageGone();
      return;
    }
    // The user may have moved on while the body loaded.
    if (_openUid != uid) return;
    _openMessage = body;

    final row = _messages.where((m) => m.uid == uid).firstOrNull;
    if (_folderId >= 0) {
      final plan = _core.markReadPlan(
        _settings.autoMarkRead,
        _settings.markReadDelaySecs,
        row?.unread ?? false,
      );
      switch (plan.plan) {
        case 'now':
          await _applyRead(uid, true);
        case 'after':
          _markReadTimer = Timer(Duration(seconds: plan.delaySecs.toInt()), () {
            // Still looking at it: Thunderbird-style, closing it early means
            // it stays unread.
            if (_openUid == uid) unawaited(_applyRead(uid, true));
          });
        case 'off':
          break;
      }
    }
    notifyListeners();
  }

  Future<void> _applyRead(int uid, bool read, {int? folderId}) async {
    final fid = folderId ?? _folderId;
    await _core.markRead(_accountId, fid, uid, read);
    if (fid == _folderId) _patchRow(uid, (m) => m.copyWith(unread: !read));
    unawaited(_reloadFolders());
    await _refreshHits();
    notifyListeners();
  }

  /// A row action from the search results may hit another folder than the
  /// one shown: re-read the hits so their flags (or their absence) show.
  Future<void> _refreshHits() async {
    if (searching) await _rerunSearch();
  }

  void closeMessage() {
    _openUid = -1;
    _openMessage = null;
    _markReadTimer?.cancel();
    // Fullscreen only makes sense over an open message; left set, the next
    // message would open straight into it.
    _readerFullscreen = false;
    notifyListeners();
  }

  // `folderId` defaults to the shown folder; search hits pass their own.

  Future<void> toggleStar(int uid, {int? folderId}) async {
    final fid = folderId ?? _folderId;
    final starred = await _core.toggleStar(_accountId, fid, uid);
    if (fid == _folderId) _patchRow(uid, (m) => m.copyWith(starred: starred));
    await _refreshHits();
    notifyListeners();
  }

  Future<void> setRead(int uid, bool read, {int? folderId}) async {
    await _applyRead(uid, read, folderId: folderId);
  }

  /// Delete a selection — Trash, or destroyed where Trash does not apply.
  /// Queued; the list refreshes when the job reports back.
  /// Delete a selection: to Trash and undoable (see [undoOffer]), or — from
  /// Junk, from Trash, or without a Trash folder — a purge job the UI has
  /// already confirmed as permanent.
  Future<void> deleteMessages(List<int> uids, {int? folderId}) =>
      _queueUndoable(
        () => _core.deleteMessages(_accountId, folderId ?? _folderId, uids),
        uids,
        folderId: folderId,
      );

  /// Destroy a selection server-side. No undo; the UI always confirms first.
  Future<void> purgeMessages(List<int> uids, {int? folderId}) {
    final fid = folderId ?? _folderId;
    return _queue(
      'Purge',
      () => _core.purgeMessages(_accountId, fid, uids),
      clearSelection: fid == _folderId,
      closeUid: fid == _folderId ? uids : const [],
    );
  }

  Future<void> archiveMessages(List<int> uids, {int? folderId}) =>
      _queueUndoable(
        () => _core.archiveMessages(_accountId, folderId ?? _folderId, uids),
        uids,
        folderId: folderId,
      );

  Future<void> moveMessages(List<int> uids, String destPath, {int? folderId}) =>
      _queueUndoable(
        () => _core.moveMessages(
          _accountId,
          folderId ?? _folderId,
          uids,
          destPath,
        ),
        uids,
        folderId: folderId,
      );

  /// The folder a search hit lives in; older index rows may lack the id.
  Future<int> folderIdOfHit(SearchHit hit) async => hit.folderId >= 0
      ? hit.folderId
      : await _core.folderIdForPath(_accountId, hit.folder);

  /// The latest undoable action, for the Undo snackbar. `seq` grows with
  /// every offer so a repeat of the same label still shows.
  ({String batch, String label, int seq})? get undoOffer => _undoOffer;
  ({String batch, String label, int seq})? _undoOffer;
  int _undoSeq = 0;

  int get undoGraceSecs => _core.undoGraceSecs();

  /// Take back a queued action (Undo on the snackbar, Ctrl+Z).
  Future<void> undo(String batch) async {
    try {
      showStatus(await _core.undoMove(batch));
      if (_undoOffer?.batch == batch) _undoOffer = null;
      await _reloadMessages();
      await _reloadFolders();
      await _refreshHits();
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
    }
  }

  /// Undo the latest offer, if it is still there.
  Future<void> undoLast() async {
    final offer = _undoOffer;
    if (offer != null) await undo(offer.batch);
  }

  Future<void> markReadMany(List<int> uids, bool read, {int? folderId}) async {
    final fid = folderId ?? _folderId;
    await _core.markReadMany(_accountId, fid, uids, read);
    if (fid == _folderId) {
      for (final uid in uids) {
        _patchRow(uid, (m) => m.copyWith(unread: !read));
      }
    }
    await _refreshHits();
    notifyListeners();
    unawaited(_reloadFolders());
  }

  Future<void> setStarMany(
    List<int> uids,
    bool starred, {
    int? folderId,
  }) async {
    final fid = folderId ?? _folderId;
    await _core.setStarMany(_accountId, fid, uids, starred);
    if (fid == _folderId) {
      for (final uid in uids) {
        _patchRow(uid, (m) => m.copyWith(starred: starred));
      }
    }
    await _refreshHits();
    notifyListeners();
  }

  // --- bulk actions on the checkbox set ------------------------------------
  //
  // Folder mode acts on the shown folder in one call; search mode hands the
  // hits to the core in one call too, which groups them by folder and
  // answers with one Undo and one purge job (`mailcore::bulk`).

  Future<void> bulkMarkRead(bool read) => _forSelection(
    (fid, uids) => markReadMany(uids, read, folderId: fid),
    (hits) => _flagHits(() => _core.markReadHits(_accountId, hits, read)),
    keepSelection: true,
  );

  Future<void> bulkStar(bool starred) => _forSelection(
    (fid, uids) => setStarMany(uids, starred, folderId: fid),
    (hits) => _flagHits(() => _core.setStarHits(_accountId, hits, starred)),
    keepSelection: true,
  );

  Future<void> bulkArchive() => _forSelection(
    (fid, uids) => archiveMessages(uids, folderId: fid),
    (hits) => _queueUndoable(
      () => _core.archiveHits(_accountId, hits),
      _selectedHitsHere,
    ),
  );

  Future<void> bulkMove(String destPath) => _forSelection(
    (fid, uids) => moveMessages(uids, destPath, folderId: fid),
    (hits) => _queueUndoable(
      () => _core.moveHits(_accountId, hits, destPath),
      _selectedHitsHere,
    ),
  );

  Future<void> bulkDelete() => _forSelection(
    (fid, uids) => deleteMessages(uids, folderId: fid),
    (hits) => _queueUndoable(
      () => _core.deleteHits(_accountId, hits),
      _selectedHitsHere,
    ),
  );

  Future<void> bulkPurge() => _forSelection(
    (fid, uids) => purgeMessages(uids, folderId: fid),
    (hits) => _queue(
      'Purge',
      () => _core.purgeHits(_accountId, hits),
      closeUid: _selectedHitsHere,
    ),
  );

  Future<void> _forSelection(
    Future<void> Function(int folderId, List<int> uids) inFolder,
    Future<void> Function(List<Hit> hits) acrossFolders, {
    bool keepSelection = false,
  }) async {
    if (!searching) {
      if (_selectedUids.isEmpty) return;
      return inFolder(_folderId, _selectedUids.toList(growable: false));
    }
    if (_selectedHits.isEmpty) return;
    try {
      await acrossFolders([
        for (final k in _selectedHits) Hit(folder: k.folder, uid: k.uid),
      ]);
    } finally {
      if (!keepSelection) {
        _selectedHits.clear();
        _selectionMode = false;
      }
      notifyListeners();
    }
  }

  /// Selected hits that live in the shown folder: the only ones that can be
  /// the open message.
  List<int> get _selectedHitsHere => [
    for (final k in _selectedHits)
      if (_folderIdByPath(k.folder) == _folderId) k.uid,
  ];

  /// A local flag write over hits, then the lists that show those flags.
  Future<void> _flagHits(Future<int> Function() write) async {
    try {
      await write();
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
      return;
    }
    await _reloadMessages();
    await _refreshHits();
    notifyListeners();
    unawaited(_reloadFolders());
  }

  int? _folderIdByPath(String path) =>
      _folders.where((f) => f.path == path).firstOrNull?.id;

  Future<void> syncAccount() {
    _lastSyncRequest = DateTime.now();
    return _queue('Sync', () => _core.syncAccount(_accountId));
  }

  // --- outbox ------------------------------------------------------------

  /// Re-read the open account's outbox counts. Cheap local read: safe to run
  /// on every job event, like Qt's footer pill does.
  Future<void> refreshOutbox() async {
    if (_accountId < 0) {
      _outboxStatus = null;
    } else {
      try {
        _outboxStatus = await _core.outboxStatus(_accountId);
      } catch (_) {
        // The pill keeps its last counts rather than blinking out.
      }
    }
    notifyListeners();
  }

  /// The open account's unsent mail, oldest first, for the outbox dialog.
  Future<List<OutboxEntry>> outboxEntries() =>
      _accountId < 0 ? Future.value(const []) : _core.outbox(_accountId);

  /// Forget one queued send, then refresh the pill. Errors go to the status
  /// line, like refused queues.
  Future<void> dismissOutbox(int id) async {
    try {
      await _core.dismissOutbox(_accountId, id);
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
    }
    await refreshOutbox();
  }

  /// The app is in the foreground again. Android freezes a backgrounded
  /// app, so the periodic sync did not run meanwhile, while the background
  /// check may well have pulled new mail into the cache. Show the cache at
  /// once, then sync unless auto-sync is off or a sync just ran.
  Future<void> resumed() async {
    if (_loading || !hasAccounts) return;
    _rescheduleAutoSync();
    await _reloadFolders();
    await _reloadMessages();
    if (_autoSyncMinutes > 0 &&
        !isSyncing &&
        shouldSyncOnResume(_lastSyncRequest, DateTime.now())) {
      unawaited(syncAccount());
    }
  }

  /// The list shows everything cached (like Qt), so "older" always means
  /// the next batch from the server; the job event re-reads the list.
  Future<void> loadOlderMessages() =>
      _queue('Sync', () => _core.loadOlderMessages(_accountId, _folderId));

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
    final gen = ++_searchGen;
    final wasSearching = searching;
    _similarSubject = null;
    _similarTarget = null;
    _searchQuery = query;
    _searchPlan = query.isEmpty ? _noSearch : _core.searchPlan(query);
    // A checkbox set belongs to the list it was made in.
    if (wasSearching != searching) _dropSelection();
    if (folderOnly != null) _searchFolderOnly = folderOnly;
    _searchBackfillTimer?.cancel();
    if (!searching) {
      _searchHits = const [];
      _serverSearchPending = false;
      notifyListeners();
      return;
    }
    final scope = _searchFolderOnly ? (folder?.path ?? '') : '';
    final plan = _searchPlan;
    final hits = await _core.search(_accountId, plan.query, folder: scope);
    if (gen != _searchGen) return;
    _searchHits = hits;
    notifyListeners();
    if (_searchHits.length < plan.hitLimit && !_serverSearchPending) {
      _searchBackfillTimer = Timer(Duration(milliseconds: plan.debounceMs), () {
        _serverSearchPending = true;
        unawaited(
          _core
              .searchServer(_accountId, plan.query, folder: scope)
              .catchError(_ignoreBusy),
        );
      });
    }
  }

  void exitSearch() {
    if (_searchQuery.isEmpty &&
        _searchHits.isEmpty &&
        _similarSubject == null) {
      return;
    }
    _searchGen++;
    _searchBackfillTimer?.cancel();
    if (searching) _dropSelection();
    _searchQuery = '';
    _searchPlan = _noSearch;
    _similarSubject = null;
    _similarTarget = null;
    _searchHits = const [];
    _serverSearchPending = false;
    notifyListeners();
  }

  /// Messages similar to the message at [uid] in [folderId] across the account.
  Future<void> findSimilar(int folderId, int uid) async {
    final gen = ++_searchGen;
    final wasSearching = searching;
    _searchBackfillTimer?.cancel();
    _serverSearchPending = false;
    final String subject;
    final List<SearchHit> hits;
    try {
      subject = await _core.similarSubject(_accountId, folderId, uid);
      hits = await _core.similar(_accountId, folderId, uid);
    } catch (e) {
      if (gen == _searchGen) {
        showStatus(
          'Could not find similar messages: ${coreErrorText(e)}',
          isError: true,
        );
      }
      return;
    }
    if (gen != _searchGen) return;
    _searchQuery = '';
    _searchPlan = _noSearch;
    _similarSubject = subject;
    _similarTarget = (folderId: folderId, uid: uid);
    if (!wasSearching) _dropSelection();
    _searchHits = hits;
    notifyListeners();
  }

  void clearSimilar() {
    if (_similarSubject == null) return;
    _searchGen++;
    _similarSubject = null;
    _similarTarget = null;
    _searchHits = const [];
    _dropSelection();
    notifyListeners();
  }

  void _dropSelection() {
    _selectionMode = false;
    _selectedUids.clear();
    _selectedHits.clear();
  }

  /// Open a search hit: switch to its folder underneath and open the
  /// message. The search stays, so back from the reader returns to the
  /// results (Qt does the same).
  Future<void> jumpToHit(SearchHit hit) async {
    final folderId = await folderIdOfHit(hit);
    _folderId = folderId;
    _openUid = -1;
    _openMessage = null;
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
    _dropSelection();
    notifyListeners();
  }

  void toggleSelectHit(SearchHit hit) {
    if (!_selectedHits.remove(hit.key)) _selectedHits.add(hit.key);
    _selectionMode = _selectedHits.isNotEmpty;
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
    final order = [for (final m in _shownMessages) m.uid];
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

  // The select-menu entries act on whichever list is showing.

  void selectAllVisible() => _selectWhere((_) => true, (_) => true);

  void selectUnread() => _selectWhere((m) => m.unread, (h) => h.unread);

  void selectStarred() => _selectWhere((m) => m.starred, (h) => h.starred);

  void _selectWhere(
    bool Function(MessageSummary) row,
    bool Function(SearchHit) hit,
  ) {
    if (searching) {
      _selectedHits.addAll(_shownHits.where(hit).map((h) => h.key));
    } else {
      _selectedUids.addAll(_shownMessages.where(row).map((m) => m.uid));
    }
    _selectionMode = selectedCount > 0;
    notifyListeners();
  }

  void invertSelection() {
    if (searching) {
      final inverted = _shownHits
          .map((h) => h.key)
          .toSet()
          .difference(_selectedHits);
      _selectedHits
        ..clear()
        ..addAll(inverted);
    } else {
      final all = _shownMessages.map((m) => m.uid).toSet();
      final inverted = all.difference(_selectedUids);
      _selectedUids
        ..clear()
        ..addAll(inverted);
    }
    _selectionMode = selectedCount > 0;
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
      await _reloadAutoSync();
      notifyListeners();
    }
    // A new account inherits the app-wide interval; a removed one may have
    // been the only one polling or pushing.
    unawaited(rescheduleBackgroundSync());
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

  Future<void> setSetting(String key, String value) =>
      setSettings({key: value});

  /// Write several settings at once (all or none), then reload once. The
  /// sync timers are only rescheduled when the interval, the scheduler or
  /// the quiet hours are part of the batch.
  Future<void> setSettings(Map<String, String> values) async {
    if (values.isEmpty) return;
    await _core.setSettings(values);
    await _reloadSettings();
    const scheduleKeys = [
      SettingKeys.syncInterval,
      SettingKeys.backgroundScheduler,
      SettingKeys.quietEnabled,
      SettingKeys.quietStart,
      SettingKeys.quietEnd,
    ];
    if (scheduleKeys.any(values.containsKey)) {
      await _reloadAutoSync();
      unawaited(rescheduleBackgroundSync());
    }
  }

  Future<AccountSettings> accountSettings(int accountId) =>
      _core.accountSettings(accountId);

  /// Write one account's overrides (an empty value inherits again), then
  /// reschedule: any of them can change what checks when.
  Future<void> setAccountSettings(
    int accountId,
    Map<String, String> values,
  ) async {
    if (values.isEmpty) return;
    await _core.setAccountSettings(accountId, values);
    if (accountId == _accountId) await _reloadAutoSync();
    unawaited(rescheduleBackgroundSync());
    notifyListeners();
  }

  Future<BackgroundPlan> backgroundPlan() => _core.backgroundPlan();

  Future<void> refreshCapabilities(int accountId) =>
      _queue('Capabilities', () => _core.refreshServerCapabilities(accountId));

  // --- maintenance ---------------------------------------------------------
  //
  // Local-only storage actions for the Maintenance settings section. No
  // network, no job events: the calls run on the bridge worker pool and
  // answer directly. Errors go to the status line, like refused queues.

  Future<Map<String, dynamic>> storageStats(String tempDir) =>
      _core.storageStats(_core.info.dbPath, tempDir);

  Future<void> exportDatabase(String dir) async {
    try {
      final path = await _core.exportDatabaseTo(dir);
      showStatus('Database exported to $path');
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
    }
  }

  Future<void> cleanupTempFiles(String tempDir) async {
    try {
      final done = await _core.cleanupTempFiles(tempDir);
      showStatus('${done['status']}');
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
    }
  }

  /// Delete cached messages past the newest N per folder, then re-read the
  /// lists: trimmed rows are gone from what is showing.
  Future<void> trimLocalCache() async {
    try {
      final removed = await _core.trimLocalCache();
      showStatus(await _core.trimStatus(removed));
      await _reloadMessages();
      await _reloadFolders();
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
    }
  }

  Future<void> evictCachedAttachments() async {
    try {
      final evicted = await _core.evictCachedAttachments();
      showStatus('${evicted['status']}');
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
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
        // The Send job's first progress means SMTP accepted the message.
        if (e.status.isNotEmpty || e.kind == 'Send') {
          showStatus(e.status.isNotEmpty ? e.status : 'Sending…');
        }
      case JobPhase.finished:
        _busyKinds.remove(e.kind);
        if (e.status.isNotEmpty) showStatus(e.status, isError: !e.ok);
        _settlePendingClose(e);
        for (final c in _finishWaiters.remove(e.kind) ?? const []) {
          c.complete(e);
        }
        _refreshFor(e);
    }
    notifyListeners();
  }

  /// Completes with the next finishing event of [kind]. Register before
  /// queueing the job, so a fast finish cannot slip past.
  Future<JobEvent> nextFinished(String kind) {
    final c = Completer<JobEvent>();
    (_finishWaiters[kind] ??= []).add(c);
    return c.future;
  }

  /// A failed job reports no account/folder, so failure matches on kind
  /// alone; success must also name the folder the messages left.
  void _settlePendingClose(JobEvent e) {
    final pending = _pendingClose;
    if (pending == null || e.kind != pending.kind) return;
    if (e.ok) {
      if (e.accountId != pending.accountId || e.folderId != pending.folderId) {
        return;
      }
      if (_openUid == pending.uid && _folderId == pending.folderId) {
        closeMessage();
      }
    }
    _pendingClose = null;
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
    // A sent message's Sent copy and folder refreshes already ran inside
    // `deliver` (`compose::send::refresh_after_send`); the finished event
    // only needs the generic reloads below, as in Qt — no second Sent sync.
    // `-1` for the account means the job changed nothing worth re-reading.
    if (e.accountId < 0 || e.accountId != _accountId) return;
    // A send or sync moves outbox rows: keep the pill honest.
    unawaited(refreshOutbox());
    unawaited(_reloadFolders());
    // A purge or sync can change what the index holds: re-query an active
    // search so hits never go stale (local only, like Qt's job refresh).
    unawaited(_refreshHits());
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
    final gen = _searchGen;
    final target = _similarTarget;
    final List<SearchHit> hits;
    if (target != null) {
      hits = await _core.similar(_accountId, target.folderId, target.uid);
    } else {
      final scope = _searchFolderOnly ? (folder?.path ?? '') : '';
      hits = await _core.search(_accountId, _searchPlan.query, folder: scope);
    }
    if (gen != _searchGen) return;
    _searchHits = hits;
    // Hits that left the results (moved, deleted) leave the selection too.
    if (_selectedHits.isNotEmpty) {
      _selectedHits.retainAll(_shownHits.map((h) => h.key));
      if (_selectedHits.isEmpty) _selectionMode = false;
    }
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
      // A sync dropped it: moved or deleted on another device. Left open,
      // the reader would show a copy no action can reach any more.
      if (_openUid == uid) _openMessageGone();
    }
  }

  /// The open message left the cache underneath the reader: close it (the
  /// narrow layout falls back to the list) and say why.
  void _openMessageGone() {
    closeMessage();
    showStatus('The message was moved or deleted elsewhere');
    unawaited(_reloadMessages());
    unawaited(_reloadFolders());
  }

  /// After a refused action on [uids]: if it was the open message and the
  /// cache no longer has it, the reader goes, like after a sync drops it.
  Future<void> _closeIfOpenGone(List<int> uids) async {
    final uid = _openUid;
    if (uid < 0 || !uids.contains(uid)) return;
    try {
      await _core.message(_folderId, uid);
    } catch (_) {
      if (_openUid == uid) _openMessageGone();
    }
  }

  /// Read the open account's interval, then restart the foreground timer.
  Future<void> _reloadAutoSync() async {
    _autoSyncMinutes = _accountId < 0
        ? 0
        : (await _core.accountSettings(_accountId)).syncIntervalMinutes;
    _rescheduleAutoSync();
  }

  void _rescheduleAutoSync() {
    _autoSyncTimer?.cancel();
    final minutes = _autoSyncMinutes;
    if (minutes <= 0 || _accountId < 0) return;
    _autoSyncTimer = Timer.periodic(
      Duration(minutes: minutes),
      (_) => unawaited(_autoSyncTick()),
    );
  }

  /// Whether someone is looking at the app: a focused desktop window, or
  /// the app in the foreground on Android. Set from the app lifecycle.
  bool _attended = true;

  void setAttended(bool attended) => _attended = attended;

  /// One timer tick. While the app is unattended, the account's quiet hours
  /// hold it back; with someone looking, it always checks.
  Future<void> _autoSyncTick() async {
    if (isSyncing || !hasAccounts) return;
    if (!_attended && (await _core.accountSettings(_accountId)).quietNow) {
      return;
    }
    if (!isSyncing) unawaited(syncAccount());
  }

  /// Re-register the Android background checks from the current settings.
  /// Called after startup and on every interval, scheduler or account
  /// change; the core works out from every account's settings what runs
  /// (push service, one poller, both or neither). Off Android this is a
  /// no-op.
  Future<void> rescheduleBackgroundSync() async {
    if (!Platform.isAndroid) return;
    await scheduleBackgroundChecks();
  }

  /// A background check (push, while the app is open) stored new mail: show
  /// it from the cache, no network.
  Future<void> reloadFromCache() async {
    if (_loading || !hasAccounts) return;
    await _reloadFolders();
    await _reloadMessages();
    notifyListeners();
  }

  /// Open a specific message from a notification tap, switching account
  /// and/or folder as needed. A folder that vanished meanwhile leaves the
  /// user on the account's default instead of stranding them.
  Future<void> openMail({
    required int accountId,
    required int folderId,
    required int uid,
  }) async {
    if (accountId != _accountId) {
      exitSearch();
      final sel = await _core.selectAccount(accountId);
      await _openAccount(sel.accountId, folderId: sel.folderId);
      // Land on the notified folder when it still exists.
      if (folderId != _folderId && _folders.any((f) => f.id == folderId)) {
        await selectFolder(folderId);
      }
    } else if (folderId != _folderId) {
      await selectFolder(folderId);
    }
    if (!_messages.any((m) => m.uid == uid)) {
      unawaited(syncAccount());
      final deadline = DateTime.now().add(const Duration(seconds: 5));
      while (!_messages.any((m) => m.uid == uid) &&
          DateTime.now().isBefore(deadline)) {
        await Future.delayed(const Duration(milliseconds: 200));
      }
    }
    await openMessage(uid);
  }

  Future<void> _openAccount(int accountId, {required int folderId}) async {
    _accountId = accountId;
    _folderId = folderId;
    _openUid = -1;
    _openMessage = null;
    exitSearch();
    await _reloadFolders();
    await _reloadMessages();
    await _reloadAutoSync();
    await refreshOutbox();
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
      _olderState = null;
      _canLoadOlder = false;
    } else {
      // Every cached row, like Qt: mail fetched with "Show older" stays on
      // the list when the folder is opened again.
      int? cached;
      try {
        final counts = await _core.folderCounts(_folderId);
        cached = counts.cached.toInt();
        _serverTotal = counts.server.toInt();
        _olderState = counts.older;
        _canLoadOlder = counts.canLoadOlder;
      } catch (_) {
        _serverTotal = -1;
        _olderState = null;
        _canLoadOlder = false;
      }
      _messages = await _core.messages(_folderId, limit: cached ?? 200);
      _cachedCount = cached ?? _messages.length;
      _pruneSelection();
    }
    notifyListeners();
  }

  /// Drop selected rows that are no longer on screen (gone from the list,
  /// or hidden by a filter), and leave selection mode when nothing remains.
  void _pruneSelection() {
    if (_selectedUids.isEmpty && _selectedHits.isEmpty) return;
    if (searching) {
      // The query is a search, not the folder's substring filter: the
      // folder set only loses rows that left the list.
      _selectedHits.retainAll(_shownHits.map((h) => h.key).toSet());
      _selectedUids.retainAll(_messages.map((m) => m.uid).toSet());
    } else {
      _selectedUids.retainAll(_shownMessages.map((m) => m.uid).toSet());
    }
    if (selectedCount == 0) _selectionMode = false;
  }

  void _patchRow(int uid, MessageSummary Function(MessageSummary) f) {
    _messages = [
      for (final m in _messages)
        if (m.uid == uid) f(m) else m,
    ];
  }

  /// Start a queued job and surface the reason if the core refuses it.
  ///
  /// `kind` must match what the Rust side reports, because that is how the
  /// finishing event clears the spinner again. A refusal is normal (the same
  /// job is already in flight) and does not deserve a dialog — the core
  /// dedupes, so the status line is the whole story.
  Future<void> _queue(
    String kind,
    Future<void> Function() start, {
    bool clearSelection = false,
    List<int> closeUid = const [],
  }) async {
    try {
      await start();
      _busyKinds.add(kind);
      if (clearSelection) {
        _selectedUids.clear();
        _selectionMode = false;
      }
      if (closeUid.contains(_openUid)) {
        _pendingClose = (
          kind: kind,
          accountId: _accountId,
          folderId: _folderId,
          uid: _openUid,
        );
      }
      notifyListeners();
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
      await _closeIfOpenGone(closeUid);
    }
  }

  /// Delete / archive / move through the core's undo queue: the messages
  /// are hidden at once, so this re-reads the lists itself instead of
  /// waiting for a job event. A destroying delete comes back as `purging`
  /// and is tracked like any other job.
  Future<void> _queueUndoable(
    Future<MoveResult> Function() start,
    List<int> uids, {
    int? folderId,
  }) async {
    final fid = folderId ?? _folderId;
    // Only rows of the shown folder can be the open or selected ones.
    final shown = fid == _folderId ? uids : const <int>[];
    try {
      final r = await start();
      if (shown.isNotEmpty) {
        _selectedUids.clear();
        _selectionMode = false;
      }
      if (r.purging) {
        _busyKinds.add('Purge');
        if (shown.contains(_openUid)) {
          _pendingClose = (
            kind: 'Purge',
            accountId: _accountId,
            folderId: _folderId,
            uid: _openUid,
          );
        }
        notifyListeners();
        return;
      }
      if (r.batch.isNotEmpty) {
        if (shown.contains(_openUid)) closeMessage();
        _undoOffer = (batch: r.batch, label: r.label, seq: ++_undoSeq);
        await _reloadMessages();
        await _reloadFolders();
        await _refreshHits();
      }
      showStatus(r.label);
    } catch (e) {
      showStatus(coreErrorText(e), isError: true);
      await _closeIfOpenGone(shown);
    }
  }

  void _ignoreBusy(Object _) {}

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

/// Whether coming back to the app should sync: not when a sync was asked
/// for within [gap] (switching apps briefly, or the startup sync). Pure, so
/// it is unit-tested.
bool shouldSyncOnResume(
  DateTime? lastSync,
  DateTime now, {
  Duration gap = const Duration(minutes: 1),
}) => lastSync == null || now.difference(lastSync) >= gap;
