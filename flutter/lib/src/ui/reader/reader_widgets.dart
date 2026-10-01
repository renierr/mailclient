import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import '../dialogs/mail_dialog.dart';
import '../message_list/message_list_pane.dart' show confirmDelete;
import '../move_to/move_to_dialog.dart';

Future<void> moreActions(
  BuildContext context,
  MessageBody message,
  String action,
) async {
  final state = context.read<MailState>();
  switch (action) {
    case 'forward':
      if (context.mounted) {
        await ComposerDialog.showForward(context, message);
      }
    case 'reply-all':
      if (context.mounted) {
        await ComposerDialog.showReply(context, message, replyAll: true);
      }
    case 'archive':
      await state.archiveMessages([message.uid]);
    case 'move':
      if (context.mounted) {
        await MoveToDialog.show(
          context,
          uids: [message.uid],
          subject: message.subject,
        );
      }
    case 'purge':
      if (context.mounted) {
        await confirmDelete(
          context,
          state,
          uids: [message.uid],
          permanent: true,
          purge: true,
        );
      }
    case 'headers':
      if (context.mounted) await showHeaders(context, message);
  }
}

Future<void> showHeaders(BuildContext context, MessageBody message) async {
  final state = context.read<MailState>();
  late MessageHeaders headers;
  try {
    headers = await MailCore.instance.messageHeaders(
      state.folderId,
      message.uid,
    );
  } catch (e) {
    if (!context.mounted) return;
    state.showStatus(coreErrorText(e), isError: true);
    return;
  }
  if (!context.mounted) return;
  await MailDialog.show(
    context,
    builder: (context) => AlertDialog(
      title: const Text('Headers'),
      content: SizedBox(
        width: MailDialog.maxWidth(context, 480),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final row in {
                'From': headers.from,
                'To': headers.to,
                'Cc': headers.cc,
                'Date': headers.date,
                'Subject': headers.subject,
                'Message-ID': headers.messageId,
                'Reply-To': headers.replyTo,
              }.entries)
                if (row.value.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 6),
                    child: SelectableText('${row.key}: ${row.value}'),
                  ),
              if (headers.raw.isNotEmpty)
                ExpansionTile(
                  title: const Text('Complete headers'),
                  tilePadding: EdgeInsets.zero,
                  children: [
                    SelectableText(
                      headers.raw,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                      ),
                    ),
                  ],
                )
              else
                const Text(
                  'Complete headers are unavailable until this message is downloaded again.',
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    ),
  );
}

class ReaderPlaceholder extends StatelessWidget {
  const ReaderPlaceholder({super.key, required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Text(text, style: TextStyle(color: scheme.outline)),
    );
  }
}

/// One row of the expanded address block: fixed-width label, selectable
/// wrapping value. Selectable so a long address can be copied out.
class HeaderDetailRow extends StatelessWidget {
  const HeaderDetailRow({super.key, required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 64,
            child: Text(
              label,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.outline,
              ),
            ),
          ),
          Expanded(
            child: SelectableText(value, style: theme.textTheme.bodySmall),
          ),
        ],
      ),
    );
  }
}
