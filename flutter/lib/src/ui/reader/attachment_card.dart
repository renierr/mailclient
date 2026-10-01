import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:open_filex/open_filex.dart';
import 'package:path_provider/path_provider.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';

/// The message's files, like the Qt reader: a bordered card under the
/// header, one row per file with its size and Open / Save on the right.
///
/// Bytes stay in SQLite until the user acts: Open stages through the temp
/// directory into the system viewer (`open_filex`), Save asks where
/// (`file_picker`). A missing download is fetched first, on demand.
class AttachmentCard extends StatelessWidget {
  const AttachmentCard({super.key, required this.message});

  final MessageBody message;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final small = theme.textTheme.bodySmall;
    final files = message.attachments.where((a) => !a.isInline).toList();
    final compact = TextButton.styleFrom(
      visualDensity: VisualDensity.compact,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      minimumSize: const Size(40, 32),
    );
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.fromLTRB(10, 4, 4, 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        border: Border.all(color: scheme.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Icon(Icons.attach_file, size: 16, color: scheme.onSurfaceVariant),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  files.length == 1
                      ? '1 attachment'
                      : '${files.length} attachments',
                  overflow: TextOverflow.ellipsis,
                  style: small?.copyWith(fontWeight: FontWeight.w600),
                ),
              ),
              if (files.length > 1)
                TextButton(
                  style: compact,
                  onPressed: () => saveAll(context, message),
                  child: const Text('Save all'),
                ),
            ],
          ),
          for (final a in files)
            Row(
              children: [
                Expanded(
                  child: Text(
                    a.filename,
                    overflow: TextOverflow.ellipsis,
                    style: small,
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  formatBytes(a.size),
                  style: small?.copyWith(color: scheme.onSurfaceVariant),
                ),
                TextButton(
                  style: compact,
                  onPressed: () => openAttachment(context, message, a),
                  child: const Text('Open'),
                ),
                TextButton(
                  style: compact,
                  onPressed: () => saveAttachment(context, message, a),
                  child: const Text('Save'),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

/// Cached bytes, downloading first when the message arrived without them.
///
/// `downloadAttachments` only queues the job — the bytes land when its
/// `Attachments` finish event arrives — so the waiter is registered *before*
/// queueing and awaited. Awaiting the queue call alone races the download:
/// the re-read below comes back empty and the user has to tap Open twice.
Future<List<int>?> attachmentBytes(
  MailState state,
  MessageBody message,
  int attachmentId,
) async {
  var bytes = await MailCore.instance.attachmentBytes(attachmentId);
  if (bytes != null) return bytes;
  // Parallel downloads share the `Attachments` kind, so a finish event may
  // belong to another message's job: keep waiting while nothing arrived.
  for (var attempt = 0; attempt < 5; attempt++) {
    final finished = state.nextFinished('Attachments');
    try {
      await MailCore.instance.downloadAttachments(
        state.accountId,
        state.folderId,
        message.uid,
      );
    } catch (e) {
      if (!coreErrorText(e).contains('already running')) {
        // The job never started; there is no finish event coming.
        return MailCore.instance.attachmentBytes(attachmentId);
      }
      // Otherwise the bytes are on their way already — wait below.
    }
    await finished;
    bytes = await MailCore.instance.attachmentBytes(attachmentId);
    if (bytes != null) return bytes;
  }
  return null;
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
    // The core names and writes the copy, as for Qt: the mail's own
    // filename never becomes a path.
    final dir = await getTemporaryDirectory();
    final path = await MailCore.instance.writeAttachmentCopy(
      attachment.id,
      '${dir.path}${Platform.pathSeparator}mailclient-attachments',
    );
    final result = await OpenFilex.open(path);
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
      fileName: attachment.fileName,
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
