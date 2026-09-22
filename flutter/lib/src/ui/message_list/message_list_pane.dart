import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';

/// The message list for the selected folder.
///
/// Rows come from the compact feed — subject, sender, snippet and flags, no
/// body — so opening a folder of two hundred mails costs no sanitizing work.
/// Selecting a row is what asks for the body.
class MessageListPane extends StatelessWidget {
  const MessageListPane({super.key, this.onMessageOpened});

  /// Lets a narrow layout navigate to the reader. Null in three-pane.
  final VoidCallback? onMessageOpened;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final messages = state.messages;

    if (state.folderId < 0) {
      return const _Empty(
        icon: Icons.folder_open_outlined,
        text: 'Pick a folder',
      );
    }
    if (messages.isEmpty) {
      return _Empty(
        icon: Icons.mail_outline,
        text: state.isSyncing ? 'Syncing…' : 'Nothing here',
      );
    }

    return Column(
      children: [
        Expanded(
          child: ListView.separated(
            itemCount: messages.length + 1,
            separatorBuilder: (_, _) => const Divider(height: 1),
            itemBuilder: (context, i) {
              // The tail row asks the server for the next older batch. It is
              // a button rather than an infinite scroll on purpose: each press
              // is a deliberate, sizeable download.
              if (i == messages.length) return const _LoadOlderTile();
              final m = messages[i];
              return _MessageTile(
                message: m,
                selected: m.uid == state.openUid,
                onTap: () {
                  state.openMessage(m.uid);
                  onMessageOpened?.call();
                },
              );
            },
          ),
        ),
      ],
    );
  }
}

class _MessageTile extends StatelessWidget {
  const _MessageTile({
    required this.message,
    required this.selected,
    required this.onTap,
  });

  final MessageSummary message;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final state = context.read<MailState>();
    final weight = message.unread ? FontWeight.w700 : FontWeight.normal;
    return ListTile(
      selected: selected,
      selectedTileColor: theme.colorScheme.secondaryContainer,
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
          if (message.snippet.isNotEmpty)
            Text(
              message.snippet,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
        ],
      ),
      trailing: IconButton(
        tooltip: message.starred ? 'Unstar' : 'Star',
        icon: Icon(
          message.starred ? Icons.star : Icons.star_border,
          size: 18,
          color: message.starred ? Colors.amber.shade700 : null,
        ),
        onPressed: () => state.toggleStar(message.uid),
      ),
    );
  }
}

class _LoadOlderTile extends StatelessWidget {
  const _LoadOlderTile();

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Center(
        child: TextButton.icon(
          icon: const Icon(Icons.history, size: 18),
          label: const Text('Show older messages'),
          onPressed: state.isSyncing ? null : state.loadOlderMessages,
        ),
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
