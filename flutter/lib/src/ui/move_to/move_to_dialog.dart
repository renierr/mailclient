import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';

/// Move one message (or a selection) into another folder of the same account.
///
/// Subscribed folders only, current folder dimmed out — moving mail where it
/// already is just reports "Already here".
class MoveToDialog extends StatelessWidget {
  const MoveToDialog({super.key, required this.uids, this.subject});

  final List<int> uids;

  /// Shown for a single message, so the dialog names what it moves.
  final String? subject;

  static Future<void> show(BuildContext context,
      {required List<int> uids, String? subject}) async {
    await showDialog(
      context: context,
      builder: (_) => MoveToDialog(uids: uids, subject: subject),
    );
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final folders = state.visibleFolders;
    final currentId = state.folderId;
    final title = uids.length > 1
        ? 'Move ${uids.length} messages to:'
        : subject != null && subject!.isNotEmpty
            ? 'Move “$subject” to:'
            : 'Move to:';
    return Dialog(
      insetPadding:
          const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 440, maxHeight: 520),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(title,
                  style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 12),
              Flexible(
                child: ListView.builder(
                  shrinkWrap: true,
                  itemCount: folders.length,
                  itemBuilder: (context, i) {
                    final f = folders[i];
                    final current = f.id == currentId;
                    return ListTile(
                      enabled: !current,
                      contentPadding: EdgeInsets.only(
                          left: 12.0 + f.depth * 14, right: 8),
                      leading: Icon(_iconFor(f.role), size: 20),
                      title: Text(f.leafName,
                          overflow: TextOverflow.ellipsis),
                      onTap: current
                          ? null
                          : () {
                              Navigator.of(context).pop();
                              state.moveMessages(uids, f.path);
                            },
                    );
                  },
                ),
              ),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: const Text('Cancel'),
                ),
              ),
            ],
          ),
        ),
      ),
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
