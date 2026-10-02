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
    final hasFilter = context.select<MailState, bool>((s) => s.hasListFilter);
    final filterUnread = context.select<MailState, bool>((s) => s.filterUnread);
    final filterStarred = context.select<MailState, bool>(
      (s) => s.filterStarred,
    );
    final filterAttachments = context.select<MailState, bool>(
      (s) => s.filterAttachments,
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
          // Quick filters narrow the current list (AND-combined), in the
          // folder list and in search results alike — the Qt `filterMenu`
          // twin. The menu closes per toggle; the icon stays tinted while
          // anything is active.
          PopupMenuButton<String>(
            tooltip: hasFilter ? 'Filter: active' : 'Filter messages',
            icon: Icon(
              Icons.filter_list,
              color: hasFilter ? Theme.of(context).colorScheme.primary : null,
            ),
            onSelected: (v) {
              final state = context.read<MailState>();
              switch (v) {
                case 'unread':
                  state.setFilterUnread(!state.filterUnread);
                case 'starred':
                  state.setFilterStarred(!state.filterStarred);
                case 'attachments':
                  state.setFilterAttachments(!state.filterAttachments);
                case 'clear':
                  state.clearListFilters();
              }
            },
            itemBuilder: (context) => [
              CheckedPopupMenuItem(
                value: 'unread',
                checked: filterUnread,
                child: const MenuRow(
                  icon: Icons.mark_email_unread_outlined,
                  text: 'Unread only',
                ),
              ),
              CheckedPopupMenuItem(
                value: 'starred',
                checked: filterStarred,
                child: const MenuRow(
                  icon: Icons.star_border,
                  text: 'Starred only',
                ),
              ),
              CheckedPopupMenuItem(
                value: 'attachments',
                checked: filterAttachments,
                child: const MenuRow(
                  icon: Icons.attach_file,
                  text: 'With attachments',
                ),
              ),
              if (hasFilter) ...[
                const PopupMenuDivider(),
                const PopupMenuItem(
                  value: 'clear',
                  child: MenuRow(icon: Icons.clear, text: 'Clear filters'),
                ),
              ],
            ],
          ),
          // Hits come in newest-first order (grouped by folder); sorting them is
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

/// One folder row: avatar, flags, subject and the row action menu. Search
/// hits draw with it too, passing their own menu ([menuItems] + [onMenu]).
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
    this.menuItems,
    this.onMenu,
  });

  final MessageSummary message;
  final bool selected;
  final bool checked;
  final bool selectionMode;
  final bool compact;
  final VoidCallback onTap;
  final VoidCallback onToggle;

  /// Row menu override; null offers the folder row's actions on [message].
  final List<PopupMenuEntry<String>> Function()? menuItems;
  final Future<void> Function(String action)? onMenu;

  List<PopupMenuEntry<String>> _items() =>
      menuItems?.call() ??
      messageActionItems(unread: message.unread, starred: message.starred);

  Future<void> _run(BuildContext context, String action) =>
      onMenu?.call(action) ??
      runMessageAction(
        context,
        action,
        uid: message.uid,
        subject: message.subject,
        unread: message.unread,
      );

  Future<void> _showContextMenu(BuildContext context, Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      context: context,
      position: RelativeRect.fromRect(
        Rect.fromPoints(at, at),
        Offset.zero & overlay.size,
      ),
      items: _items(),
    );
    if (choice != null && context.mounted) await _run(context, choice);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final weight = message.unread ? FontWeight.w700 : FontWeight.normal;
    // Same paperclip the Qt row shows under its avatar (Material
    // attach_file, U+E226 like QML `Icons.attachFile`), kept below the
    // avatar instead of inline before the text. Generous on purpose: the
    // avatar and the glyph dwarf a tight gap, and downscaled screenshots
    // swallow it entirely.
    final attach = message.hasAttachments
        ? Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Icon(
              Icons.attach_file,
              size: 14,
              color: theme.colorScheme.outline,
            ),
          )
        : const SizedBox.shrink();
    return GestureDetector(
      // Desktop parity: right-click opens the same row menu as ⋮.
      onSecondaryTapDown: (d) => _showContextMenu(context, d.globalPosition),
      child: Material(
        color: (selected || checked)
            ? theme.colorScheme.secondaryContainer
            : Colors.transparent,
        child: InkWell(
          onTap: onTap,
          child: Padding(
            // Tight all round so the row uses the pane edge to edge: the
            // sender name and the subject get every pixel left over.
            padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 4),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: 30,
                  // Avatar at the top of the row, not centred: the title
                  // lines up with it and the paperclip sits underneath.
                  // The clip stays in selection mode too, below the box.
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (selectionMode)
                        Checkbox(value: checked, onChanged: (_) => onToggle())
                      else
                        // Unread marker as a badge on the avatar's
                        // corner, like the Qt row — bold text alone is
                        // too easy to miss, and a column of its own
                        // would cost the row its left edge.
                        Badge(
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
                      attach,
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
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
                                  Icon(
                                    Icons.star,
                                    size: 14,
                                    color: Colors.amber.shade700,
                                  ),
                                ],
                              ],
                            ),
                          ),
                          // Always top right, kept small so the name keeps
                          // its room.
                          const SizedBox(width: 4),
                          Text(
                            message.date,
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.outline,
                            ),
                          ),
                        ],
                      ),
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              message.subject,
                              overflow: TextOverflow.ellipsis,
                              style: theme.textTheme.bodyMedium?.copyWith(
                                fontWeight: weight,
                              ),
                            ),
                          ),
                          // One line below the date, slimmed to its icon at
                          // the same edge; the whole row stays tappable, so
                          // the smaller hit area costs nothing.
                          PopupMenuButton<String>(
                            tooltip: 'Message actions',
                            padding: const EdgeInsets.all(4),
                            icon: const Icon(Icons.more_vert, size: 18),
                            onSelected: (v) => _run(context, v),
                            itemBuilder: (context) => _items(),
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
              ],
            ),
          ),
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

/// One search hit, drawn as a folder row. Tapping opens it; the ⋮ /
/// right-click menu adds "Open message" to the folder row's actions, aimed
/// at the folder the hit lives in.
class SearchHitTile extends StatelessWidget {
  const SearchHitTile({
    super.key,
    required this.hit,
    required this.selected,
    required this.checked,
    required this.selectionMode,
    required this.compact,
    this.onOpened,
  });

  final SearchHit hit;
  final bool selected;
  final bool checked;
  final bool selectionMode;
  final bool compact;
  final VoidCallback? onOpened;

  @override
  Widget build(BuildContext context) {
    void open() {
      FocusManager.instance.primaryFocus?.unfocus();
      context.read<MailState>().jumpToHit(hit);
      onOpened?.call();
    }

    void toggle() => context.read<MailState>().toggleSelectHit(hit);

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

    return MessageTile(
      message: hit.summary,
      selected: selected,
      checked: checked,
      selectionMode: selectionMode,
      compact: compact,
      onTap: selectionMode ? toggle : open,
      onToggle: toggle,
      menuItems: () => [
        const PopupMenuItem(
          value: 'open',
          child: MenuRow(icon: Icons.open_in_new, text: 'Open message'),
        ),
        const PopupMenuDivider(),
        ...messageActionItems(unread: hit.unread, starred: hit.starred),
      ],
      onMenu: act,
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

/// Account-wide hits grouped by folder: folders in the order of their newest
/// hit, newest first inside each. Strings are the folder headers.
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
    // Filters only narrow the loaded rows (Qt says the same).
    final filtered = context.select<MailState, bool>(
      (s) => s.hasListFilter || s.searchQuery.trim().isNotEmpty,
    );
    final count = server < 0
        ? 'Cached $cached (server not checked)'
        : cached >= server
        ? null
        : 'Cached $cached of $server';
    final label = count == null
        ? 'All $cached loaded'
        : filtered
        ? '$count · filters cover loaded mail only'
        : count;
    final canLoad = canAsk && (server < 0 || server > cached);
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
