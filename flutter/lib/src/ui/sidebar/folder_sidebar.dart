import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../accounts/account_setup_dialog.dart';
import '../accounts/accounts_dialog.dart';
import '../composer/composer_dialog.dart';
import '../dialogs/mail_dialog.dart';
import '../folders/folder_manager_dialog.dart';

/// Accounts on top, this account's folders below.
class FolderSidebar extends StatelessWidget {
  const FolderSidebar({super.key, this.onFolderSelected});

  /// Called after a folder is picked, so a narrow layout can navigate away
  /// from the sidebar. Null in the three-pane layout, where nothing moves.
  final VoidCallback? onFolderSelected;

  @override
  Widget build(BuildContext context) {
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.visibleFolders,
    );
    final folderId = context.select<MailState, int>((s) => s.folderId);
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
          child: FilledButton.icon(
            icon: const Icon(Icons.edit_outlined, size: 18),
            label: const Text('Compose'),
            onPressed: () => ComposerDialog.showBlank(context),
          ),
        ),
        const AccountPicker(),
        const Divider(height: 1),
        Expanded(
          child: folders.isEmpty
              ? Center(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Text(
                      syncing
                          ? 'Syncing folders…'
                          : 'No folders yet — sync or manage folders.',
                      textAlign: TextAlign.center,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.outline,
                      ),
                    ),
                  ),
                )
              : ListView.builder(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  itemCount: folders.length,
                  itemBuilder: (context, i) {
                    final f = folders[i];
                    return FolderTile(
                      folder: f,
                      selected: f.id == folderId,
                      onTap: () {
                        context.read<MailState>().selectFolder(f.id);
                        onFolderSelected?.call();
                      },
                    );
                  },
                ),
        ),
        const Divider(height: 1),
        TextButton.icon(
          icon: const Icon(Icons.folder_open_outlined, size: 16),
          label: const Text('Manage folders…'),
          onPressed: () => FolderManagerDialog.show(context),
        ),
      ],
    );
  }
}

class AccountPicker extends StatelessWidget {
  const AccountPicker({super.key});

  @override
  Widget build(BuildContext context) {
    final account = context.select<MailState, Account?>((s) => s.account);
    final accounts = context.select<MailState, List<Account>>(
      (s) => s.accounts,
    );
    if (account == null) {
      return const Padding(
        padding: EdgeInsets.all(12),
        child: Text('No account'),
      );
    }
    final avatar = CircleAvatar(
      radius: 18,
      backgroundColor: avatarColor(context, account.email),
      foregroundColor: Theme.of(context).colorScheme.onPrimary,
      child: Text(
        account.email.isEmpty ? '?' : account.email[0].toUpperCase(),
        style: const TextStyle(fontWeight: FontWeight.w600),
      ),
    );
    // One account is the common case and a dropdown around it is just noise.
    // The chip keeps the Qt account menu (switch / add / manage) in both.
    Widget title(String name, String? sub) => Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(name, overflow: TextOverflow.ellipsis),
        if (sub != null)
          Text(
            sub,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.bodySmall,
          ),
      ],
    );
    if (accounts.length == 1) {
      return PopupMenuButton<String>(
        onSelected: (v) => _menu(context, v),
        itemBuilder: (context) => [
          const PopupMenuItem(value: 'add', child: Text('Add account…')),
          const PopupMenuItem(value: 'manage', child: Text('Manage accounts…')),
        ],
        child: ListTile(
          leading: avatar,
          title: title(account.displayName, account.email),
          trailing: accounts.length > 1
              ? null
              : const Icon(Icons.expand_more, size: 18),
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      child: Row(
        children: [
          avatar,
          const SizedBox(width: 8),
          Expanded(
            child: DropdownButtonHideUnderline(
              child: DropdownButton<int>(
                isExpanded: true,
                value: account.id,
                items: [
                  for (final a in accounts)
                    DropdownMenuItem(
                      value: a.id,
                      child: Text(a.email, overflow: TextOverflow.ellipsis),
                    ),
                ],
                onChanged: (id) {
                  if (id != null) context.read<MailState>().selectAccount(id);
                },
              ),
            ),
          ),
          PopupMenuButton<String>(
            icon: const Icon(Icons.expand_more, size: 18),
            tooltip: 'Account options',
            onSelected: (v) => _menu(context, v),
            itemBuilder: (context) => [
              const PopupMenuItem(value: 'add', child: Text('Add account…')),
              const PopupMenuItem(
                value: 'manage',
                child: Text('Manage accounts…'),
              ),
            ],
          ),
        ],
      ),
    );
  }

  void _menu(BuildContext context, String v) {
    switch (v) {
      case 'add':
        AccountSetupDialog.show(context);
      case 'manage':
        AccountsDialog.show(context);
    }
  }
}

class FolderTile extends StatelessWidget {
  const FolderTile({
    super.key,
    required this.folder,
    required this.selected,
    required this.onTap,
  });

  final Folder folder;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final tile = ListTile(
      selected: selected,
      selectedTileColor: scheme.secondaryContainer,
      // Hierarchy lives in the IMAP path, so depth is derived rather than
      // stored: `Work/Client` sits one level in without a tree structure.
      contentPadding: EdgeInsets.only(left: 12.0 + folder.depth * 14, right: 8),
      leading: Icon(folderIcon(folder.role), size: 20),
      title: Text(
        folder.leafName,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontWeight: folder.unread > 0 ? FontWeight.w600 : FontWeight.normal,
        ),
      ),
      trailing: folder.unread > 0
          ? Badge(label: Text('${folder.unread}'))
          : (folder.total > 0
                ? Text(
                    '${folder.total}',
                    style: TextStyle(color: scheme.outline, fontSize: 11),
                  )
                : null),
      onTap: onTap,
    );
    // Qt parity: tooltip names totals ("12 · 3 unread").
    return Tooltip(
      message: '${folder.total} total · ${folder.unread} unread',
      child: tile,
    );
  }
}
