import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';
import '../../theme/app_theme.dart';
import '../accounts/account_setup_dialog.dart';
import '../message_list/message_list_pane.dart';
import '../reader/reader_pane.dart';
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

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();

    if (state.loading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    if (!state.hasAccounts) {
      return const Scaffold(body: _NoAccountsView());
    }

    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        final body = width >= Breakpoints.medium
            ? _threePane()
            : width >= Breakpoints.compact
                ? _twoPane()
                : _onePane();
        return Scaffold(
          appBar: _TopBar(
            showBack: width < Breakpoints.compact && _pane != _Pane.folders,
            onBack: () => setState(() => _pane = _pane.previous),
          ),
          body: Column(
            children: [
              Expanded(child: body),
              const _StatusBar(),
            ],
          ),
        );
      },
    );
  }

  Widget _threePane() => Row(
        children: [
          const SizedBox(width: 260, child: FolderSidebar()),
          const VerticalDivider(width: 1),
          const SizedBox(width: 360, child: MessageListPane()),
          const VerticalDivider(width: 1),
          const Expanded(child: ReaderPane()),
        ],
      );

  Widget _twoPane() {
    final state = context.watch<MailState>();
    // The reader takes the list's place rather than squeezing a third column
    // into a width where none of them would be usable.
    return Row(
      children: [
        const SizedBox(width: 240, child: FolderSidebar()),
        const VerticalDivider(width: 1),
        Expanded(
          child: state.openUid >= 0
              ? ReaderPane(onClose: state.closeMessage)
              : const MessageListPane(),
        ),
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

class _TopBar extends StatelessWidget implements PreferredSizeWidget {
  const _TopBar({required this.showBack, required this.onBack});

  final bool showBack;
  final VoidCallback onBack;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    return AppBar(
      leading: showBack
          ? IconButton(icon: const Icon(Icons.arrow_back), onPressed: onBack)
          : null,
      title: Text(state.folder?.leafName ?? 'Mail'),
      actions: [
        IconButton(
          tooltip: 'Sync',
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
          onSelected: (value) => switch (value) {
            'add-account' => AccountSetupDialog.show(context),
            'refresh-folders' => state.refreshFolders(),
            _ => null,
          },
          itemBuilder: (context) => const [
            PopupMenuItem(value: 'add-account', child: Text('Add account…')),
            PopupMenuItem(
                value: 'refresh-folders', child: Text('Refresh folder list')),
          ],
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
          Text('No account yet', style: Theme.of(context).textTheme.titleLarge),
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
