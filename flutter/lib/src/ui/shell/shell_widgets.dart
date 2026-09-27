import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';
import '../accounts/account_setup_dialog.dart';
import '../accounts/accounts_dialog.dart';
import '../composer/composer_dialog.dart';
import '../contacts/contacts_dialog.dart';
import '../dialogs/mail_dialog.dart';
import '../folders/folder_manager_dialog.dart';
import '../menu_row.dart';
import '../settings/settings_dialog.dart';

/// The draggable split between panes, like the Qt SplitView handle.
///
/// The visual line stays 1px but the hit area is ~24px wide: a 9px target is
/// not grabbable on touch screens, and the drag would lose to scrolling.
/// A grip pill appears on hover/drag as the grab affordance.
class PaneDivider extends StatefulWidget {
  const PaneDivider({super.key, required this.onDelta});

  final ValueChanged<double> onDelta;

  @override
  State<PaneDivider> createState() => PaneDividerState();
}

class PaneDividerState extends State<PaneDivider> {
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

class ShellTopBar extends StatelessWidget implements PreferredSizeWidget {
  const ShellTopBar({
    super.key,
    required this.showBack,
    required this.onBack,
    required this.searchFocus,
    required this.searchController,
    required this.narrow,
    required this.searchOpen,
    required this.onOpenSearch,
    required this.onCloseSearch,
    this.sidebarToggle,
  });

  final bool showBack;
  final VoidCallback onBack;
  final FocusNode searchFocus;
  final TextEditingController searchController;
  final bool narrow;

  /// Narrow layouts trade the title for an inline search field — no dialog,
  /// so the keyboard can never break it.
  final bool searchOpen;
  final VoidCallback onOpenSearch;
  final VoidCallback onCloseSearch;
  final Widget? sidebarToggle;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final folderName = context.select<MailState, String>(
      (s) => s.folder?.leafName ?? 'Mail',
    );
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final inlineSearch = narrow && searchOpen;
    final leading = inlineSearch
        // While searching, the leading affordance closes the search (and
        // clears it), not the pane navigation underneath.
        ? IconButton(icon: const Icon(Icons.close), onPressed: onCloseSearch)
        : showBack
        ? IconButton(icon: const Icon(Icons.arrow_back), onPressed: onBack)
        : sidebarToggle;
    return AppBar(
      leading: leading,
      // With no leading the title sat flush at the screen edge (titleSpacing
      // 0 + no back button = "INBOX" touching the bezel). Keep 0 only when a
      // leading icon already provides the inset.
      titleSpacing: leading == null ? 16 : 0,
      title: inlineSearch
          ? ShellInlineSearch(focus: searchFocus, controller: searchController)
          : narrow
          // The folders pane has no back button, which is also how we know
          // it is showing: label it 'Mail', not the selected folder.
          ? Text(
              showBack ? folderName : 'Mail',
              overflow: TextOverflow.ellipsis,
            )
          : ShellSearchField(focus: searchFocus, controller: searchController),
      actions: [
        // The search icon becomes the inline field; hide it while open.
        if (narrow && !searchOpen)
          IconButton(
            tooltip: 'Search',
            icon: const Icon(Icons.search),
            onPressed: onOpenSearch,
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
          icon: syncing
              ? const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.sync),
          onPressed: syncing
              ? null
              : () => context.read<MailState>().syncAccount(),
        ),
        PopupMenuButton<String>(
          onSelected: (value) => openShellMenu(context, value),
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
}

/// App-bar overflow menu. Lives here so the bar does not have to watch the
/// whole [MailState] just to route a tap.
void openShellMenu(BuildContext context, String value) {
  final state = context.read<MailState>();
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

/// Narrow-layout search field, living inline in the AppBar title slot instead
/// of a dialog: no route means the keyboard can never break it. The Scaffold
/// resizes, the list pane underneath shows the hits.
class ShellInlineSearch extends StatelessWidget {
  const ShellInlineSearch({
    super.key,
    required this.focus,
    required this.controller,
  });

  final FocusNode focus;
  final TextEditingController controller;

  @override
  Widget build(BuildContext context) {
    final query = context.select<MailState, String>((s) => s.searchQuery);
    if (controller.text != query && query.isEmpty) {
      controller.clear();
    }
    return TextField(
      controller: controller,
      focusNode: focus,
      autofocus: true,
      keyboardType: TextInputType.text,
      textInputAction: TextInputAction.search,
      decoration: InputDecoration(
        hintText: 'Search (3+ letters)',
        border: InputBorder.none,
        suffixIcon: query.isEmpty
            ? null
            : IconButton(
                icon: const Icon(Icons.clear, size: 20),
                onPressed: () {
                  controller.clear();
                  context.read<MailState>().exitSearch();
                },
              ),
      ),
      onChanged: (v) => context.read<MailState>().runSearch(v),
      onSubmitted: (v) => context.read<MailState>().runSearch(v),
    );
  }
}

/// Account-wide FTS from 3+ letters, with an optional folder scope.
/// Short input keeps the instant folder list — searching the server on every
/// keystroke would be a very expensive autocomplete.
class ShellSearchField extends StatelessWidget {
  const ShellSearchField({
    super.key,
    required this.focus,
    required this.controller,
  });

  final FocusNode focus;
  final TextEditingController controller;

  @override
  Widget build(BuildContext context) {
    final query = context.select<MailState, String>((s) => s.searchQuery);
    final folderOnly = context.select<MailState, bool>(
      (s) => s.searchFolderOnly,
    );
    if (controller.text != query && query.isEmpty) {
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
              suffixIcon: query.isEmpty
                  ? null
                  : IconButton(
                      icon: const Icon(Icons.clear, size: 18),
                      onPressed: () {
                        controller.clear();
                        context.read<MailState>().exitSearch();
                      },
                    ),
              border: const OutlineInputBorder(),
              isDense: true,
              contentPadding: const EdgeInsets.symmetric(
                horizontal: 8,
                vertical: 8,
              ),
            ),
            onChanged: (v) => context.read<MailState>().runSearch(v),
            onSubmitted: (v) => context.read<MailState>().runSearch(v),
          ),
        ),
        Tooltip(
          message: folderOnly
              ? 'Searching this folder — click for the whole account'
              : 'Searching the whole account — click for this folder only',
          child: TextButton(
            onPressed: () => context.read<MailState>().runSearch(
              query,
              folderOnly: !folderOnly,
            ),
            child: Text(folderOnly ? 'Folder' : 'Account'),
          ),
        ),
      ],
    );
  }
}

/// One line of what the core last said. Errors are coloured, not popped up:
/// a failed background sync should not interrupt what the user is reading.
/// Tapping the status bar opens a dialog with the full message and copy button.
class StatusBar extends StatelessWidget {
  const StatusBar({super.key});

  void _showStatusDialog(BuildContext context, String status, bool isError) {
    MailDialog.show<void>(
      context,
      builder: (ctx) => AlertDialog(
        title: Row(
          children: [
            Icon(
              isError ? Icons.error_outline : Icons.info_outline,
              color: isError ? Theme.of(ctx).colorScheme.error : null,
            ),
            const SizedBox(width: 8),
            Expanded(child: Text(isError ? 'Error Details' : 'Status Details')),
          ],
        ),
        content: ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: 320,
            maxWidth: MailDialog.maxWidth(ctx, 480),
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
          Wrap(
            alignment: WrapAlignment.end,
            spacing: 8,
            children: [
              TextButton.icon(
                icon: const Icon(Icons.copy, size: 18),
                label: const Text('Copy'),
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
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final status = context.select<MailState, String>((s) => s.status);
    final isError = context.select<MailState, bool>((s) => s.statusIsError);
    final email = context.select<MailState, String>(
      (s) => s.account?.email ?? 'Ready',
    );
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final scheme = Theme.of(context).colorScheme;
    // Always visible like the Qt footer: an empty status shows the account,
    // so the bar never pops the layout in and out, and the account is always
    // one glance away. SafeArea keeps it above the gesture bar — the reported
    // "slightly cut off" bottom line.
    final text = status.isEmpty ? email : status;
    final error = status.isNotEmpty && isError;
    return SafeArea(
      top: false,
      left: false,
      right: false,
      child: Material(
        color: scheme.surfaceContainerHighest,
        child: InkWell(
          onTap: status.isEmpty
              ? null
              : () => _showStatusDialog(context, status, error),
          child: Tooltip(
            message: status.isEmpty
                ? email
                : 'Tap to view full status and copy',
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              child: Row(
                children: [
                  if (error)
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
                        color: error ? scheme.error : scheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                  if (status.isNotEmpty) ...[
                    const SizedBox(width: 6),
                    Icon(
                      Icons.open_in_full,
                      size: 13,
                      color: error ? scheme.error : scheme.onSurfaceVariant,
                    ),
                  ] else if (syncing)
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

/// Empty mailbox: the first-run screen, scrollable so a short phone with the
/// keyboard or a large text scale still reaches the button.
class NoAccountsView extends StatelessWidget {
  const NoAccountsView({super.key});

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
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 8),
              const Text(
                'Add an IMAP account to get started.',
                textAlign: TextAlign.center,
              ),
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
