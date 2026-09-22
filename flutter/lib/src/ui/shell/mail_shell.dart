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
      },
      child: Focus(
        autofocus: true,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final width = constraints.maxWidth;
            // Read here, in build, and pass down: provider's watch/select
            // may only run in a build method, not in these helpers called
            // from the layout callback.
            final fullscreen = state.readerFullscreen;
            final openUid = state.openUid;
            final body = width >= Breakpoints.medium
                ? _threePane(fullscreen)
                : width >= Breakpoints.compact
                    ? _twoPane(fullscreen, openUid)
                    : _onePane();
            return Scaffold(
              appBar: _TopBar(
                showBack:
                    width < Breakpoints.compact && _pane != _Pane.folders,
                onBack: () => setState(() => _pane = _pane.previous),
                searchFocus: _searchFocus,
                searchController: _searchController,
                narrow: width < Breakpoints.compact,
              ),
              body: Column(
                children: [
                  Expanded(child: body),
                  const _StatusBar(),
                ],
              ),
            );
          },
        ),
      ),
    );
  }

  Widget _threePane(bool fullscreen) {
    if (fullscreen) {
      // The exit lives in the reader header, next to where fullscreen was
      // entered — no extra chrome needed here.
      return const ReaderPane();
    }
    return Row(
      children: [
        SizedBox(width: _sidebarWidth, child: const FolderSidebar()),
        _PaneDivider(
          onDelta: (dx) => setState(() => _sidebarWidth =
              (_sidebarWidth + dx).clamp(160.0, 480.0)),
        ),
        SizedBox(width: _listWidth, child: const MessageListPane()),
        _PaneDivider(
          onDelta: (dx) => setState(() =>
              _listWidth = (_listWidth + dx).clamp(240.0, 700.0)),
        ),
        const Expanded(child: ReaderPane()),
      ],
    );
  }

  Widget _twoPane(bool fullscreen, int openUid) {
    // The reader takes the list's place rather than squeezing a third column
    // into a width where none of them would be usable.
    final main = openUid >= 0
        ? ReaderPane(onClose: context.read<MailState>().closeMessage)
        : const MessageListPane();
    if (fullscreen) return main;
    return Row(
      children: [
        const SizedBox(width: 240, child: FolderSidebar()),
        const VerticalDivider(width: 1),
        Expanded(child: main),
      ],
    );
  }

  Widget _onePane() => switch (_pane) {
        _Pane.folders =>
          FolderSidebar(onFolderSelected: () => _go(_Pane.list)),
        _Pane.list =>
          MessageListPane(onMessageOpened: () => _go(_Pane.reader)),
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

/// The draggable split between panes: a visible divider that resizes on
/// horizontal drag, like the Qt SplitView handle.
class _PaneDivider extends StatelessWidget {
  const _PaneDivider({required this.onDelta});

  final ValueChanged<double> onDelta;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      child: GestureDetector(
        behavior: HitTestBehavior.translucent,
        onHorizontalDragUpdate: (d) => onDelta(d.delta.dx),
        child: Container(
          width: 9,
          alignment: Alignment.center,
          child: Container(
            width: 1,
            color: Theme.of(context).dividerColor,
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
  });

  final bool showBack;
  final VoidCallback onBack;
  final FocusNode searchFocus;
  final TextEditingController searchController;
  final bool narrow;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    return AppBar(
      leading: showBack
          ? IconButton(icon: const Icon(Icons.arrow_back), onPressed: onBack)
          : null,
      titleSpacing: 0,
      title: narrow
          ? Text(state.folder?.leafName ?? 'Mail',
              overflow: TextOverflow.ellipsis)
          : _SearchField(
              focus: searchFocus,
              controller: searchController,
            ),
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
                  icon: Icons.person_add_outlined, text: 'Add account…'),
            ),
            PopupMenuItem(
              value: 'accounts',
              child: MenuRow(
                  icon: Icons.manage_accounts_outlined,
                  text: 'Manage accounts…'),
            ),
            PopupMenuItem(
              value: 'folders',
              child: MenuRow(
                  icon: Icons.create_new_folder_outlined,
                  text: 'Manage IMAP folders…'),
            ),
            PopupMenuItem(
              value: 'refresh-folders',
              child: MenuRow(
                  icon: Icons.refresh_outlined,
                  text: 'Refresh folder list'),
            ),
            PopupMenuItem(
              value: 'contacts',
              child:
                  MenuRow(icon: Icons.contacts_outlined, text: 'Contacts'),
            ),
            PopupMenuItem(
              value: 'settings',
              child:
                  MenuRow(icon: Icons.settings_outlined, text: 'Settings…'),
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
  Future<void> _searchDialog(BuildContext context, MailState state) async {
    final controller =
        TextEditingController(text: state.searchQuery);
    await showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Search'),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: const InputDecoration(
            hintText: '3 or more letters',
            prefixIcon: Icon(Icons.search),
          ),
          onChanged: (v) => state.runSearch(v),
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
    if (controller.text != state.searchQuery &&
        state.searchQuery.isEmpty) {
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
                  horizontal: 8, vertical: 8),
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
            onPressed: () => state.runSearch(state.searchQuery,
                folderOnly: !state.searchFolderOnly),
            child: Text(state.searchFolderOnly ? 'Folder' : 'Account'),
          ),
        ),
      ],
    );
  }
}

/// One line of what the core last said. Errors are coloured, not popped up:
/// a failed background sync should not interrupt what the user is reading.
class _StatusBar extends StatelessWidget {
  const _StatusBar();

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    if (state.status.isEmpty) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      color: scheme.surfaceContainerHighest,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: Text(
        state.status,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 12,
          color: state.statusIsError ? scheme.error : scheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

class _NoAccountsView extends StatelessWidget {
  const _NoAccountsView();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.mark_email_unread_outlined, size: 56),
          const SizedBox(height: 16),
          Text('No account yet',
              style: Theme.of(context).textTheme.titleLarge),
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
    );
  }
}
