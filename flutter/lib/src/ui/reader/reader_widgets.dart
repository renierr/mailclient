import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:open_filex/open_filex.dart';
import 'package:path_provider/path_provider.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import '../dialogs/mail_dialog.dart';
import '../menu_row.dart';
import '../message_list/message_list_pane.dart' show confirmDelete;
import '../move_to/move_to_dialog.dart';

/// Subject, sender and the reply/star/delete row.
class ReaderHeader extends StatelessWidget {
  const ReaderHeader({
    super.key,
    required this.message,
    required this.headersFuture,
    required this.details,
    required this.onToggleDetails,
    this.onClose,
  });

  final MessageBody message;
  final Future<MessageHeaders?>? headersFuture;
  final bool details;
  final VoidCallback onToggleDetails;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final fullscreen = context.select<MailState, bool>(
      (s) => s.readerFullscreen,
    );
    final starred = context.select<MailState, bool>(
      (s) =>
          s.messages.where((m) => m.uid == message.uid).firstOrNull?.starred ??
          false,
    );
    final permanent = context.select<MailState, bool>(
      (s) => s.deleteIsPermanent,
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Wrap, not Row: five icon buttons plus a long subject overflow a
          // 360px phone. The subject takes a full line; actions wrap under it.
          Text(
            message.subject,
            style: theme.textTheme.titleMedium,
            maxLines: 3,
            overflow: TextOverflow.ellipsis,
          ),
          Wrap(
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              if (onClose != null)
                IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: onClose,
                ),
              IconButton(
                tooltip: 'Reply',
                icon: const Icon(Icons.reply_outlined),
                onPressed: () => ComposerDialog.showReply(context, message),
              ),
              IconButton(
                tooltip: starred ? 'Unstar' : 'Star',
                icon: Icon(
                  starred ? Icons.star : Icons.star_border,
                  color: starred ? Colors.amber.shade700 : null,
                ),
                onPressed: () =>
                    context.read<MailState>().toggleStar(message.uid),
              ),
              IconButton(
                tooltip: 'Delete',
                icon: const Icon(Icons.delete_outline),
                onPressed: () => confirmDelete(
                  context,
                  context.read<MailState>(),
                  uids: [message.uid],
                  permanent: permanent,
                ),
              ),
              // Wide layouts only: narrower ones already give the reader
              // every pixel they have.
              if (onClose == null)
                IconButton(
                  tooltip: fullscreen ? 'Exit fullscreen' : 'Fullscreen',
                  icon: Icon(
                    fullscreen ? Icons.close_fullscreen : Icons.open_in_full,
                  ),
                  onPressed: () =>
                      context.read<MailState>().toggleReaderFullscreen(),
                ),
              PopupMenuButton<String>(
                icon: const Icon(Icons.more_vert),
                onSelected: (v) => moreActions(context, message, v),
                itemBuilder: (context) => const [
                  PopupMenuItem(
                    value: 'forward',
                    child: MenuRow(
                      icon: Icons.forward_outlined,
                      text: 'Forward',
                    ),
                  ),
                  PopupMenuItem(
                    value: 'reply-all',
                    child: MenuRow(
                      icon: Icons.reply_all_outlined,
                      text: 'Reply all',
                    ),
                  ),
                  PopupMenuItem(
                    value: 'archive',
                    child: MenuRow(
                      icon: Icons.archive_outlined,
                      text: 'Archive',
                    ),
                  ),
                  PopupMenuItem(
                    value: 'move',
                    child: MenuRow(
                      icon: Icons.drive_file_move_outlined,
                      text: 'Move to…',
                    ),
                  ),
                  PopupMenuItem(
                    value: 'purge',
                    child: MenuRow(
                      icon: Icons.delete_forever_outlined,
                      text: 'Delete permanently…',
                    ),
                  ),
                  PopupMenuItem(
                    value: 'headers',
                    child: MenuRow(
                      icon: Icons.info_outline,
                      text: 'Show headers…',
                    ),
                  ),
                ],
              ),
            ],
          ),
          const SizedBox(height: 4),
          InkWell(
            onTap: onToggleDetails,
            child: Row(
              children: [
                SenderAvatar(from: message.from),
                const SizedBox(width: 10),
                Expanded(
                  child: FutureBuilder<MessageHeaders?>(
                    future: headersFuture,
                    builder: (context, snap) {
                      // The list feed only carries the bare address; the full
                      // From header has the display name, like the Qt reader.
                      final from = (snap.data?.from.isNotEmpty ?? false)
                          ? snap.data!.from
                          : message.from;
                      final shown = splitAddr(from);
                      return Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            shown.name.isNotEmpty ? shown.name : shown.addr,
                            style: theme.textTheme.bodyMedium?.copyWith(
                              fontWeight: FontWeight.w600,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                          if (shown.name.isNotEmpty && shown.addr != shown.name)
                            Text(
                              shown.addr,
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: theme.colorScheme.outline,
                              ),
                              overflow: TextOverflow.ellipsis,
                            ),
                        ],
                      );
                    },
                  ),
                ),
                Column(
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: [
                    Text(message.date, style: theme.textTheme.bodySmall),
                    Icon(details ? Icons.expand_less : Icons.expand_more),
                  ],
                ),
              ],
            ),
          ),
          if (!details && message.to.isNotEmpty)
            Text(
              'To: ${message.to}',
              style: theme.textTheme.bodySmall,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          // The expander opens the full address block, like the Qt details
          // grid — From/To/Cc/Date/Reply-To. It used to reveal only the Cc
          // line, so on mail without Cc the tap visibly did nothing.
          if (details)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: FutureBuilder<MessageHeaders?>(
                future: headersFuture,
                builder: (context, snap) {
                  final h = snap.data;
                  final from = (h?.from.isNotEmpty ?? false)
                      ? h!.from
                      : message.from;
                  final to = (h?.to.isNotEmpty ?? false) ? h!.to : message.to;
                  final cc = (h?.cc.isNotEmpty ?? false) ? h!.cc : message.cc;
                  final date = (h?.date.isNotEmpty ?? false)
                      ? h!.date
                      : message.date;
                  final replyTo = (h?.replyTo.isNotEmpty ?? false)
                      ? h!.replyTo
                      : message.replyTo;
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      HeaderDetailRow(label: 'From', value: from),
                      if (to.isNotEmpty)
                        HeaderDetailRow(label: 'To', value: to),
                      if (cc.isNotEmpty)
                        HeaderDetailRow(label: 'Cc', value: cc),
                      if (date.isNotEmpty)
                        HeaderDetailRow(label: 'Date', value: date),
                      if (replyTo.isNotEmpty)
                        HeaderDetailRow(label: 'Reply-To', value: replyTo),
                    ],
                  );
                },
              ),
            ),
          // Shown inline, not hidden in a details view: replying to the wrong
          // address is not something the user can take back. Red like Qt —
          // this is a warning, not information.
          if (message.replyTo.isNotEmpty)
            Container(
              margin: const EdgeInsets.only(top: 6),
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                color: theme.colorScheme.errorContainer,
                borderRadius: BorderRadius.circular(6),
              ),
              child: Text(
                'Replies go to: ${message.replyTo}',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.onErrorContainer,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

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
    state.showStatus('$e', isError: true);
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

class RemoteImagesBanner extends StatelessWidget {
  const RemoteImagesBanner({super.key, required this.onShowOnce});

  final VoidCallback onShowOnce;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      color: scheme.surfaceContainerHighest,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Wrap(
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: 8,
        runSpacing: 4,
        children: [
          Icon(
            Icons.image_not_supported_outlined,
            size: 18,
            color: scheme.outline,
          ),
          ConstrainedBox(
            constraints: BoxConstraints(
              maxWidth: MediaQuery.sizeOf(context).width - 48,
            ),
            child: const Text(
              'Remote images were blocked. Loading them tells the sender you '
              'opened this message.',
            ),
          ),
          TextButton(onPressed: onShowOnce, child: const Text('Show once')),
        ],
      ),
    );
  }
}

/// `"Name <addr>"` split apart; a bare address yields both identical.
({String name, String addr}) splitAddr(String full) {
  final s = full.trim();
  final lt = s.indexOf('<');
  final gt = s.lastIndexOf('>');
  if (lt >= 0 && gt > lt) {
    var name = s
        .substring(0, lt)
        .trim()
        .replaceAll(RegExp('^["\']|["\']\$'), '');
    final addr = s.substring(lt + 1, gt).trim();
    if (name.isEmpty) name = addr;
    return (name: name, addr: addr);
  }
  return (name: s, addr: s);
}

/// Attachment files with working Open / Save / Save-all.
///
/// Bytes stay in SQLite until the user acts: Open stages through the temp
/// directory into the system viewer (`open_filex`), Save asks where
/// (`file_picker`). A missing download is fetched first, on demand.
class AttachmentBar extends StatelessWidget {
  const AttachmentBar({super.key, required this.message});

  final MessageBody message;

  @override
  Widget build(BuildContext context) {
    final files = message.attachments.where((a) => !a.isInline).toList();
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: Theme.of(context).dividerColor)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  '${files.length} attachment(s)',
                  style: Theme.of(context).textTheme.bodySmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (files.length > 1)
                TextButton.icon(
                  icon: const Icon(Icons.save_alt, size: 16),
                  label: const Text('Save all…'),
                  onPressed: () => saveAll(context, message),
                ),
            ],
          ),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final a in files)
                InputChip(
                  avatar: const Icon(Icons.attach_file, size: 16),
                  label: Text('${a.filename}  (${formatBytes(a.size)})'),
                  onPressed: () => openAttachment(context, message, a),
                  onDeleted: () => saveAttachment(context, message, a),
                  deleteButtonTooltipMessage: 'Save as…',
                  deleteIcon: const Icon(Icons.save_alt, size: 16),
                ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Cached bytes, downloading first when the message arrived without them.
Future<List<int>?> attachmentBytes(
  MailState state,
  MessageBody message,
  int attachmentId,
) async {
  var bytes = await MailCore.instance.attachmentBytes(attachmentId);
  if (bytes != null) return bytes;
  await MailCore.instance.downloadAttachments(
    state.accountId,
    state.folderId,
    message.uid,
  );
  return MailCore.instance.attachmentBytes(attachmentId);
}

Future<void> openAttachment(
  BuildContext context,
  MessageBody message,
  AttachmentInfo attachment,
) async {
  final state = context.read<MailState>();
  try {
    state.showStatus('Opening ${attachment.filename}…');
    final bytes = await attachmentBytes(state, message, attachment.id);
    if (bytes == null) {
      state.showStatus(
        '${attachment.filename} is not downloaded yet',
        isError: true,
      );
      return;
    }
    final dir = await getTemporaryDirectory();
    final file = File(
      '${dir.path}/mailclient-${attachment.id}-${attachment.filename}',
    );
    await file.writeAsBytes(bytes, flush: true);
    final result = await OpenFilex.open(file.path);
    if (result.type != ResultType.done) {
      state.showStatus(
        'Could not open ${attachment.filename}: ${result.message}',
        isError: true,
      );
    } else {
      state.showStatus('Opened ${attachment.filename}');
    }
  } catch (e) {
    state.showStatus(
      'Could not open ${attachment.filename}: $e',
      isError: true,
    );
  }
}

Future<void> saveAttachment(
  BuildContext context,
  MessageBody message,
  AttachmentInfo attachment,
) async {
  final state = context.read<MailState>();
  try {
    // The picker writes the bytes itself and hands back where they went.
    final bytes = await attachmentBytes(state, message, attachment.id);
    if (bytes == null) {
      state.showStatus(
        '${attachment.filename} is not downloaded yet',
        isError: true,
      );
      return;
    }
    final uri = await FilePicker.saveFile(
      dialogTitle: 'Save attachment',
      fileName: attachment.filename,
      bytes: Uint8List.fromList(bytes),
      mimeType: attachment.mimeType,
    );
    if (uri == null) return;
    state.showStatus('Saved ${attachment.filename}');
  } catch (e) {
    state.showStatus(
      'Could not save ${attachment.filename}: $e',
      isError: true,
    );
  }
}

Future<void> saveAll(BuildContext context, MessageBody message) async {
  final state = context.read<MailState>();
  try {
    final dir = await FilePicker.getDirectoryPath(
      dialogTitle: 'Save all attachments',
    );
    if (dir == null) return;
    for (final a in message.attachments.where((a) => !a.isInline)) {
      await attachmentBytes(state, message, a.id);
    }
    final n = await MailCore.instance.saveAllAttachmentsTo(
      state.folderId,
      message.uid,
      dir,
    );
    state.showStatus('Saved $n file(s)');
  } catch (e) {
    state.showStatus('Could not save attachments: $e', isError: true);
  }
}

String formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).round()} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
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

/// Deterministic sender avatar, like the Qt Avatar seed.
class SenderAvatar extends StatelessWidget {
  const SenderAvatar({super.key, required this.from});

  final String from;

  @override
  Widget build(BuildContext context) {
    final bg = avatarColor(context, from);
    return CircleAvatar(
      radius: 18,
      backgroundColor: bg,
      foregroundColor: Theme.of(context).colorScheme.onPrimary,
      child: Text(senderInitial(from), style: const TextStyle(fontSize: 15)),
    );
  }
}
