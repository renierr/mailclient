import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';
import '../../theme/app_theme.dart';
import '../accounts/account_setup_dialog.dart';
import '../accounts/accounts_dialog.dart';
import '../composer/composer_dialog.dart';
import '../contacts/contacts_dialog.dart';
import '../folders/folder_manager_dialog.dart';
import '../menu_row.dart';
import '../message_list/message_list_pane.dart';
import '../reader/reader_pane.dart';
import '../settings/settings_dialog.dart';
import '../sidebar/folder_sidebar.dart';

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
    final state = context.watch<MailState>();

    if (state.loading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    if (!state.hasAccounts) {
      return const Scaffold(body: _NoAccountsView());
    }

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyN, control: true): () =>
            ComposerDialog.showBlank(context),
        const SingleActivator(LogicalKeyboardKey.keyR, control: true): () =>
            state.syncAccount(),
        const SingleActivator(LogicalKeyboardKey.keyF, control: true): () =>
            _searchFocus.requestFocus(),
        // Qt parity: Delete trashes, Shift+Delete purges, Esc leaves the
        // reader/fullscreen, F11 toggles reader fullscreen.
        const SingleActivator(LogicalKeyboardKey.delete): () =>
            _deleteOpen(state, permanent: false),
        const SingleActivator(LogicalKeyboardKey.delete, shift: true): () =>
            _deleteOpen(state, permanent: true),
        const SingleActivator(LogicalKeyboardKey.escape): () => _escape(state),
        const SingleActivator(LogicalKeyboardKey.f11): () =>
            state.toggleReaderFullscreen(),
      },
      child: Focus(
        autofocus: true,
        child: LayoutBuilder(
          builder: (context, constraints) {
            // Scale-aware breakpoints like Qt (`<720·uiScale` etc.): at 150%
            // text scale a 800px window behaves like a narrow one, otherwise
            // fixed-height rows overflow.
            final scale = state.settings.uiScale;
            final width = constraints.maxWidth;
            final effective = width / scale;
            // Read here, in build, and pass down: provider's watch/select
            // may only run in a build method, not in these helpers called
            // from the layout callback.
            final fullscreen = state.readerFullscreen;
            final openUid = state.openUid;
            final body = effective >= Breakpoints.medium
                ? _threePane(fullscreen)
                : effective >= Breakpoints.compact
                ? _twoPane(fullscreen, openUid, width)
                : _onePane();
            // Android system-back must walk the views (reader → list →
            // folders) instead of closing the app from a nested pane. The
            // order mirrors the visible back affordances: fullscreen first,
            // then search/selection, then the pane stack.
            final backBlocksPop =
                fullscreen ||
                state.searching ||
                state.selectionMode ||
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
                appBar: _TopBar(
                  showBack:
                      effective < Breakpoints.compact && _pane != _Pane.folders,
                  onBack: () => setState(() => _pane = _pane.previous),
                  searchFocus: _searchFocus,
                  searchController: _searchController,
                  narrow: effective < Breakpoints.compact,
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
                    const _StatusBar(),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }

  void _deleteOpen(MailState state, {required bool permanent}) {
    final uid = state.openUid;
    if (uid < 0) return;
    if (permanent) {
      state.purgeMessages([uid]);
    } else {
      state.deleteMessages([uid]);
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
      if (_pane == _Pane.reader) {
        setState(() => _pane = _Pane.list);
      } else if (_pane == _Pane.list) {
        setState(() => _pane = _Pane.folders);
      }
      return;
    }
    if (state.openUid >= 0) state.closeMessage();
  }

  Widget _threePane(bool fullscreen) {
    if (fullscreen) {
      // The exit lives in the reader header, next to where fullscreen was
      // entered — no extra chrome needed here. If the open message vanishes
      // (deleted elsewhere), fall back to the list instead of an empty pane.
      if (context.read<MailState>().openUid < 0) {
        return Row(
          children: [
            if (_sidebarVisible)
              SizedBox(width: _sidebarWidth, child: const FolderSidebar()),
            if (_sidebarVisible)
              _PaneDivider(
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
          _PaneDivider(
            onDelta: (dx) => setState(
              () => _sidebarWidth = (_sidebarWidth + dx).clamp(160.0, 480.0),
            ),
          ),
        SizedBox(width: _listWidth, child: const MessageListPane()),
        _PaneDivider(
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
        _PaneDivider(
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
    _Pane.reader => ReaderPane(onClose: () => _go(_Pane.list)),
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

/// The draggable split between panes, like the Qt SplitView handle.
///
/// The visual line stays 1px but the hit area is ~24px wide: a 9px target is
/// not grabbable on touch screens, and the drag would lose to scrolling.
/// A grip pill appears on hover/drag as the grab affordance.
class _PaneDivider extends StatefulWidget {
  const _PaneDivider({required this.onDelta});

  final ValueChanged<double> onDelta;

  @override
  State<_PaneDivider> createState() => _PaneDividerState();
}

class _PaneDividerState extends State<_PaneDivider> {
  bool _active = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      onEnter: (_) => setState(() => _active = true),
      onExit: (_) => setState(() => _active = false),
      child: GestureDetector(
        behavior: HitTestBehavior.translucent,
        onHorizontalDragStart: (_) => setState(() => _active = true),
        onHorizontalDragEnd: (_) => setState(() => _active = false),
        onHorizontalDragCancel: () => setState(() => _active = false),
        onHorizontalDragUpdate: (d) => widget.onDelta(d.delta.dx),
        // Double-tap resets nothing, but the affordance must say grabbable.
        child: Semantics(
          label: 'Resize panes',
          child: Container(
            width: 24,
            alignment: Alignment.center,
            child: Stack(
              alignment: Alignment.center,
              children: [
                Container(
                  width: _active ? 3 : 1,
                  color: _active
                      ? scheme.primary
                      : Theme.of(context).dividerColor,
                ),
                Container(
                  width: 8,
                  height: 40,
                  decoration: BoxDecoration(
                    color: _active
                        ? scheme.primaryContainer
                        : scheme.surfaceContainerHighest,
                    borderRadius: BorderRadius.circular(4),
                    border: Border.all(color: scheme.outlineVariant),
                  ),
                  child: Icon(
                    Icons.drag_indicator,
                    size: 8,
                    color: scheme.outline,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _TopBar extends StatelessWidget implements PreferredSizeWidget {
  const _TopBar({
    required this.showBack,
    required this.onBack,
    required this.searchFocus,
    required this.searchController,
    required this.narrow,
    this.sidebarToggle,
  });

  final bool showBack;
  final VoidCallback onBack;
  final FocusNode searchFocus;
  final TextEditingController searchController;
  final bool narrow;
  final Widget? sidebarToggle;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final leading = showBack
        ? IconButton(icon: const Icon(Icons.arrow_back), onPressed: onBack)
        : sidebarToggle;
    return AppBar(
      leading: leading,
      // With no leading the title sat flush at the screen edge (titleSpacing
      // 0 + no back button = "INBOX" touching the bezel). Keep 0 only when a
      // leading icon already provides the inset.
      titleSpacing: leading == null ? 16 : 0,
      title: narrow
          // The folders pane has no back button, which is also how we know
          // it is showing: label it 'Mail', not the selected folder.
          ? Text(
              showBack ? (state.folder?.leafName ?? 'Mail') : 'Mail',
              overflow: TextOverflow.ellipsis,
            )
          : _SearchField(focus: searchFocus, controller: searchController),
      actions: [
        if (narrow)
          IconButton(
            tooltip: 'Search',
            icon: const Icon(Icons.search),
            onPressed: () => _searchDialog(context, state),
          ),
        // Wide layouts compose from the sidebar button, like the Qt
        // toolbar; the narrow panes have no sidebar, so they keep an icon.
        if (narrow)
          IconButton(
            tooltip: 'Compose (Ctrl+N)',
            icon: const Icon(Icons.edit_outlined),
            onPressed: () => ComposerDialog.showBlank(context),
          ),
        IconButton(
          tooltip: 'Sync (Ctrl+R)',
          // A spinner in place of the icon, rather than a disabled icon: the
          // control that started the work is where the work should be visible.
          icon: state.isSyncing
              ? const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.sync),
          onPressed: state.isSyncing ? null : state.syncAccount,
        ),
        PopupMenuButton<String>(
          onSelected: (value) => _menu(context, state, value),
          itemBuilder: (context) => const [
            PopupMenuItem(
              value: 'add-account',
              child: MenuRow(
                icon: Icons.person_add_outlined,
                text: 'Add account…',
              ),
            ),
            PopupMenuItem(
              value: 'accounts',
              child: MenuRow(
                icon: Icons.manage_accounts_outlined,
                text: 'Manage accounts…',
              ),
            ),
            PopupMenuItem(
              value: 'folders',
              child: MenuRow(
                icon: Icons.create_new_folder_outlined,
                text: 'Manage IMAP folders…',
              ),
            ),
            PopupMenuItem(
              value: 'refresh-folders',
              child: MenuRow(
                icon: Icons.refresh_outlined,
                text: 'Refresh folder list',
              ),
            ),
            PopupMenuItem(
              value: 'contacts',
              child: MenuRow(icon: Icons.contacts_outlined, text: 'Contacts'),
            ),
            PopupMenuItem(
              value: 'settings',
              child: MenuRow(icon: Icons.settings_outlined, text: 'Settings…'),
            ),
          ],
        ),
      ],
    );
  }

  void _menu(BuildContext context, MailState state, String value) {
    switch (value) {
      case 'add-account':
        AccountSetupDialog.show(context);
      case 'accounts':
        AccountsDialog.show(context);
      case 'folders':
        FolderManagerDialog.show(context);
      case 'refresh-folders':
        state.refreshFolders();
      case 'contacts':
        ContactsDialog.show(context);
      case 'settings':
        SettingsDialog.show(context);
    }
  }

  /// The narrow layout has no room for an inline field; search gets a dialog.
  /// Keyboard-safe: SafeArea + viewInsets padding so the on-screen keyboard
  /// never covers the field, and a scroll wrapper for short screens.
  Future<void> _searchDialog(BuildContext context, MailState state) async {
    final controller = TextEditingController(text: state.searchQuery);
    await showDialog(
      context: context,
      useSafeArea: true,
      builder: (context) => SafeArea(
        child: AnimatedPadding(
          padding: MediaQuery.viewInsetsOf(context),
          duration: const Duration(milliseconds: 150),
          curve: Curves.easeOut,
          child: AlertDialog(
            title: const Text('Search'),
            content: SingleChildScrollView(
              child: TextField(
                controller: controller,
                autofocus: true,
                keyboardType: TextInputType.text,
                textInputAction: TextInputAction.search,
                decoration: const InputDecoration(
                  hintText: '3 or more letters',
                  prefixIcon: Icon(Icons.search),
                ),
                onChanged: (v) => state.runSearch(v),
              ),
            ),
            actions: [
              TextButton(
                onPressed: () {
                  state.exitSearch();
                  Navigator.of(context).pop();
                },
                child: const Text('Clear'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(),
                child: const Text('Done'),
              ),
            ],
          ),
        ),
      ),
    );
    controller.dispose();
  }
}

/// Account-wide FTS from 3+ letters, with an optional folder scope.
/// Short input keeps the instant folder list — searching the server on every
/// keystroke would be a very expensive autocomplete.
class _SearchField extends StatelessWidget {
  const _SearchField({required this.focus, required this.controller});

  final FocusNode focus;
  final TextEditingController controller;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    if (controller.text != state.searchQuery && state.searchQuery.isEmpty) {
      controller.clear();
    }
    return Row(
      children: [
        Expanded(
          child: TextField(
            controller: controller,
            focusNode: focus,
            decoration: InputDecoration(
              hintText: 'Search (3+ letters)',
              prefixIcon: const Icon(Icons.search, size: 18),
              suffixIcon: state.searchQuery.isEmpty
                  ? null
                  : IconButton(
                      icon: const Icon(Icons.clear, size: 18),
                      onPressed: () {
                        controller.clear();
                        state.exitSearch();
                      },
                    ),
              border: const OutlineInputBorder(),
              isDense: true,
              contentPadding: const EdgeInsets.symmetric(
                horizontal: 8,
                vertical: 8,
              ),
            ),
            onChanged: (v) => state.runSearch(v),
            onSubmitted: (v) => state.runSearch(v),
          ),
        ),
        Tooltip(
          message: state.searchFolderOnly
              ? 'Searching this folder — click for the whole account'
              : 'Searching the whole account — click for this folder only',
          child: TextButton(
            onPressed: () => state.runSearch(
              state.searchQuery,
              folderOnly: !state.searchFolderOnly,
            ),
            child: Text(state.searchFolderOnly ? 'Folder' : 'Account'),
          ),
        ),
      ],
    );
  }
}

/// One line of what the core last said. Errors are coloured, not popped up:
/// a failed background sync should not interrupt what the user is reading.
/// Tapping the status bar opens a dialog with the full message and copy button.
class _StatusBar extends StatelessWidget {
  const _StatusBar();

  void _showStatusDialog(BuildContext context, String status, bool isError) {
    showDialog<void>(
      context: context,
      useSafeArea: true,
      builder: (ctx) => SafeArea(
        child: AnimatedPadding(
          padding: MediaQuery.viewInsetsOf(ctx),
          duration: const Duration(milliseconds: 150),
          curve: Curves.easeOut,
          child: AlertDialog(
            title: Row(
              children: [
                Icon(
                  isError ? Icons.error_outline : Icons.info_outline,
                  color: isError ? Theme.of(ctx).colorScheme.error : null,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(isError ? 'Error Details' : 'Status Details'),
                ),
              ],
            ),
            content: ConstrainedBox(
              constraints: BoxConstraints(
                maxHeight: 320,
                maxWidth: MediaQuery.sizeOf(ctx).width - 64,
              ),
              child: SingleChildScrollView(
                child: SelectableText(
                  status,
                  style: TextStyle(
                    fontSize: 13,
                    fontFamily: isError ? 'monospace' : null,
                  ),
                ),
              ),
            ),
            actions: [
              TextButton.icon(
                icon: const Icon(Icons.copy, size: 18),
                label: const Text('Copy to Clipboard'),
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: status));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                      content: Text('Copied to clipboard'),
                      duration: Duration(seconds: 2),
                    ),
                  );
                },
              ),
              TextButton(
                child: const Text('Close'),
                onPressed: () => Navigator.of(ctx).pop(),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final scheme = Theme.of(context).colorScheme;
    // Always visible like the Qt footer: an empty status shows the account,
    // so the bar never pops the layout in and out, and the account is always
    // one glance away. SafeArea keeps it above the gesture bar — the reported
    // "slightly cut off" bottom line.
    final text = state.status.isEmpty
        ? (state.account?.email ?? 'Ready')
        : state.status;
    final isError = state.status.isNotEmpty && state.statusIsError;
    return SafeArea(
      top: false,
      left: false,
      right: false,
      child: Material(
        color: scheme.surfaceContainerHighest,
        child: InkWell(
          onTap: state.status.isEmpty
              ? null
              : () => _showStatusDialog(context, state.status, isError),
          child: Tooltip(
            message: state.status.isEmpty
                ? (state.account?.email ?? '')
                : 'Tap to view full status and copy',
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              child: Row(
                children: [
                  if (isError)
                    Padding(
                      padding: const EdgeInsets.only(right: 6),
                      child: Icon(
                        Icons.error_outline,
                        size: 14,
                        color: scheme.error,
                      ),
                    ),
                  Expanded(
                    child: Text(
                      text,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        color: isError ? scheme.error : scheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                  if (state.status.isNotEmpty) ...[
                    const SizedBox(width: 6),
                    Icon(
                      Icons.open_in_full,
                      size: 13,
                      color: isError ? scheme.error : scheme.onSurfaceVariant,
                    ),
                  ] else if (state.isSyncing)
                    const SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _NoAccountsView extends StatelessWidget {
  const _NoAccountsView();

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.mark_email_unread_outlined, size: 56),
              const SizedBox(height: 16),
              Text(
                'No account yet',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 8),
              const Text('Add an IMAP account to get started.'),
              const SizedBox(height: 20),
              FilledButton.icon(
                icon: const Icon(Icons.add),
                label: const Text('Add account'),
                onPressed: () => AccountSetupDialog.show(context),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
