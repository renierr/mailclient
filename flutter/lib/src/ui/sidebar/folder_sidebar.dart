import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';
import '../dialogs/sender_avatar.dart';

/// Accounts on top, this account's folders below. Parents collapse (default
/// closed); well-known folders stay visible inside a collapsed parent and a
/// collapsed parent aggregates its hidden children's counts.
class FolderSidebar extends StatefulWidget {
  const FolderSidebar({super.key, this.onFolderSelected});

  /// Called after a folder is picked, so a narrow layout can navigate away
  /// from the sidebar. Null in the three-pane layout, where nothing moves.
  final VoidCallback? onFolderSelected;

  @override
  State<FolderSidebar> createState() => _FolderSidebarState();
}

class _FolderSidebarState extends State<FolderSidebar> {
  /// Expanded parents, by folder id. In-memory: every launch starts
  /// collapsed (default closed).
  final Set<int> _expanded = {};

  @override
  Widget build(BuildContext context) {
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.visibleFolders,
    );
    final folderId = context.select<MailState, int>((s) => s.folderId);
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final rows = collapseFolders(folders, _expanded);
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
                  itemCount: rows.length,
                  itemBuilder: (context, i) {
                    final row = rows[i];
                    final f = row.folder;
                    return FolderTile(
                      folder: f,
                      selected: f.id == folderId,
                      unread: row.unread,
                      total: row.total,
                      hasChildren: row.hasChildren,
                      expanded: row.expanded,
                      onToggle: () => setState(() {
                        if (!_expanded.remove(f.id)) _expanded.add(f.id);
                      }),
                      onTap: () {
                        context.read<MailState>().selectFolder(f.id);
                        widget.onFolderSelected?.call();
                      },
                    );
                  },
                ),
        ),
      ],
    );
  }
}

/// One visible sidebar row: the folder plus its collapse state and the counts
/// to paint (a collapsed parent aggregates its hidden children's counts, so
/// no unread badge disappears with them).
class FolderRow {
  const FolderRow({
    required this.folder,
    required this.hasChildren,
    required this.expanded,
    required this.unread,
    required this.total,
  });

  final Folder folder;
  final bool hasChildren;
  final bool expanded;
  final int unread;
  final int total;
}

/// The feed `leaf` is the last path segment; what precedes it (minus the
/// single-char IMAP delimiter) is the parent. Depth 0 has none.
String? parentPathOf(Folder f) {
  if (f.depth <= 0) return null;
  final cut = f.path.length - f.leafName.length - 1;
  return cut > 0 ? f.path.substring(0, cut) : null;
}

/// Fold the flat visible list into sidebar rows: top-level and well-known
/// folders (`alwaysVisible`, e.g. an Archive filed below INBOX) always show;
/// custom subfolders show only while every ancestor up to the nearest
/// always-visible one is expanded. A folder whose parent is not visible
/// reads as a root, the way the old flat list showed it.
List<FolderRow> collapseFolders(List<Folder> folders, Set<int> expanded) {
  final byPath = {for (final f in folders) f.path: f};
  Folder? parentOf(Folder f) => byPath[parentPathOf(f)];

  bool shown(Folder f) {
    if (f.depth <= 0 || f.alwaysVisible) return true;
    final p = parentOf(f);
    if (p == null) return true;
    return expanded.contains(p.id) && shown(p);
  }

  bool hasKids(Folder f) => folders.any((o) => parentOf(o)?.id == f.id);

  bool under(Folder row, Folder d) {
    Folder? q = d;
    while (q != null) {
      if (q.id == row.id) return true;
      q = parentOf(q);
    }
    return false;
  }

  final rows = <FolderRow>[];
  for (final f in folders) {
    if (!shown(f)) continue;
    final kids = hasKids(f);
    final open = expanded.contains(f.id);
    var unread = f.unread;
    var total = f.total;
    if (kids && !open) {
      for (final d in folders) {
        if (d.id == f.id || shown(d) || !under(f, d)) continue;
        unread += d.unread;
        total += d.total;
      }
    }
    rows.add(
      FolderRow(
        folder: f,
        hasChildren: kids,
        expanded: open,
        unread: unread,
        total: total,
      ),
    );
  }
  return rows;
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
          SenderAvatar(
            badge: account?.badge ?? SenderBadge.none,
            radius: 14,
            fontSize: 12,
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
    this.unread,
    this.total,
    this.hasChildren = false,
    this.expanded = false,
    this.onToggle,
  });

  final Folder folder;
  final bool selected;
  final VoidCallback onTap;

  /// Painted counts: a collapsed parent aggregates its hidden children's.
  final int? unread;
  final int? total;

  final bool hasChildren;
  final bool expanded;
  final VoidCallback? onToggle;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final unreadCount = unread ?? folder.unread;
    final totalCount = total ?? folder.total;
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
          fontWeight: unreadCount > 0 ? FontWeight.w600 : FontWeight.normal,
        ),
      ),
      // Counts first, collapse chevron last: the label edge never moves
      // whether a row has children or not, and the slot stays reserved so
      // counts align down the list.
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (unreadCount > 0)
            Badge(label: Text('$unreadCount'))
          else if (totalCount > 0)
            Text(
              '$totalCount',
              style: TextStyle(color: scheme.outline, fontSize: 11),
            ),
          if (hasChildren)
            IconButton(
              tooltip: expanded ? 'Collapse subfolders' : 'Expand subfolders',
              constraints: const BoxConstraints.tightFor(width: 28, height: 28),
              padding: EdgeInsets.zero,
              iconSize: 18,
              icon: Icon(
                expanded ? Icons.expand_more : Icons.chevron_right,
                color: scheme.onSurfaceVariant,
              ),
              onPressed: onToggle,
            )
          else
            const SizedBox(width: 28),
        ],
      ),
      onTap: onTap,
    );
    // Qt parity: tooltip names totals ("12 · 3 unread").
    return Tooltip(
      message: '$totalCount total · $unreadCount unread',
      child: tile,
    );
  }
}
