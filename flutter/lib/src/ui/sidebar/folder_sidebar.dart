import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../folders/folder_manager_dialog.dart';

/// Accounts on top, this account's folders below.
class FolderSidebar extends StatelessWidget {
  const FolderSidebar({super.key, this.onFolderSelected});

  /// Called after a folder is picked, so a narrow layout can navigate away
  /// from the sidebar. Null in the three-pane layout, where nothing moves.
  final VoidCallback? onFolderSelected;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final folders = state.visibleFolders;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const _AccountPicker(),
        const Divider(height: 1),
        Expanded(
          child: ListView.builder(
            padding: const EdgeInsets.symmetric(vertical: 4),
            itemCount: folders.length,
            itemBuilder: (context, i) {
              final f = folders[i];
              return _FolderTile(
                folder: f,
                selected: f.id == state.folderId,
                onTap: () {
                  state.selectFolder(f.id);
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

class _AccountPicker extends StatelessWidget {
  const _AccountPicker();

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final account = state.account;
    if (account == null) {
      return const Padding(
        padding: EdgeInsets.all(12),
        child: Text('No account'),
      );
    }
    // One account is the common case and a dropdown around it is just noise.
    if (state.accounts.length == 1) {
      return ListTile(
        leading: const Icon(Icons.account_circle_outlined),
        title: Text(account.displayName, overflow: TextOverflow.ellipsis),
        subtitle: Text(account.email, overflow: TextOverflow.ellipsis),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      child: DropdownButtonHideUnderline(
        child: DropdownButton<int>(
          isExpanded: true,
          value: account.id,
          items: [
            for (final a in state.accounts)
              DropdownMenuItem(
                value: a.id,
                child: Text(a.email, overflow: TextOverflow.ellipsis),
              ),
          ],
          onChanged: (id) {
            if (id != null) state.selectAccount(id);
          },
        ),
      ),
    );
  }
}

class _FolderTile extends StatelessWidget {
  const _FolderTile({
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
    return ListTile(
      selected: selected,
      selectedTileColor: scheme.secondaryContainer,
      // Hierarchy lives in the IMAP path, so depth is derived rather than
      // stored: `Work/Client` sits one level in without a tree structure.
      contentPadding: EdgeInsets.only(left: 12.0 + folder.depth * 14, right: 8),
      leading: Icon(_iconFor(folder.role), size: 20),
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
              ? Text('${folder.total}',
                  style: TextStyle(color: scheme.outline, fontSize: 11))
              : null),
      onTap: onTap,
    );
  }

  static IconData _iconFor(FolderRole role) => switch (role) {
        FolderRole.inbox => Icons.inbox_outlined,
        FolderRole.sent => Icons.send_outlined,
        FolderRole.drafts => Icons.edit_note_outlined,
        FolderRole.trash => Icons.delete_outline,
        FolderRole.junk => Icons.report_gmailerrorred_outlined,
        FolderRole.archive => Icons.archive_outlined,
        FolderRole.custom => Icons.folder_outlined,
      };
}
