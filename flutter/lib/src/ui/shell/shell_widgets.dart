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

/// The shell toolbar, laid out like the Qt one: back (one pane) or the
/// sidebar toggle (three panes), Compose, a width-capped search field, then
/// Sync and the tool entries — as icons where there is room, behind an
/// overflow menu where there is not.
class ShellTopBar extends StatelessWidget implements PreferredSizeWidget {
  const ShellTopBar({
    super.key,
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

  /// One-pane layout: Compose shrinks to an icon, the folder-scope toggle
  /// and the tool entries move into the overflow menu.
  final bool narrow;
  final Widget? sidebarToggle;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final folderOnly = context.select<MailState, bool>(
      (s) => s.searchFolderOnly,
    );
    final leading = showBack
        ? IconButton(
            tooltip: 'Folders',
            icon: const Icon(Icons.arrow_back),
            onPressed: onBack,
          )
        : sidebarToggle;
    void compose() => ComposerDialog.showBlank(context);
    return AppBar(
      leading: leading,
      titleSpacing: leading == null ? 8 : 0,
      title: Row(
        children: [
          if (narrow)
            IconButton(
              tooltip: 'Compose (Ctrl+N)',
              color: Theme.of(context).colorScheme.primary,
              icon: const Icon(Icons.edit_outlined),
              onPressed: compose,
            )
          else
            FilledButton.icon(
              icon: const Icon(Icons.edit_outlined, size: 18),
              label: const Text('Compose'),
              onPressed: compose,
            ),
          const SizedBox(width: 8),
          Flexible(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 460),
              child: ShellSearchField(
                focus: searchFocus,
                controller: searchController,
                compact: narrow,
              ),
            ),
          ),
          if (!narrow)
            Tooltip(
              message: 'Search only the current folder',
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Checkbox(
                    value: folderOnly,
                    onChanged: (_) => _toggleScope(context),
                  ),
                  const Text('Folder', style: TextStyle(fontSize: 13)),
                ],
              ),
            ),
        ],
      ),
      actions: [
        IconButton(
          tooltip: 'Sync now (Ctrl+R)',
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
        if (!narrow) ...[
          for (final (value, icon, text) in _tools)
            IconButton(
              tooltip: text,
              icon: Icon(icon),
              onPressed: () => openShellMenu(context, value),
            ),
          const SizedBox(width: 4),
        ] else
          PopupMenuButton<String>(
            tooltip: 'More',
            onSelected: (value) => value == 'scope'
                ? _toggleScope(context)
                : openShellMenu(context, value),
            itemBuilder: (context) => [
              CheckedPopupMenuItem(
                value: 'scope',
                checked: folderOnly,
                child: const Text('Search only this folder'),
              ),
              for (final (value, icon, text) in _tools)
                PopupMenuItem(
                  value: value,
                  child: MenuRow(icon: icon, text: text),
                ),
            ],
          ),
      ],
    );
  }

  /// The tool entries, in the Qt toolbar's order.
  static const _tools = [
    ('folders', Icons.folder_outlined, 'Manage folders'),
    ('contacts', Icons.contacts_outlined, 'Contacts'),
    ('accounts', Icons.person_outline, 'Accounts'),
    ('settings', Icons.settings_outlined, 'Settings'),
  ];

  void _toggleScope(BuildContext context) {
    final state = context.read<MailState>();
    state.runSearch(state.searchQuery, folderOnly: !state.searchFolderOnly);
  }
}

/// Toolbar tool entry. Lives here so the bar does not have to watch the
/// whole [MailState] just to route a tap.
void openShellMenu(BuildContext context, String value) {
  switch (value) {
    case 'folders':
      FolderManagerDialog.show(context);
    case 'contacts':
      ContactsDialog.show(context);
    case 'accounts':
      AccountsDialog.show(context);
    case 'settings':
      SettingsDialog.show(context);
  }
}

/// Account-wide FTS from 3+ letters, or this folder only when the scope
/// toggle is on. Short input keeps the instant folder list — searching the
/// server on every keystroke would be a very expensive autocomplete.
class ShellSearchField extends StatelessWidget {
  const ShellSearchField({
    super.key,
    required this.focus,
    required this.controller,
    this.compact = false,
  });

  final FocusNode focus;
  final TextEditingController controller;

  /// Short placeholder for the one-pane toolbar.
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final query = context.select<MailState, String>((s) => s.searchQuery);
    final folderOnly = context.select<MailState, bool>(
      (s) => s.searchFolderOnly,
    );
    if (controller.text != query && query.isEmpty) {
      controller.clear();
    }
    final hint = switch ((compact, folderOnly)) {
      (true, true) => 'Search folder…',
      (true, false) => 'Search…',
      (false, true) => 'Search this folder… (3+ letters)',
      (false, false) => 'Search mail… (3+ letters)',
    };
    void clear() {
      controller.clear();
      context.read<MailState>().exitSearch();
    }

    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): clear},
      child: TextField(
        controller: controller,
        focusNode: focus,
        textInputAction: TextInputAction.search,
        decoration: InputDecoration(
          hintText: hint,
          suffixIcon: query.isEmpty
              ? null
              : IconButton(
                  tooltip: 'Clear search',
                  icon: const Icon(Icons.close, size: 18),
                  onPressed: clear,
                ),
          border: const OutlineInputBorder(),
          isDense: true,
          contentPadding: const EdgeInsets.symmetric(
            horizontal: 10,
            vertical: 10,
          ),
        ),
        onChanged: (v) => context.read<MailState>().runSearch(v),
        onSubmitted: (v) => context.read<MailState>().runSearch(v),
      ),
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
