import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';
import '../dialogs/sender_avatar.dart';
import '../menu_row.dart';
import '../move_to/move_to_dialog.dart';

/// Title, count, sort and select menus above the list.
class MessageListHeader extends StatelessWidget {
  const MessageListHeader({super.key});

  @override
  Widget build(BuildContext context) {
    final searching = context.select<MailState, bool>((s) => s.searching);
    final folderOnly = context.select<MailState, bool>(
      (s) => s.searchFolderOnly,
    );
    final title = context.select<MailState, String>(
      (s) => s.folder?.leafName ?? 'Messages',
    );
    final count = context.select<MailState, int>(
      (s) => s.searching ? s.searchHits.length : s.messages.length,
    );
    final selectionMode = context.select<MailState, bool>(
      (s) => s.selectionMode,
    );
    final sortField = context.select<MailState, String>(
      (s) => s.settings.sortField,
    );
    final sortDescending = context.select<MailState, bool>(
      (s) => s.settings.sortDescending,
    );
    return Container(
      padding: const EdgeInsets.only(left: 2, right: 4),
      child: Row(
        children: [
          // Narrow hit box, centred over the avatar column below it, so
          // the title starts close to the pane edge.
          IconButton(
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints(minWidth: 34, minHeight: 40),
            tooltip: selectionMode ? 'Leave selection' : 'Select messages',
            icon: Icon(
              selectionMode
                  ? Icons.check_box_outlined
                  : Icons.check_box_outline_blank,
            ),
            onPressed: () {
              final state = context.read<MailState>();
              selectionMode
                  ? state.exitSelectionMode()
                  : state.enterSelectionMode();
            },
          ),
          Expanded(
            child: Text(
              searching
                  ? '$count result(s) in ${folderOnly ? title : 'this account'}'
                  : '$title · $count',
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleSmall,
            ),
          ),
          if (selectionMode)
            PopupMenuButton<String>(
              tooltip: 'Select',
              icon: const Icon(Icons.arrow_drop_down),
              onSelected: (v) {
                final state = context.read<MailState>();
                switch (v) {
                  case 'all':
                    state.selectAllVisible();
                  case 'none':
                    state.exitSelectionMode();
                  case 'unread':
                    state.selectUnread();
                  case 'starred':
                    state.selectStarred();
                  case 'invert':
                    state.invertSelection();
                }
              },
              itemBuilder: (context) => const [
                PopupMenuItem(
                  value: 'all',
                  child: MenuRow(
                    icon: Icons.select_all,
                    text: 'Select all visible',
                  ),
                ),
                PopupMenuItem(
                  value: 'unread',
                  child: MenuRow(
                    icon: Icons.mark_email_unread_outlined,
                    text: 'Select unread',
                  ),
                ),
                PopupMenuItem(
                  value: 'starred',
                  child: MenuRow(
                    icon: Icons.star_border,
                    text: 'Select starred',
                  ),
                ),
                PopupMenuItem(
                  value: 'invert',
                  child: MenuRow(
                    icon: Icons.swap_horiz,
                    text: 'Invert selection',
                  ),
                ),
                PopupMenuItem(
                  value: 'none',
                  child: MenuRow(icon: Icons.clear, text: 'Clear'),
                ),
              ],
            ),
          // Hits come in rank order (grouped by folder); sorting them is
          // not offered.
          if (!searching)
            PopupMenuButton<String>(
              tooltip: 'Sort',
              icon: const Icon(Icons.sort),
              onSelected: (v) {
                final parts = v.split(':');
                context.read<MailState>().setSort(parts[0], parts[1] == 'desc');
              },
              itemBuilder: (context) {
                PopupMenuItem<String> item(
                  String value,
                  IconData icon,
                  String text,
                ) => PopupMenuItem(
                  value: value,
                  child: MenuRow(
                    icon: icon,
                    text:
                        (sortField == value.split(':')[0] &&
                            sortDescending == value.endsWith(':desc'))
                        ? '✓ $text'
                        : text,
                  ),
                );
                return [
                  item('date:desc', Icons.schedule, 'Date, newest first'),
                  item('date:asc', Icons.schedule, 'Date, oldest first'),
                  item('from:asc', Icons.person_outline, 'From A–Z'),
                  item('from:desc', Icons.person_outline, 'From Z–A'),
                  item('subject:asc', Icons.subject, 'Subject A–Z'),
                  item('subject:desc', Icons.subject, 'Subject Z–A'),
                ];
              },
            ),
        ],
      ),
    );
  }
}

/// The selected message(s), and what to do with them.
class BulkActionBar extends StatelessWidget {
  const BulkActionBar({super.key, required this.onAction});

  final VoidCallback onAction;

  @override
  Widget build(BuildContext context) {
    final count = context.select<MailState, int>((s) => s.selectedCount);
    final starred = context.select<MailState, bool>(
      (s) => s.selectionAllStarred,
    );
    final permanent = context.select<MailState, bool>(
      (s) => s.selectionDeleteIsPermanent,
    );
    return Container(
      color: Theme.of(context).colorScheme.secondaryContainer,
      padding: const EdgeInsets.symmetric(horizontal: 4),
      // A narrow window still has to fit every action: scroll instead of
      // clipping, the way the Qt bulk bar collapses.
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            IconButton(
              tooltip: 'Clear selection',
              icon: const Icon(Icons.close, size: 18),
              onPressed: () => context.read<MailState>().exitSelectionMode(),
            ),
            Text(
              '$count selected',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(width: 8),
            IconButton(
              tooltip: 'Mark read',
              icon: const Icon(Icons.mark_email_read_outlined, size: 18),
              onPressed: () => context.read<MailState>().bulkMarkRead(true),
            ),
            IconButton(
              tooltip: 'Mark unread',
              icon: const Icon(Icons.mark_email_unread_outlined, size: 18),
              onPressed: () => context.read<MailState>().bulkMarkRead(false),
            ),
            IconButton(
              tooltip: starred ? 'Unstar' : 'Star',
              icon: Icon(starred ? Icons.star : Icons.star_border, size: 18),
              onPressed: () => context.read<MailState>().bulkStar(!starred),
            ),
            IconButton(
              tooltip: 'Archive',
              icon: const Icon(Icons.archive_outlined, size: 18),
              onPressed: () {
                context.read<MailState>().bulkArchive();
                onAction();
              },
            ),
            IconButton(
              tooltip: 'Move to…',
              icon: const Icon(Icons.drive_file_move_outlined, size: 18),
              onPressed: () => MoveToDialog.showForSelection(context),
            ),
            IconButton(
              tooltip: 'Move to Trash',
              icon: const Icon(Icons.delete_outline, size: 18),
              onPressed: () => confirmSelectionDelete(
                context,
                context.read<MailState>(),
                permanent: permanent,
              ),
            ),
            PopupMenuButton<String>(
              icon: const Icon(Icons.more_vert, size: 18),
              onSelected: (v) {
                final state = context.read<MailState>();
                switch (v) {
                  case 'purge':
                    confirmSelectionDelete(
                      context,
                      state,
                      permanent: true,
                      purge: true,
                    );
                  case 'unread':
                    state.selectUnread();
                  case 'starred':
                    state.selectStarred();
                }
              },
              itemBuilder: (context) => const [
                PopupMenuItem(
                  value: 'purge',
                  child: MenuRow(
                    icon: Icons.delete_forever_outlined,
                    text: 'Delete permanently…',
                  ),
                ),
                PopupMenuItem(
                  value: 'unread',
                  child: MenuRow(
                    icon: Icons.mark_email_unread_outlined,
                    text: 'Select unread',
                  ),
                ),
                PopupMenuItem(
                  value: 'starred',
                  child: MenuRow(
                    icon: Icons.star_border,
                    text: 'Select starred',
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Delete confirm shared by the list, the bulk bar and the reader.
///
/// Trash moves are reversible, so the setting gates them; permanent destroys
/// always ask, whatever the setting says.
Future<void> confirmDelete(
  BuildContext context,
  MailState state, {
  required List<int> uids,
  required bool permanent,
  bool purge = false,
  String? subject,
  int? folderId,
  int? count,
  Future<void> Function()? perform,
}) async {
  Future<void> run() async {
    if (perform != null) return perform();
    if (purge) return state.purgeMessages(uids, folderId: folderId);
    return state.deleteMessages(uids, folderId: folderId);
  }

  if (!purge && !permanent && !state.settings.confirmDelete) {
    await run();
    return;
  }
  final n = count ?? uids.length;
  final title = purge || permanent ? 'Delete permanently?' : 'Move to Trash?';
  final what = n > 1 || uids.isEmpty
      ? '$n message${n == 1 ? '' : 's'}'
      : '“${subject ?? subjectOf(state, uids.first)}”';
  final how = purge || permanent
      ? 'will be destroyed on the server. This cannot be undone.'
      : 'will be moved to Trash.';
  final confirmed =
      await MailDialog.show<bool>(
        context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text('$what $how'),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            // Permanent destruction is danger-red; reversible Trash moves
            // stay the plain filled style, like Qt's intent split.
            FilledButton(
              style: purge || permanent
                  ? MailDialog.dangerStyle(context)
                  : null,
              onPressed: () => Navigator.of(context).pop(true),
              child: Text(
                purge || permanent ? 'Delete permanently' : 'Move to Trash',
              ),
            ),
          ],
        ),
      ) ??
      false;
  if (!confirmed || !context.mounted) return;
  await run();
}

/// [confirmDelete] for the checkbox set of whichever list is showing.
Future<void> confirmSelectionDelete(
  BuildContext context,
  MailState state, {
  required bool permanent,
  bool purge = false,
}) => confirmDelete(
  context,
  state,
  uids: const [],
  count: state.selectedCount,
  permanent: permanent,
  purge: purge,
  perform: purge ? state.bulkPurge : state.bulkDelete,
);

String subjectOf(MailState state, int uid) =>
    state.messages.where((m) => m.uid == uid).firstOrNull?.subject ?? '';

/// One folder row: avatar, flags, subject and the row action menu.
class MessageTile extends StatelessWidget {
  const MessageTile({
    super.key,
    required this.message,
    required this.selected,
    required this.checked,
    required this.selectionMode,
    required this.compact,
    required this.onTap,
    required this.onToggle,
  });

  final MessageSummary message;
  final bool selected;
  final bool checked;
  final bool selectionMode;
  final bool compact;
  final VoidCallback onTap;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final weight = message.unread ? FontWeight.w700 : FontWeight.normal;
    return GestureDetector(
      // Desktop parity: right-click opens the same row menu as ⋮.
      onSecondaryTapDown: (d) =>
          showMessageContextMenu(context, message, d.globalPosition),
      child: ListTile(
        selected: selected,
        selectedTileColor: theme.colorScheme.secondaryContainer,
        // Tight all round so the row uses the pane edge to edge: the
        // sender name and the subject get every pixel left over.
        contentPadding: const EdgeInsets.symmetric(horizontal: 4),
        horizontalTitleGap: 8,
        minLeadingWidth: 0,
        dense: true,
        leading: selectionMode
            ? Checkbox(value: checked, onChanged: (_) => onToggle())
            : SizedBox(
                width: 30,
                // Unread marker as a badge on the avatar's corner, like the
                // Qt row — bold text alone is too easy to miss, and a column
                // of its own would cost the row its left edge.
                child: Badge(
                  isLabelVisible: message.unread,
                  smallSize: 10,
                  alignment: AlignmentDirectional.topStart,
                  offset: const Offset(-2, -2),
                  backgroundColor: theme.colorScheme.primary,
                  child: SenderAvatar(
                    badge: message.badge,
                    radius: 13,
                    fontSize: 10,
                  ),
                ),
              ),
        onTap: onTap,
        title: Row(
          children: [
            Expanded(
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Flexible(
                    child: Text(
                      message.senderName,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        fontWeight: weight,
                      ),
                    ),
                  ),
                  if (message.starred) ...[
                    const SizedBox(width: 4),
                    Icon(Icons.star, size: 14, color: Colors.amber.shade700),
                  ],
                ],
              ),
            ),
            // Always top right, kept small so the name keeps its room.
            const SizedBox(width: 4),
            Text(
              message.date,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.outline,
              ),
            ),
          ],
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                if (message.hasAttachments) ...[
                  Icon(
                    Icons.attach_file,
                    size: 14,
                    color: theme.colorScheme.outline,
                  ),
                  const SizedBox(width: 4),
                ],
                Expanded(
                  child: Text(
                    message.subject,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodyMedium?.copyWith(
                      fontWeight: weight,
                    ),
                  ),
                ),
                // One line below the date, slimmed to its icon at the
                // same edge; the whole row stays tappable, so the
                // smaller hit area costs nothing.
                PopupMenuButton<String>(
                  tooltip: 'Message actions',
                  padding: const EdgeInsets.all(4),
                  icon: const Icon(Icons.more_vert, size: 18),
                  onSelected: (v) => runMessageAction(
                    context,
                    v,
                    uid: message.uid,
                    subject: message.subject,
                    unread: message.unread,
                  ),
                  itemBuilder: (context) => messageActionItems(
                    unread: message.unread,
                    starred: message.starred,
                  ),
                ),
              ],
            ),
            if (!compact && message.snippet.isNotEmpty)
              Text(
                message.snippet,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

List<PopupMenuEntry<String>> messageActionItems({
  required bool unread,
  required bool starred,
}) => [
  PopupMenuItem(
    value: 'read',
    child: MenuRow(
      icon: unread
          ? Icons.mark_email_read_outlined
          : Icons.mark_email_unread_outlined,
      text: unread ? 'Mark as read' : 'Mark as unread',
    ),
  ),
  PopupMenuItem(
    value: 'star',
    child: MenuRow(
      icon: starred ? Icons.star : Icons.star_border,
      text: starred ? 'Remove star' : 'Star',
    ),
  ),
  const PopupMenuItem(
    value: 'archive',
    child: MenuRow(icon: Icons.archive_outlined, text: 'Archive'),
  ),
  const PopupMenuItem(
    value: 'move',
    child: MenuRow(icon: Icons.drive_file_move_outlined, text: 'Move to…'),
  ),
  const PopupMenuItem(
    value: 'delete',
    child: MenuRow(icon: Icons.delete_outline, text: 'Move to Trash'),
  ),
  const PopupMenuItem(
    value: 'purge',
    child: MenuRow(
      icon: Icons.delete_forever_outlined,
      text: 'Delete permanently…',
    ),
  ),
];

Future<void> showMessageContextMenu(
  BuildContext context,
  MessageSummary message,
  Offset at,
) async {
  final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
  final choice = await showMenu<String>(
    context: context,
    position: RelativeRect.fromRect(
      Rect.fromPoints(at, at),
      Offset.zero & overlay.size,
    ),
    items: messageActionItems(unread: message.unread, starred: message.starred),
  );
  if (choice != null && context.mounted) {
    await runMessageAction(
      context,
      choice,
      uid: message.uid,
      subject: message.subject,
      unread: message.unread,
    );
  }
}

/// One row action on [uid]. `folderId` is the shown folder unless given;
/// a search hit passes the folder it lives in.
Future<void> runMessageAction(
  BuildContext context,
  String action, {
  required int uid,
  required String subject,
  required bool unread,
  int? folderId,
}) async {
  final state = context.read<MailState>();
  switch (action) {
    case 'read':
      await state.setRead(uid, unread, folderId: folderId);
    case 'star':
      await state.toggleStar(uid, folderId: folderId);
    case 'archive':
      await state.archiveMessages([uid], folderId: folderId);
    case 'move':
      if (context.mounted) {
        await MoveToDialog.show(
          context,
          uids: [uid],
          subject: subject,
          folderId: folderId,
        );
      }
    case 'delete':
      if (context.mounted) {
        await confirmDelete(
          context,
          state,
          uids: [uid],
          subject: subject,
          folderId: folderId,
          permanent: state.deleteIsPermanentIn(folderId ?? state.folderId),
        );
      }
    case 'purge':
      if (context.mounted) {
        await confirmDelete(
          context,
          state,
          uids: [uid],
          subject: subject,
          folderId: folderId,
          permanent: true,
          purge: true,
        );
      }
  }
}

/// One search hit. Tapping opens it; the ⋮ / right-click menu offers the
/// folder row's actions, aimed at the folder the hit lives in.
class SearchHitTile extends StatelessWidget {
  const SearchHitTile({
    super.key,
    required this.hit,
    required this.checked,
    required this.selectionMode,
    this.onOpened,
  });

  final SearchHit hit;
  final bool checked;
  final bool selectionMode;
  final VoidCallback? onOpened;

  @override
  Widget build(BuildContext context) {
    void open() {
      FocusManager.instance.primaryFocus?.unfocus();
      context.read<MailState>().jumpToHit(hit);
      onOpened?.call();
    }

    Future<void> act(String action) async {
      if (action == 'open') return open();
      final folderId = await context.read<MailState>().folderIdOfHit(hit);
      if (!context.mounted) return;
      await runMessageAction(
        context,
        action,
        uid: hit.uid,
        subject: hit.subject,
        unread: hit.unread,
        folderId: folderId,
      );
    }

    List<PopupMenuEntry<String>> items() => [
      const PopupMenuItem(
        value: 'open',
        child: MenuRow(icon: Icons.open_in_new, text: 'Open message'),
      ),
      const PopupMenuDivider(),
      ...messageActionItems(unread: hit.unread, starred: hit.starred),
    ];

    final tile = ListTile(
      title: Row(
        children: [
          Expanded(child: Text(hit.subject, overflow: TextOverflow.ellipsis)),
          if (hit.starred) ...[
            const SizedBox(width: 4),
            Icon(Icons.star, size: 14, color: Colors.amber.shade700),
          ],
        ],
      ),
      leading: selectionMode
          ? Checkbox(
              value: checked,
              onChanged: (_) => context.read<MailState>().toggleSelectHit(hit),
            )
          : null,
      subtitle: Text(
        '${hit.from}\n${hit.snippet}',
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
      ),
      isThreeLine: true,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (hit.unread)
            Container(
              width: 8,
              height: 8,
              margin: const EdgeInsets.only(right: 8),
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: Theme.of(context).colorScheme.primary,
              ),
            ),
          PopupMenuButton<String>(
            tooltip: 'Message actions',
            icon: const Icon(Icons.more_vert, size: 18),
            onSelected: act,
            itemBuilder: (context) => items(),
          ),
        ],
      ),
      onTap: selectionMode
          ? () => context.read<MailState>().toggleSelectHit(hit)
          : open,
    );
    return GestureDetector(
      // Desktop parity with folder rows: right-click opens the same menu.
      onSecondaryTapDown: (d) async {
        final overlay =
            Overlay.of(context).context.findRenderObject() as RenderBox;
        final choice = await showMenu<String>(
          context: context,
          position: RelativeRect.fromRect(
            Rect.fromPoints(d.globalPosition, d.globalPosition),
            Offset.zero & overlay.size,
          ),
          items: items(),
        );
        if (choice != null) await act(choice);
      },
      child: tile,
    );
  }
}

/// Heads one folder's hits in account-wide results.
class SearchFolderHeader extends StatelessWidget {
  const SearchFolderHeader({super.key, required this.folder});

  final String folder;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      width: double.infinity,
      color: theme.colorScheme.surfaceContainerHighest,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: Text(
        folder,
        overflow: TextOverflow.ellipsis,
        style: theme.textTheme.labelMedium?.copyWith(
          color: theme.colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// Account-wide hits grouped by folder: folders in the order of their best
/// hit, rank order kept inside each. Strings are the folder headers.
List<Object> groupHitsByFolder(List<SearchHit> hits) {
  final groups = <String, List<SearchHit>>{};
  for (final h in hits) {
    (groups[h.folder] ??= []).add(h);
  }
  return [
    for (final g in groups.entries) ...[g.key, ...g.value],
  ];
}

class LoadOlderTile extends StatelessWidget {
  const LoadOlderTile({super.key});

  @override
  Widget build(BuildContext context) {
    final cached = context.select<MailState, int>((s) => s.cachedCount);
    final server = context.select<MailState, int>((s) => s.serverTotal);
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final canAsk = context.select<MailState, bool>((s) => s.folderId >= 0);
    // The list pages the cache, so cached rows can still be off the list.
    final hidden = context.select<MailState, bool>(
      (s) => s.cachedCount > s.messages.length,
    );
    final label = server < 0
        ? 'Cached $cached (server not checked)'
        : cached >= server
        ? 'All $cached loaded'
        : 'Cached $cached of $server';
    final canLoad = canAsk && (hidden || server < 0 || server > cached);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        children: [
          Text(label, style: Theme.of(context).textTheme.bodySmall),
          const SizedBox(height: 4),
          TextButton.icon(
            icon: const Icon(Icons.history, size: 18),
            label: const Text('Show older messages'),
            onPressed: syncing || !canLoad
                ? null
                : () => context.read<MailState>().loadOlderMessages(),
          ),
        ],
      ),
    );
  }
}

class EmptyPane extends StatelessWidget {
  const EmptyPane({super.key, required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 40, color: scheme.outlineVariant),
            const SizedBox(height: 8),
            Text(
              text,
              textAlign: TextAlign.center,
              style: TextStyle(color: scheme.outline),
            ),
          ],
        ),
      ),
    );
  }
}
