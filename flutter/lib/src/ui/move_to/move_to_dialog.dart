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
    this.forSelection = false,
  });

  /// Move the state's checkbox set (folder or search results) instead of
  /// [uids].
  final bool forSelection;

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

  static Future<void> showForSelection(BuildContext context) async {
    await MailDialog.show(
      context,
      builder: (_) => const MoveToDialog(uids: [], forSelection: true),
    );
  }

  @override
  Widget build(BuildContext context) {
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.visibleFolders,
    );
    final shownId = context.select<MailState, int>((s) => s.folderId);
    final selectionCount = context.select<MailState, int>(
      (s) => s.selectedCount,
    );
    final searching = context.select<MailState, bool>((s) => s.searching);
    // Search selections can span folders: none is "here" for all of them
    // (the core answers "Already here" per folder where it applies).
    final currentId = forSelection && searching ? -1 : folderId ?? shownId;
    final n = forSelection ? selectionCount : uids.length;
    final title = n > 1
        ? 'Move $n messages to:'
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
                        final state = context.read<MailState>();
                        if (forSelection) {
                          state.bulkMove(f.path);
                        } else {
                          state.moveMessages(uids, f.path, folderId: folderId);
                        }
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
