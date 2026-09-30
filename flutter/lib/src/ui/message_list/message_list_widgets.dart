import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';
import '../menu_row.dart';
import '../move_to/move_to_dialog.dart';

/// Title, count, sort and select menus above the list.
class MessageListHeader extends StatelessWidget {
  const MessageListHeader({super.key});

  @override
  Widget build(BuildContext context) {
    final title = context.select<MailState, String>(
      (s) => s.folder?.leafName ?? 'Messages',
    );
    final count = context.select<MailState, int>((s) => s.messages.length);
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
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: Row(
        children: [
          IconButton(
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
              '$title · $count',
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
    final uids = context.select<MailState, List<int>>(
      (s) => s.selectedUids.toList(growable: false),
    );
    final starred = context.select<MailState, bool>((s) {
      if (s.selectedUids.isEmpty) return false;
      return s.selectedUids.every(
        (u) =>
            s.messages.where((m) => m.uid == u).firstOrNull?.starred ?? false,
      );
    });
    final permanent = context.select<MailState, bool>(
      (s) => s.deleteIsPermanent,
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
              '${uids.length} selected',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(width: 8),
            IconButton(
              tooltip: 'Mark read',
              icon: const Icon(Icons.mark_email_read_outlined, size: 18),
              onPressed: () =>
                  context.read<MailState>().markReadMany(uids, true),
            ),
            IconButton(
              tooltip: 'Mark unread',
              icon: const Icon(Icons.mark_email_unread_outlined, size: 18),
              onPressed: () =>
                  context.read<MailState>().markReadMany(uids, false),
            ),
            IconButton(
              tooltip: starred ? 'Unstar' : 'Star',
              icon: Icon(starred ? Icons.star : Icons.star_border, size: 18),
              onPressed: () =>
                  context.read<MailState>().setStarMany(uids, !starred),
            ),
            IconButton(
              tooltip: 'Archive',
              icon: const Icon(Icons.archive_outlined, size: 18),
              onPressed: () {
                context.read<MailState>().archiveMessages(uids);
                onAction();
              },
            ),
            IconButton(
              tooltip: 'Move to…',
              icon: const Icon(Icons.drive_file_move_outlined, size: 18),
              onPressed: () => MoveToDialog.show(context, uids: uids),
            ),
            IconButton(
              tooltip: 'Move to Trash',
              icon: const Icon(Icons.delete_outline, size: 18),
              onPressed: () => confirmDelete(
                context,
                context.read<MailState>(),
                uids: uids,
                permanent: permanent,
              ),
            ),
            PopupMenuButton<String>(
              icon: const Icon(Icons.more_vert, size: 18),
              onSelected: (v) {
                final state = context.read<MailState>();
                switch (v) {
                  case 'purge':
                    confirmDelete(
                      context,
                      state,
                      uids: uids,
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
}) async {
  if (!purge && !permanent && !state.settings.confirmDelete) {
    await state.deleteMessages(uids);
    return;
  }
  final title = purge || permanent ? 'Delete permanently?' : 'Move to Trash?';
  final what = uids.length > 1
      ? '${uids.length} messages'
      : '“${subjectOf(state, uids.first)}”';
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
  if (purge) {
    await state.purgeMessages(uids);
  } else {
    await state.deleteMessages(uids);
  }
}

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
    final avatarBg = avatarColor(context, message.from);
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
                width: 40,
                child: Row(
                  children: [
                    // Unread marker beside the avatar, like the Qt row's dot
                    // column — bold text alone is too easy to miss.
                    if (message.unread)
                      Container(
                        width: 8,
                        height: 8,
                        margin: const EdgeInsets.only(right: 4),
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: theme.colorScheme.primary,
                        ),
                      )
                    else
                      const SizedBox(width: 12),
                    CircleAvatar(
                      radius: 13,
                      backgroundColor: avatarBg,
                      foregroundColor: theme.colorScheme.onPrimary,
                      child: Text(
                        senderInitial(message.from),
                        style: const TextStyle(fontSize: 12),
                      ),
                    ),
                  ],
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
                  onSelected: (v) => runMessageAction(context, message, v),
                  itemBuilder: (context) => messageActionItems(message),
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

List<PopupMenuEntry<String>> messageActionItems(MessageSummary message) => [
  PopupMenuItem(
    value: 'read',
    child: MenuRow(
      icon: message.unread
          ? Icons.mark_email_read_outlined
          : Icons.mark_email_unread_outlined,
      text: message.unread ? 'Mark as read' : 'Mark as unread',
    ),
  ),
  PopupMenuItem(
    value: 'star',
    child: MenuRow(
      icon: message.starred ? Icons.star : Icons.star_border,
      text: message.starred ? 'Remove star' : 'Star',
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
    items: messageActionItems(message),
  );
  if (choice != null && context.mounted) {
    await runMessageAction(context, message, choice);
  }
}

Future<void> runMessageAction(
  BuildContext context,
  MessageSummary message,
  String action,
) async {
  final state = context.read<MailState>();
  final uid = message.uid;
  switch (action) {
    case 'read':
      await state.setRead(uid, message.unread);
    case 'star':
      await state.toggleStar(uid);
    case 'archive':
      await state.archiveMessages([uid]);
    case 'move':
      if (context.mounted) {
        await MoveToDialog.show(context, uids: [uid], subject: message.subject);
      }
    case 'delete':
      if (context.mounted) {
        await confirmDelete(
          context,
          state,
          uids: [uid],
          permanent: state.deleteIsPermanent,
        );
      }
    case 'purge':
      if (context.mounted) {
        await confirmDelete(
          context,
          state,
          uids: [uid],
          permanent: true,
          purge: true,
        );
      }
  }
}

/// One search hit. Navigates only: mutating a row that lives in another
/// folder from here would act on the wrong mailbox.
class SearchHitTile extends StatelessWidget {
  const SearchHitTile({super.key, required this.hit, this.onOpened});

  final SearchHit hit;
  final VoidCallback? onOpened;

  @override
  Widget build(BuildContext context) {
    void open() {
      context.read<MailState>().jumpToHit(hit);
      onOpened?.call();
    }

    return ListTile(
      title: Row(
        children: [
          Expanded(child: Text(hit.subject, overflow: TextOverflow.ellipsis)),
          if (hit.starred) ...[
            const SizedBox(width: 4),
            Icon(Icons.star, size: 14, color: Colors.amber.shade700),
          ],
        ],
      ),
      subtitle: Text(
        '${hit.from} · ${hit.folder}\n${hit.snippet}',
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
            tooltip: 'Actions',
            icon: const Icon(Icons.more_vert, size: 18),
            onSelected: (v) {
              if (v == 'open') open();
            },
            itemBuilder: (context) => const [
              PopupMenuItem(
                value: 'open',
                child: MenuRow(
                  icon: Icons.open_in_new,
                  text: 'Jump to message',
                ),
              ),
            ],
          ),
        ],
      ),
      onTap: open,
    );
  }
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
