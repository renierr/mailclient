import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

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
      ],
    );
  }
}

/// Who you are, like the Qt account chip: avatar and address in a bordered
/// card. With several accounts it opens a menu to switch between them;
/// adding and managing accounts live in the toolbar's Accounts entry.
class AccountPicker extends StatelessWidget {
  const AccountPicker({super.key});

  @override
  Widget build(BuildContext context) {
    final account = context.select<MailState, Account?>((s) => s.account);
    final accounts = context.select<MailState, List<Account>>(
      (s) => s.accounts,
    );
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final email = account?.email ?? '';
    final canSwitch = accounts.length > 1;
    final chip = Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      decoration: BoxDecoration(
        border: Border.all(color: scheme.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        children: [
          CircleAvatar(
            radius: 14,
            backgroundColor: avatarColor(context, email),
            foregroundColor: scheme.onPrimary,
            child: Text(
              email.isEmpty ? '?' : email[0].toUpperCase(),
              style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  email.isEmpty ? 'No account' : email,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (canSwitch)
                  Text(
                    '${accounts.length} accounts — switch',
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
              ],
            ),
          ),
          if (canSwitch)
            Icon(Icons.expand_more, size: 18, color: scheme.onSurfaceVariant),
        ],
      ),
    );
    return Padding(
      padding: const EdgeInsets.all(8),
      child: canSwitch
          ? PopupMenuButton<int>(
              tooltip: 'Switch account',
              onSelected: (id) => context.read<MailState>().selectAccount(id),
              itemBuilder: (context) => [
                for (final a in accounts)
                  CheckedPopupMenuItem(
                    value: a.id,
                    checked: a.id == account?.id,
                    child: Text(a.email, overflow: TextOverflow.ellipsis),
                  ),
              ],
              child: chip,
            )
          : chip,
    );
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
