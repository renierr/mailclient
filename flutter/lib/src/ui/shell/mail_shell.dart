import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';
import '../../theme/app_theme.dart';
import '../composer/composer_dialog.dart';
import '../message_list/message_list_pane.dart';
import '../reader/reader_pane.dart';
import '../sidebar/folder_sidebar.dart';
import 'shell_widgets.dart';

/// The window: sidebar, list and reader, arranged for the width available.
///
/// Three panes side by side on a wide window, sidebar + list with the reader
/// pushed on top in the middle range, and one pane at a time when narrow —
/// which is the phone layout, and equally a desktop window dragged small.
class MailShell extends StatefulWidget {
  const MailShell({super.key});

  @override
  State<MailShell> createState() => _MailShellState();
}

class _MailShellState extends State<MailShell> {
  /// Which pane the narrow layouts are showing. Ignored when there is room
  /// for all three.
  _Pane _pane = _Pane.list;
  final _searchFocus = FocusNode();
  final _searchController = TextEditingController();

  /// Narrow layouts show the search field inline in the AppBar instead of a
  /// dialog (a dialog plus keyboard leaves no room on a phone).
  bool _searchOpen = false;

  /// Layout class of the last build, for the shortcut handlers (which run
  /// outside the LayoutBuilder that knows the width).
  bool _narrow = false;
  bool _wide = true;

  void _focusSearch() {
    // Wide layouts keep a permanent field in the AppBar; only the narrow
    // ones have an inline field to open (and later to close again).
    if (_narrow) setState(() => _searchOpen = true);
    // The inline field may just have appeared; focus it after the frame.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _searchFocus.requestFocus();
    });
  }

  void _closeSearch(MailState state) {
    _searchController.clear();
    state.exitSearch();
    _searchFocus.unfocus();
    setState(() => _searchOpen = false);
  }

  /// Pane widths on the wide layout, dragged at the dividers. Plain fields,
  /// not settings: the Qt SplitView does not remember them either.
  double _sidebarWidth = 260;
  double _listWidth = 380;

  /// Same for the two-pane layout, which used to be a fixed 240px sidebar
  /// behind a static divider — not resizable at all, let alone by touch.
  double _twoPaneSidebarWidth = 240;

  /// Wide-layout sidebar visibility, like the Qt hamburger toggle.
  bool _sidebarVisible = true;

  @override
  void dispose() {
    _searchFocus.dispose();
    _searchController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final loading = context.select<MailState, bool>((s) => s.loading);
    final hasAccounts = context.select<MailState, bool>((s) => s.hasAccounts);
    final scale = context.select<MailState, double>((s) => s.settings.uiScale);
    // The layout and the back handling depend on these, so they must be
    // subscriptions: a plain read here leaves the panes and PopScope stale.
    final fullscreen = context.select<MailState, bool>(
      (s) => s.readerFullscreen,
    );
    final openUid = context.select<MailState, int>((s) => s.openUid);
    final searching = context.select<MailState, bool>((s) => s.searching);
    final selectionMode = context.select<MailState, bool>(
      (s) => s.selectionMode,
    );
    final state = context.read<MailState>();

    if (loading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    if (!hasAccounts) {
      return const Scaffold(body: NoAccountsView());
    }

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyN, control: true): () =>
            ComposerDialog.showBlank(context),
        const SingleActivator(LogicalKeyboardKey.keyR, control: true): () =>
            state.syncAccount(),
        const SingleActivator(LogicalKeyboardKey.keyF, control: true): () =>
            _focusSearch(),
        // Qt parity: Delete trashes, Shift+Delete purges, Esc leaves the
        // reader/fullscreen, F11 toggles reader fullscreen.
        const SingleActivator(LogicalKeyboardKey.delete): () =>
            _deleteShortcut(state, purge: false),
        const SingleActivator(LogicalKeyboardKey.delete, shift: true): () =>
            _deleteShortcut(state, purge: true),
        const SingleActivator(LogicalKeyboardKey.escape): () => _escape(state),
        const SingleActivator(LogicalKeyboardKey.keyZ, control: true): () =>
            state.undoLast(),
        // Fullscreen is a wide-layout mode; narrower ones already give the
        // reader every pixel and offer no way back out of it.
        const SingleActivator(LogicalKeyboardKey.f11): () {
          if (_wide && state.openUid >= 0) state.toggleReaderFullscreen();
        },
      },
      child: Focus(
        autofocus: true,
        child: LayoutBuilder(
          builder: (context, constraints) {
            // Scale-aware breakpoints like Qt (`<720·uiScale` etc.): at 150%
            // text scale a 800px window behaves like a narrow one, otherwise
            // fixed-height rows overflow.
            final width = constraints.maxWidth;
            final effective = width / scale;
            _narrow = effective < Breakpoints.compact;
            _wide = effective >= Breakpoints.medium;
            // Plain assignments, not setState: these are derived from what
            // this very build reads, and everything below uses them.
            if (!_narrow) _searchOpen = false;
            if (_narrow) _syncPaneToOpen(openUid);
            final body = effective >= Breakpoints.medium
                ? _threePane(fullscreen, openUid)
                : effective >= Breakpoints.compact
                ? _twoPane(fullscreen, openUid, width)
                : _onePane();
            // Android system-back must walk the views (reader → list →
            // folders) instead of closing the app from a nested pane. The
            // order mirrors the visible back affordances: fullscreen first,
            // then search/selection, then the pane stack.
            final backBlocksPop =
                fullscreen ||
                _searchOpen ||
                searching ||
                selectionMode ||
                (effective < Breakpoints.compact
                    ? _pane != _Pane.folders
                    : openUid >= 0);
            return PopScope(
              canPop: !backBlocksPop,
              onPopInvokedWithResult: (didPop, _) {
                if (didPop) return;
                _systemBack(state, effective);
              },
              child: Scaffold(
                appBar: ShellTopBar(
                  showBack:
                      effective < Breakpoints.compact && _pane != _Pane.folders,
                  onBack: () => _paneBack(state),
                  searchFocus: _searchFocus,
                  searchController: _searchController,
                  narrow: effective < Breakpoints.compact,
                  searchOpen: _searchOpen,
                  onOpenSearch: _focusSearch,
                  onCloseSearch: () => _closeSearch(state),
                  sidebarToggle: effective >= Breakpoints.medium
                      ? IconButton(
                          tooltip: _sidebarVisible
                              ? 'Hide folders'
                              : 'Show folders',
                          icon: const Icon(Icons.menu),
                          onPressed: () => setState(
                            () => _sidebarVisible = !_sidebarVisible,
                          ),
                        )
                      : null,
                ),
                body: Column(
                  children: [
                    Expanded(child: body),
                    const StatusBar(),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }

  /// Delete / Shift+Delete: the selection when there is one, else the open
  /// message — through the same confirm as the buttons, so the setting and
  /// the always-ask rule for permanent deletes hold for the keyboard too.
  void _deleteShortcut(MailState state, {required bool purge}) {
    final uids = state.selectionMode && state.selectedUids.isNotEmpty
        ? state.selectedUids.toList(growable: false)
        : state.openUid >= 0
        ? [state.openUid]
        : const <int>[];
    if (uids.isEmpty) return;
    confirmDelete(
      context,
      state,
      uids: uids,
      permanent: purge || state.deleteIsPermanent,
      purge: purge,
    );
  }

  /// One-pane layout: the reader pane is showing exactly when a message is
  /// open. Covers messages opened from outside the list (notification tap,
  /// search hit) and messages that close underneath it (moved, deleted).
  void _syncPaneToOpen(int openUid) {
    if (openUid >= 0) {
      _pane = _Pane.reader;
    } else if (_pane == _Pane.reader) {
      _pane = _Pane.list;
    }
  }

  /// Back from a narrow pane. Leaving the reader closes the message, so its
  /// mark-read timer and the Delete/Esc shortcuts stop acting on it.
  void _paneBack(MailState state) {
    if (_pane == _Pane.reader) {
      state.closeMessage();
      _go(_Pane.list);
    } else {
      _go(_pane.previous);
    }
  }

  void _escape(MailState state) {
    if (state.readerFullscreen) {
      state.toggleReaderFullscreen();
      return;
    }
    if (state.openUid >= 0) state.closeMessage();
  }

  /// System-back (Android gesture/button): same steps as the visible back
  /// affordances, innermost first. Called only when the pop was blocked.
  void _systemBack(MailState state, double effective) {
    if (state.readerFullscreen) {
      state.toggleReaderFullscreen();
      return;
    }
    // Inline search field first: one back press leaves search entirely.
    if (_searchOpen) {
      _closeSearch(state);
      return;
    }
    if (state.searching) {
      state.exitSearch();
      return;
    }
    if (state.selectionMode) {
      state.exitSelectionMode();
      return;
    }
    if (effective < Breakpoints.compact) {
      // Narrow stack: reader → list → folders. Folders is the root there,
      // so from it the pop is allowed through (app closes).
      if (_pane != _Pane.folders) _paneBack(state);
      return;
    }
    if (state.openUid >= 0) state.closeMessage();
  }

  Widget _threePane(bool fullscreen, int openUid) {
    if (fullscreen) {
      // The exit lives in the reader header, next to where fullscreen was
      // entered — no extra chrome needed here. If the open message vanishes
      // (deleted elsewhere), fall back to the list instead of an empty pane.
      if (openUid < 0) {
        return Row(
          children: [
            if (_sidebarVisible)
              SizedBox(width: _sidebarWidth, child: const FolderSidebar()),
            if (_sidebarVisible)
              PaneDivider(
                onDelta: (dx) => setState(
                  () =>
                      _sidebarWidth = (_sidebarWidth + dx).clamp(160.0, 480.0),
                ),
              ),
            const Expanded(child: MessageListPane()),
          ],
        );
      }
      return const ReaderPane();
    }
    return Row(
      children: [
        if (_sidebarVisible)
          SizedBox(width: _sidebarWidth, child: const FolderSidebar()),
        if (_sidebarVisible)
          PaneDivider(
            onDelta: (dx) => setState(
              () => _sidebarWidth = (_sidebarWidth + dx).clamp(160.0, 480.0),
            ),
          ),
        SizedBox(width: _listWidth, child: const MessageListPane()),
        PaneDivider(
          onDelta: (dx) => setState(
            () => _listWidth = (_listWidth + dx).clamp(240.0, 700.0),
          ),
        ),
        const Expanded(child: ReaderPane()),
      ],
    );
  }

  Widget _twoPane(bool fullscreen, int openUid, double maxWidth) {
    // The reader takes the list's place rather than squeezing a third column
    // into a width where none of them would be usable.
    final main = openUid >= 0
        ? ReaderPane(onClose: context.read<MailState>().closeMessage)
        : const MessageListPane();
    if (fullscreen) return main;
    // Keep at least ~300px for the main pane: the sidebar cap follows the
    // window, so shrinking the window can never push the list off-screen.
    final cap = (maxWidth - 300).clamp(200.0, 480.0);
    final side = _twoPaneSidebarWidth.clamp(160.0, cap);
    return Row(
      children: [
        SizedBox(width: side, child: const FolderSidebar()),
        PaneDivider(
          onDelta: (dx) => setState(
            () => _twoPaneSidebarWidth = (_twoPaneSidebarWidth + dx).clamp(
              160.0,
              cap,
            ),
          ),
        ),
        Expanded(child: main),
      ],
    );
  }

  Widget _onePane() => switch (_pane) {
    _Pane.folders => FolderSidebar(onFolderSelected: () => _go(_Pane.list)),
    _Pane.list => MessageListPane(onMessageOpened: () => _go(_Pane.reader)),
    _Pane.reader => ReaderPane(
      onClose: () => _paneBack(context.read<MailState>()),
    ),
  };

  void _go(_Pane pane) => setState(() => _pane = pane);
}

enum _Pane {
  folders,
  list,
  reader;

  _Pane get previous => switch (this) {
    _Pane.folders => _Pane.folders,
    _Pane.list => _Pane.folders,
    _Pane.reader => _Pane.list,
  };
}
