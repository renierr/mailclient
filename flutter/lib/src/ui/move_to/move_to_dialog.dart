import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// Move one message (or a selection) into another folder of the same account.
///
/// Subscribed folders only, current folder dimmed out — moving mail where it
/// already is just reports "Already here".
class MoveToDialog extends StatelessWidget {
  const MoveToDialog({
    super.key,
    required this.uids,
    this.subject,
    this.folderId,
  });

  final List<int> uids;

  /// The folder the messages live in, when not the shown one (search hits).
  final int? folderId;

  /// Shown for a single message, so the dialog names what it moves.
  final String? subject;

  static Future<void> show(
    BuildContext context, {
    required List<int> uids,
    String? subject,
    int? folderId,
  }) async {
    await MailDialog.show(
      context,
      builder: (_) =>
          MoveToDialog(uids: uids, subject: subject, folderId: folderId),
    );
  }

  @override
  Widget build(BuildContext context) {
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.visibleFolders,
    );
    final shownId = context.select<MailState, int>((s) => s.folderId);
    final currentId = folderId ?? shownId;
    final title = uids.length > 1
        ? 'Move ${uids.length} messages to:'
        : subject != null && subject!.isNotEmpty
        ? 'Move “$subject” to:'
        : 'Move to:';
    final list = folders.isEmpty
        ? const Padding(
            padding: EdgeInsets.symmetric(vertical: 16),
            child: Text('No other folders available.'),
          )
        : ListView.builder(
            shrinkWrap: true,
            itemCount: folders.length,
            itemBuilder: (context, i) {
              final f = folders[i];
              final current = f.id == currentId;
              return ListTile(
                enabled: !current,
                contentPadding: EdgeInsets.only(
                  left: 12.0 + f.depth * 14,
                  right: 8,
                ),
                leading: Icon(folderIcon(f.role), size: 20),
                title: Text(f.leafName, overflow: TextOverflow.ellipsis),
                onTap: current
                    ? null
                    : () {
                        Navigator.of(context).pop();
                        context.read<MailState>().moveMessages(
                          uids,
                          f.path,
                          folderId: folderId,
                        );
                      },
              );
            },
          );
    return MailDialogShell(
      title: title,
      maxWidth: 440,
      maxHeight: 520,
      scrollBody: false,
      body: list,
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
      ],
    );
  }
}
