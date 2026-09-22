import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import '../message_list/message_list_pane.dart' show confirmDelete;
import '../move_to/move_to_dialog.dart';
import 'mail_html_view.dart';

/// The selected message.
///
/// The body arrives already sanitized. The one thing this pane may do with
/// remote content is ask the core to re-sanitize with images allowed, and only
/// because the user pressed the button that says so.
class ReaderPane extends StatefulWidget {
  const ReaderPane({super.key, this.onClose});

  /// Shown as a back affordance in the narrow layouts.
  final VoidCallback? onClose;

  @override
  State<ReaderPane> createState() => _ReaderPaneState();
}

class _ReaderPaneState extends State<ReaderPane> {
  /// Remote images the user allowed for *this* message only. Reset whenever
  /// the selection changes, because "show once" has to mean once.
  String? _htmlWithRemoteImages;
  int _shownForUid = -1;
  bool _details = false;

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final message = state.openBody;

    if (state.openUid < 0) {
      return _Placeholder(text: 'Select a message');
    }
    if (message == null) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_shownForUid != message.uid) {
      _htmlWithRemoteImages = null;
      _shownForUid = message.uid;
      _details = false;
    }

    final scale = state.settings.readerScale;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Header(
          message: message,
          details: _details,
          onToggleDetails: () =>
              setState(() => _details = !_details),
          onClose: widget.onClose,
        ),
        const Divider(height: 1),
        if (message.hasRemoteImages &&
            _htmlWithRemoteImages == null &&
            !state.settings.loadRemoteImages)
          _RemoteImagesBanner(onShowOnce: () => _showRemoteImages(message)),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: message.isHtml
                ? MailHtmlView(
                    html: _htmlWithRemoteImages ?? message.bodyHtml)
                : MediaQuery(
                    data: MediaQuery.of(context).copyWith(
                      textScaler: TextScaler.linear(scale),
                    ),
                    child: SelectableText(message.bodyText),
                  ),
          ),
        ),
        if (message.attachments.any((a) => !a.isInline))
          _AttachmentBar(message: message),
      ],
    );
  }

  Future<void> _showRemoteImages(MessageBody message) async {
    final state = context.read<MailState>();
    // Re-sanitized by the core rather than patched here: the list feed stripped
    // the remote references entirely, so there is nothing local to un-strip.
    final html = await MailCore.instance
        .messageHtmlWithRemoteImages(state.folderId, message.uid);
    if (!mounted) return;
    setState(() => _htmlWithRemoteImages = html);
  }
}

class _Header extends StatelessWidget {
  const _Header({
    required this.message,
    required this.details,
    required this.onToggleDetails,
    this.onClose,
  });

  final MessageBody message;
  final bool details;
  final VoidCallback onToggleDetails;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final state = context.read<MailState>();
    final starred = context.select<MailState, bool>((s) => s.messages
        .where((m) => m.uid == message.uid)
        .firstOrNull
        ?.starred ??
        false);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              if (onClose != null)
                IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: onClose,
                ),
              Expanded(
                child: Text(message.subject,
                    style: theme.textTheme.titleMedium,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis),
              ),
              IconButton(
                tooltip: 'Reply',
                icon: const Icon(Icons.reply_outlined),
                onPressed: () =>
                    ComposerDialog.showReply(context, message),
              ),
              IconButton(
                tooltip: starred ? 'Unstar' : 'Star',
                icon: Icon(
                  starred ? Icons.star : Icons.star_border,
                  color: starred ? Colors.amber.shade700 : null,
                ),
                onPressed: () => state.toggleStar(message.uid),
              ),
              IconButton(
                tooltip: 'Delete',
                icon: const Icon(Icons.delete_outline),
                onPressed: () => confirmDelete(context, state,
                    uids: [message.uid],
                    permanent: state.deleteIsPermanent),
              ),
              PopupMenuButton<String>(
                icon: const Icon(Icons.more_vert),
                onSelected: (v) => _more(context, state, v),
                itemBuilder: (context) => const [
                  PopupMenuItem(
                      value: 'forward', child: Text('Forward')),
                  PopupMenuItem(
                      value: 'reply-all',
                      child: Text('Reply all')),
                  PopupMenuItem(
                      value: 'archive', child: Text('Archive')),
                  PopupMenuItem(
                      value: 'move', child: Text('Move to…')),
                  PopupMenuItem(
                      value: 'purge',
                      child: Text('Delete permanently…')),
                  PopupMenuItem(
                      value: 'headers',
                      child: Text('Show headers…')),
                ],
              ),
            ],
          ),
          const SizedBox(height: 4),
          InkWell(
            onTap: onToggleDetails,
            child: Row(
              children: [
                Expanded(
                  child: Text('${message.from}  ·  ${message.date}',
                      style: theme.textTheme.bodySmall),
                ),
                Icon(details
                    ? Icons.expand_less
                    : Icons.expand_more),
              ],
            ),
          ),
          if (message.to.isNotEmpty)
            Text('To: ${message.to}',
                style: theme.textTheme.bodySmall),
          if (details && message.cc.isNotEmpty)
            Text('Cc: ${message.cc}',
                style: theme.textTheme.bodySmall),
          // Shown inline, not hidden in a details view: replying to the wrong
          // address is not something the user can take back.
          if (message.replyTo.isNotEmpty)
            Text(
              'Replies go to: ${message.replyTo}',
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.primary),
            ),
        ],
      ),
    );
  }

  Future<void> _more(
      BuildContext context, MailState state, String v) async {
    switch (v) {
      case 'forward':
        if (context.mounted) {
          await ComposerDialog.showForward(context, message);
        }
      case 'reply-all':
        if (context.mounted) {
          await ComposerDialog.showReply(context, message,
              replyAll: true);
        }
      case 'archive':
        await state.archiveMessages([message.uid]);
      case 'move':
        if (context.mounted) {
          await MoveToDialog.show(context,
              uids: [message.uid], subject: message.subject);
        }
      case 'purge':
        if (context.mounted) {
          await confirmDelete(context, state,
              uids: [message.uid], permanent: true, purge: true);
        }
      case 'headers':
        if (context.mounted) _showHeaders(context, state);
    }
  }

  Future<void> _showHeaders(BuildContext context, MailState state) async {
    late MessageHeaders headers;
    try {
      headers = await MailCore.instance
          .messageHeaders(state.folderId, message.uid);
    } catch (e) {
      if (!context.mounted) return;
      state.showStatus('$e', isError: true);
      return;
    }
    if (!context.mounted) return;
    await showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Headers'),
        content: SizedBox(
          width: 480,
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
                      child: SelectableText(
                          '${row.key}: ${row.value}'),
                    ),
                if (headers.raw.isNotEmpty)
                  ExpansionTile(
                    title: const Text('Complete headers'),
                    tilePadding: EdgeInsets.zero,
                    children: [
                      SelectableText(
                        headers.raw,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 12),
                      ),
                    ],
                  )
                else
                  const Text(
                      'Complete headers are unavailable until this message is downloaded again.'),
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
}

class _RemoteImagesBanner extends StatelessWidget {
  const _RemoteImagesBanner({required this.onShowOnce});

  final VoidCallback onShowOnce;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      color: scheme.surfaceContainerHighest,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(
        children: [
          Icon(Icons.image_not_supported_outlined,
              size: 18, color: scheme.outline),
          const SizedBox(width: 8),
          const Expanded(
            child: Text(
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

/// Attachment files with mock Open/Save actions.
///
/// Downloading and opening files needs the file-picker/opener decision that is
/// explicitly parked: the buttons are visible and honest about it rather than
/// absent, so the layout they will live in is already real.
class _AttachmentBar extends StatelessWidget {
  const _AttachmentBar({required this.message});

  final MessageBody message;

  @override
  Widget build(BuildContext context) {
    final state = context.read<MailState>();
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
              Text(
                  '📎 ${files.length} attachment(s)',
                  style: Theme.of(context).textTheme.bodySmall),
              const Spacer(),
              if (files.length > 1)
                TextButton(
                  onPressed: () => state.showStatus(
                      'Saving files is not wired yet — the file picker is still an open decision.'),
                  child: const Text('Save all (mock)'),
                ),
            ],
          ),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final a in files)
                Chip(
                  avatar: const Icon(Icons.attach_file, size: 16),
                  label: Text('${a.filename}  (${_size(a.size)})'),
                  deleteIcon: const Icon(Icons.open_in_new, size: 16),
                  onDeleted: () => state.showStatus(
                      'Opening files is not wired yet — the file opener is still an open decision.'),
                ),
            ],
          ),
        ],
      ),
    );
  }

  static String _size(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).round()} KB';
    return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
}

class _Placeholder extends StatelessWidget {
  const _Placeholder({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Text(text, style: TextStyle(color: scheme.outline)),
    );
  }
}
