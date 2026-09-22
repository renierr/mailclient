import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';
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
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Header(message: message, onClose: widget.onClose),
        const Divider(height: 1),
        if (message.hasRemoteImages && _htmlWithRemoteImages == null)
          _RemoteImagesBanner(onShowOnce: () => _showRemoteImages(message)),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: message.isHtml
                ? MailHtmlView(html: _htmlWithRemoteImages ?? message.bodyHtml)
                : SelectableText(message.bodyText),
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
  const _Header({required this.message, this.onClose});

  final MessageBody message;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final state = context.read<MailState>();
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
                tooltip: 'Archive',
                icon: const Icon(Icons.archive_outlined),
                onPressed: () => state.archiveMessages([message.uid]),
              ),
              IconButton(
                tooltip: 'Delete',
                icon: const Icon(Icons.delete_outline),
                onPressed: () => state.deleteMessages([message.uid]),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text('${message.from}  ·  ${message.date}',
              style: theme.textTheme.bodySmall),
          if (message.to.isNotEmpty)
            Text('To: ${message.to}', style: theme.textTheme.bodySmall),
          if (message.cc.isNotEmpty)
            Text('Cc: ${message.cc}', style: theme.textTheme.bodySmall),
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

class _AttachmentBar extends StatelessWidget {
  const _AttachmentBar({required this.message});

  final MessageBody message;

  @override
  Widget build(BuildContext context) {
    final files = message.attachments.where((a) => !a.isInline);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: Theme.of(context).dividerColor)),
      ),
      child: Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          for (final a in files)
            Chip(
              avatar: const Icon(Icons.attach_file, size: 16),
              label: Text('${a.filename}  (${_size(a.size)})'),
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
