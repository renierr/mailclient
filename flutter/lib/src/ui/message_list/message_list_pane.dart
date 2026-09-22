import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import '../move_to/move_to_dialog.dart';

/// The message list for the selected folder — or account-wide search hits.
///
/// Rows come from the compact feed — subject, sender, snippet and flags, no
/// body — so opening a folder of two hundred mails costs no sanitizing work.
/// Selecting a row is what asks for the body.
class MessageListPane extends StatefulWidget {
  const MessageListPane({super.key, this.onMessageOpened});

  /// Lets a narrow layout navigate to the reader. Null in three-pane.
  final VoidCallback? onMessageOpened;

  @override
  State<MessageListPane> createState() => _MessageListPaneState();
}

class _MessageListPaneState extends State<MessageListPane> {
  int? _anchorUid;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    if (state.searching) return _searchResults(state);
    return _folderList(state);
  }

  // --- folder feed -------------------------------------------------------

  Widget _folderList(MailState state) {
    final messages = state.messages;
    if (state.folderId < 0) {
      return const _Empty(
        icon: Icons.folder_open_outlined,
        text: 'Pick a folder',
      );
    }
    return Column(
      children: [
        _ListHeader(anchorUid: _anchorUid),
        if (state.selectionMode && state.selectedCount > 0)
          _BulkBar(onAction: () => setState(() => _anchorUid = null)),
        Expanded(
          child: messages.isEmpty
              ? _Empty(
                  icon: Icons.mail_outline,
                  text: state.isSyncing ? 'Syncing…' : 'Nothing here',
                )
              : ListView.separated(
                  itemCount: messages.length + 1,
                  separatorBuilder: (_, _) => const Divider(height: 1),
                  itemBuilder: (context, i) {
                    // The tail row asks the server for the next older batch.
                    // It is a button rather than an infinite scroll on
                    // purpose: each press is a deliberate, sizeable download.
                    if (i == messages.length) return const _LoadOlderTile();
                    final m = messages[i];
                    return _MessageTile(
                      message: m,
                      selected: m.uid == state.openUid,
                      checked:
                          state.selectedUids.contains(m.uid),
                      selectionMode: state.selectionMode,
                      compact: state.settings.isCompact,
                      onTap: () => _onRowTap(state, m),
                      onToggle: () => _onRowToggle(state, m),
                    );
                  },
                ),
        ),
      ],
    );
  }

  void _onRowTap(MailState state, MessageSummary m) {
    if (state.selectionMode) {
      _onRowToggle(state, m);
      return;
    }
    setState(() => _anchorUid = m.uid);
    if (state.isDraftsFolder) {
      // Drafts open in the composer, never in the reader.
      ComposerDialog.showDraft(context, state.accountId, m.uid);
      return;
    }
    state.openMessage(m.uid);
    widget.onMessageOpened?.call();
  }

  void _onRowToggle(MailState state, MessageSummary m) {
    if (_shiftHeld && _anchorUid != null && _anchorUid != m.uid) {
      state.selectRange(_anchorUid!, m.uid);
    } else {
      state.toggleSelect(m.uid);
    }
    setState(() => _anchorUid = m.uid);
  }

  bool get _shiftHeld {
    final keys = HardwareKeyboard.instance.logicalKeysPressed;
    return keys.contains(LogicalKeyboardKey.shiftLeft) ||
        keys.contains(LogicalKeyboardKey.shiftRight);
  }

  // --- search results ----------------------------------------------------

  Widget _searchResults(MailState state) {
    final hits = state.searchHits;
    return Column(
      children: [
        Container(
          width: double.infinity,
          padding:
              const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Text(
            '${hits.length} result(s) across ${state.searchFolderOnly ? 'this folder' : 'this account'}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: hits.isEmpty
              ? _Empty(
                  icon: Icons.search_off_outlined,
                  text: state.isBusy
                      ? 'Searching the server…'
                      : 'No matches for “${state.searchQuery}”',
                )
              : ListView.separated(
                  itemCount: hits.length,
                  separatorBuilder: (_, _) => const Divider(height: 1),
                  itemBuilder: (context, i) {
                    final h = hits[i];
                    // Search rows navigate only: mutating a row that lives in
                    // another folder from here would act on the wrong mailbox.
                    return ListTile(
                      title: Text(h.subject,
                          overflow: TextOverflow.ellipsis),
                      subtitle: Text(
                        '${h.from} · ${h.folder}\n${h.snippet}',
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      isThreeLine: true,
                      trailing: h.unread
                          ? Container(
                              width: 8,
                              height: 8,
                              decoration: BoxDecoration(
                                shape: BoxShape.circle,
                                color: Theme.of(context)
                                    .colorScheme
                                    .primary,
                              ),
                            )
                          : null,
                      onTap: () {
                        state.jumpToHit(h);
                        widget.onMessageOpened?.call();
                      },
                    );
                  },
                ),
        ),
      ],
    );
  }
}

/// Title, count, sort and select menus above the list.
class _ListHeader extends StatelessWidget {
  const _ListHeader({required this.anchorUid});

  final int? anchorUid;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final title = state.folder?.leafName ?? 'Messages';
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: Row(
        children: [
          IconButton(
            tooltip: state.selectionMode
                ? 'Leave selection'
                : 'Select messages',
            icon: Icon(state.selectionMode
                ? Icons.check_box_outlined
                : Icons.check_box_outline_blank),
            onPressed: () => state.selectionMode
                ? state.exitSelectionMode()
                : state.enterSelectionMode(),
          ),
          Expanded(
            child: Text('$title · ${state.messages.length}',
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleSmall),
          ),
          if (state.selectionMode)
            PopupMenuButton<String>(
              tooltip: 'Select',
              icon: const Icon(Icons.arrow_drop_down),
              onSelected: (v) => switch (v) {
                'all' => state.selectAllVisible(),
                'none' => state.exitSelectionMode(),
                'unread' => state.selectUnread(),
                'starred' => state.selectStarred(),
                'invert' => state.invertSelection(),
                _ => null,
              },
              itemBuilder: (context) => const [
                PopupMenuItem(
                    value: 'all', child: Text('Select all visible')),
                PopupMenuItem(
                    value: 'unread', child: Text('Select unread')),
                PopupMenuItem(
                    value: 'starred', child: Text('Select starred')),
                PopupMenuItem(
                    value: 'invert', child: Text('Invert selection')),
                PopupMenuItem(value: 'none', child: Text('Clear')),
              ],
            ),
          PopupMenuButton<String>(
            tooltip: 'Sort',
            icon: const Icon(Icons.sort),
            onSelected: (v) {
              final parts = v.split(':');
              state.setSort(parts[0], parts[1] == 'desc');
            },
            itemBuilder: (context) {
              final s = state.settings;
              String tick(String f, bool d) =>
                  (s.sortField == f && s.sortDescending == d)
                      ? '✓ '
                      : '　';
              return [
                PopupMenuItem(
                    value: 'date:desc',
                    child: Text('${tick('date', true)}Date, newest first')),
                PopupMenuItem(
                    value: 'date:asc',
                    child: Text('${tick('date', false)}Date, oldest first')),
                PopupMenuItem(
                    value: 'from:asc',
                    child: Text('${tick('from', false)}From A–Z')),
                PopupMenuItem(
                    value: 'from:desc',
                    child: Text('${tick('from', true)}From Z–A')),
                PopupMenuItem(
                    value: 'subject:asc',
                    child: Text('${tick('subject', false)}Subject A–Z')),
                PopupMenuItem(
                    value: 'subject:desc',
                    child: Text('${tick('subject', true)}Subject Z–A')),
              ];
            },
          ),
        ],
      ),
    );
  }
}

/// The selected message(s), and what to do with them.
class _BulkBar extends StatelessWidget {
  const _BulkBar({required this.onAction});

  final VoidCallback onAction;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final uids = state.selectedUids.toList(growable: false);
    final starred =
        uids.isNotEmpty && uids.every((u) => _isStarred(state, u));
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
              onPressed: state.exitSelectionMode,
            ),
            Text('${uids.length} selected',
                style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(width: 8),
            IconButton(
              tooltip: 'Mark read',
              icon: const Icon(Icons.mark_email_read_outlined, size: 18),
              onPressed: () => state.markReadMany(uids, true),
            ),
            IconButton(
              tooltip: 'Mark unread',
              icon: const Icon(Icons.mark_email_unread_outlined, size: 18),
              onPressed: () => state.markReadMany(uids, false),
            ),
            IconButton(
              tooltip: starred ? 'Unstar' : 'Star',
              icon: Icon(starred ? Icons.star : Icons.star_border,
                  size: 18),
              onPressed: () => state.setStarMany(uids, !starred),
            ),
            IconButton(
              tooltip: 'Archive',
              icon: const Icon(Icons.archive_outlined, size: 18),
              onPressed: () {
                state.archiveMessages(uids);
                onAction();
              },
            ),
            IconButton(
              tooltip: 'Move to…',
              icon:
                  const Icon(Icons.drive_file_move_outlined, size: 18),
              onPressed: () => MoveToDialog.show(context, uids: uids),
            ),
            IconButton(
              tooltip: 'Move to Trash',
              icon: const Icon(Icons.delete_outline, size: 18),
              onPressed: () => confirmDelete(context, state,
                  uids: uids, permanent: state.deleteIsPermanent),
            ),
            PopupMenuButton<String>(
              icon: const Icon(Icons.more_vert, size: 18),
              onSelected: (v) {
                switch (v) {
                  case 'purge':
                    confirmDelete(context, state,
                        uids: uids, permanent: true, purge: true);
                  case 'unread':
                    state.selectUnread();
                  case 'starred':
                    state.selectStarred();
                }
              },
              itemBuilder: (context) => const [
                PopupMenuItem(
                    value: 'purge',
                    child: Text('Delete permanently…')),
                PopupMenuItem(
                    value: 'unread', child: Text('Select unread')),
                PopupMenuItem(
                    value: 'starred', child: Text('Select starred')),
              ],
            ),
          ],
        ),
      ),
    );
  }

  static bool _isStarred(MailState state, int uid) =>
      state.messages
          .where((m) => m.uid == uid)
          .firstOrNull
          ?.starred ??
      false;
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
  final title =
      purge || permanent ? 'Delete permanently?' : 'Move to Trash?';
  final what = uids.length > 1
      ? '${uids.length} messages'
      : '“${_subjectOf(state, uids.first)}”';
  final how = purge || permanent
      ? 'will be destroyed on the server. This cannot be undone.'
      : 'will be moved to Trash.';
  final confirmed = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text('$what $how'),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: Text(purge || permanent
                  ? 'Delete permanently'
                  : 'Move to Trash'),
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

String _subjectOf(MailState state, int uid) => state.messages
        .where((m) => m.uid == uid)
        .firstOrNull
        ?.subject ??
    '';

class _MessageTile extends StatelessWidget {
  const _MessageTile({
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
    final state = context.read<MailState>();
    final weight = message.unread ? FontWeight.w700 : FontWeight.normal;
    return ListTile(
      selected: selected,
      selectedTileColor: theme.colorScheme.secondaryContainer,
      leading: selectionMode
          ? Checkbox(value: checked, onChanged: (_) => onToggle())
          : message.unread
              ? Container(
                  width: 8,
                  height: 8,
                  margin: const EdgeInsets.only(top: 14),
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: theme.colorScheme.primary,
                  ),
                )
              : const SizedBox(width: 8),
      onTap: onTap,
      title: Row(
        children: [
          Expanded(
            child: Text(
              message.from,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodyMedium?.copyWith(fontWeight: weight),
            ),
          ),
          const SizedBox(width: 8),
          Text(
            message.date,
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.outline),
          ),
        ],
      ),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              if (message.hasAttachments) ...[
                Icon(Icons.attach_file,
                    size: 14, color: theme.colorScheme.outline),
                const SizedBox(width: 4),
              ],
              Expanded(
                child: Text(
                  message.subject,
                  overflow: TextOverflow.ellipsis,
                  style:
                      theme.textTheme.bodyMedium?.copyWith(fontWeight: weight),
                ),
              ),
            ],
          ),
          if (!compact && message.snippet.isNotEmpty)
            Text(
              message.snippet,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
        ],
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: message.starred ? 'Unstar' : 'Star',
            icon: Icon(
              message.starred ? Icons.star : Icons.star_border,
              size: 18,
              color: message.starred ? Colors.amber.shade700 : null,
            ),
            onPressed: () => state.toggleStar(message.uid),
          ),
          PopupMenuButton<String>(
            icon: const Icon(Icons.more_vert, size: 18),
            onSelected: (v) => _rowAction(context, state, v),
            itemBuilder: (context) => [
              PopupMenuItem(
                  value: 'read',
                  child: Text(
                      message.unread ? 'Mark as read' : 'Mark as unread')),
              PopupMenuItem(
                  value: 'star',
                  child: Text(
                      message.starred ? 'Remove star' : 'Star')),
              const PopupMenuItem(value: 'archive', child: Text('Archive')),
              const PopupMenuItem(value: 'move', child: Text('Move to…')),
              const PopupMenuItem(
                  value: 'delete', child: Text('Move to Trash')),
              const PopupMenuItem(
                  value: 'purge', child: Text('Delete permanently…')),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _rowAction(
      BuildContext context, MailState state, String v) async {
    final uid = message.uid;
    switch (v) {
      case 'read':
        await state.setRead(uid, message.unread);
      case 'star':
        await state.toggleStar(uid);
      case 'archive':
        await state.archiveMessages([uid]);
      case 'move':
        if (context.mounted) {
          await MoveToDialog.show(context,
              uids: [uid], subject: message.subject);
        }
      case 'delete':
        if (context.mounted) {
          await confirmDelete(context, state,
              uids: [uid], permanent: state.deleteIsPermanent);
        }
      case 'purge':
        if (context.mounted) {
          await confirmDelete(context, state,
              uids: [uid], permanent: true, purge: true);
        }
    }
  }
}

class _LoadOlderTile extends StatelessWidget {
  const _LoadOlderTile();

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final cached = state.cachedCount;
    final server = state.serverTotal;
    final label = server < 0
        ? 'Cached $cached (server not checked)'
        : cached >= server
            ? 'All $cached loaded'
            : 'Cached $cached of $server';
    final canLoad = state.folderId >= 0 && (server < 0 || server > cached);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        children: [
          Text(label, style: Theme.of(context).textTheme.bodySmall),
          const SizedBox(height: 4),
          TextButton.icon(
            icon: const Icon(Icons.history, size: 18),
            label: const Text('Show older messages'),
            onPressed:
                state.isSyncing || !canLoad ? null : state.loadOlderMessages,
          ),
        ],
      ),
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 40, color: scheme.outlineVariant),
          const SizedBox(height: 8),
          Text(text, style: TextStyle(color: scheme.outline)),
        ],
      ),
    );
  }
}
