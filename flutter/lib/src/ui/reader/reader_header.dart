import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import '../dialogs/sender_avatar.dart';
import '../menu_row.dart';
import '../message_list/message_list_pane.dart' show confirmDelete;
import 'reader_widgets.dart';

/// Subject, sender and actions, laid out like the Qt reader: the subject
/// (with back in front where the layout needs it), the sender block with a
/// details chevron, then the actions right-aligned on their own row.
class ReaderHeader extends StatelessWidget {
  const ReaderHeader({
    super.key,
    required this.message,
    required this.headersFuture,
    required this.details,
    required this.onToggleDetails,
    this.onClose,
    this.originalColors = false,
    this.onToggleColors,
    this.onShowRemoteImages,
    this.passThrough = false,
  });

  final MessageBody message;
  final Future<MessageHeaders?>? headersFuture;
  final bool details;
  final VoidCallback onToggleDetails;

  /// Back in front of the subject; null hides it.
  final VoidCallback? onClose;

  /// A designed mail in a dark theme: the sender's colours are shown as
  /// sent rather than darkened. Only meaningful with [onToggleColors].
  final bool originalColors;

  /// Null hides the colours toggle (nothing to darken).
  final VoidCallback? onToggleColors;

  /// Remote images were blocked: the menu offers to show them once. Null
  /// hides the entry.
  final VoidCallback? onShowRemoteImages;

  /// The header overlays a WebView that owns the scroll (Android): display
  /// text lets touches fall through to the page underneath, so drags and
  /// flings starting on the header are the WebView's own native ones
  /// instead of a forwarded copy. Buttons stay tappable; semantics stay on.
  final bool passThrough;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final muted = theme.textTheme.bodySmall?.copyWith(
      color: scheme.onSurfaceVariant,
    );
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
    // Display text never handles a touch: wrapped, its hit test passes the
    // gesture to the WebView below, so drags and flings starting on it are
    // native. Semantics stay on for screen readers.
    Widget flow(Widget child) =>
        passThrough ? IgnorePointer(child: child) : child;
    final divider = BoxDecoration(
      border: Border(bottom: BorderSide(color: theme.dividerColor)),
    );
    final content = Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 12, 8),
      child: FutureBuilder<MessageHeaders?>(
        future: headersFuture,
        builder: (context, snap) {
          final h = snap.data;
          // Sender and the Reply-To decision come parsed from the core,
          // as in Qt; the full headers add To/Cc and the long date.
          String pick(String? full, String fallback) =>
              (full?.isNotEmpty ?? false) ? full! : fallback;
          final from = pick(h?.from, message.from);
          final to = pick(h?.to, message.to);
          final cc = pick(h?.cc, message.cc);
          final date = pick(h?.date, message.date);
          final replyTo = message.replyTo;
          final replyToDiffers = message.replyToDiffers;
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  if (onClose != null)
                    IconButton(
                      tooltip: 'Back to the list',
                      icon: const Icon(Icons.arrow_back),
                      onPressed: onClose,
                    ),
                  Expanded(
                    child: flow(
                      Text(
                        message.subject,
                        style: theme.textTheme.titleLarge?.copyWith(
                          fontWeight: FontWeight.w700,
                        ),
                        maxLines: 3,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  flow(SenderAvatar(badge: message.badge)),
                  const SizedBox(width: 12),
                  Expanded(
                    child: flow(
                      Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            children: [
                              Expanded(
                                child: Text(
                                  message.senderName,
                                  overflow: TextOverflow.ellipsis,
                                  style: theme.textTheme.bodyMedium?.copyWith(
                                    fontWeight: FontWeight.w700,
                                  ),
                                ),
                              ),
                              const SizedBox(width: 8),
                              Text(message.date, style: muted),
                            ],
                          ),
                          if (message.fromName.isNotEmpty)
                            Text(
                              message.from,
                              overflow: TextOverflow.ellipsis,
                              style: muted,
                            ),
                          if (!details && to.isNotEmpty)
                            Text(
                              'To $to',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: muted,
                            ),
                          // Shown inline, not only in the details: replying
                          // to the wrong address cannot be taken back.
                          if (replyToDiffers)
                            Text(
                              'Replies go to $replyTo, not to the sender',
                              overflow: TextOverflow.ellipsis,
                              style: muted?.copyWith(color: scheme.error),
                            ),
                        ],
                      ),
                    ),
                  ),
                  IconButton(
                    tooltip: details ? 'Hide details' : 'Show details',
                    icon: Icon(
                      details ? Icons.expand_more : Icons.chevron_right,
                    ),
                    onPressed: onToggleDetails,
                  ),
                ],
              ),
              if (details)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: flow(
                    Column(
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
                    ),
                  ),
                ),
              const SizedBox(height: 4),
              // Right-aligned like the Qt action row; Wrap so a 360px
              // phone stacks them instead of overflowing.
              Wrap(
                alignment: WrapAlignment.end,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  IconButton(
                    tooltip: 'Reply',
                    icon: const Icon(Icons.reply_outlined),
                    onPressed: () => ComposerDialog.showReply(context, message),
                  ),
                  IconButton(
                    tooltip: 'Forward',
                    icon: const Icon(Icons.forward_outlined),
                    onPressed: () =>
                        ComposerDialog.showForward(context, message),
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
                    color: scheme.error,
                    icon: const Icon(Icons.delete_outline),
                    onPressed: () => confirmDelete(
                      context,
                      context.read<MailState>(),
                      uids: [message.uid],
                      permanent: permanent,
                    ),
                  ),
                  if (onToggleColors != null)
                    IconButton(
                      tooltip: originalColors
                          ? 'Darken to match the theme'
                          : 'Show original colours',
                      icon: Icon(
                        originalColors
                            ? Icons.dark_mode_outlined
                            : Icons.invert_colors,
                      ),
                      onPressed: onToggleColors,
                    ),
                  IconButton(
                    tooltip: fullscreen ? 'Exit full screen' : 'Full screen',
                    icon: Icon(
                      fullscreen ? Icons.close_fullscreen : Icons.open_in_full,
                    ),
                    onPressed: () =>
                        context.read<MailState>().toggleReaderFullscreen(),
                  ),
                  PopupMenuButton<String>(
                    tooltip: 'More actions',
                    icon: const Icon(Icons.more_vert),
                    onSelected: (v) => v == 'remote'
                        ? onShowRemoteImages?.call()
                        : moreActions(context, message, v),
                    itemBuilder: (context) => [
                      const PopupMenuItem(
                        value: 'reply-all',
                        child: MenuRow(
                          icon: Icons.reply_all_outlined,
                          text: 'Reply all',
                        ),
                      ),
                      const PopupMenuItem(
                        value: 'archive',
                        child: MenuRow(
                          icon: Icons.archive_outlined,
                          text: 'Archive',
                        ),
                      ),
                      const PopupMenuItem(
                        value: 'move',
                        child: MenuRow(
                          icon: Icons.drive_file_move_outlined,
                          text: 'Move to…',
                        ),
                      ),
                      const PopupMenuItem(
                        value: 'purge',
                        child: MenuRow(
                          icon: Icons.delete_forever_outlined,
                          text: 'Delete permanently…',
                        ),
                      ),
                      const PopupMenuDivider(),
                      const PopupMenuItem(
                        value: 'headers',
                        child: MenuRow(
                          icon: Icons.info_outline,
                          text: 'Show headers…',
                        ),
                      ),
                      // Blocked for tracking protection; loading them
                      // tells the sender the mail was opened.
                      if (onShowRemoteImages != null)
                        const PopupMenuItem(
                          value: 'remote',
                          child: MenuRow(
                            icon: Icons.image_outlined,
                            text: 'Show remote images',
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ],
          );
        },
      ),
    );
    if (!passThrough) {
      return DecoratedBox(decoration: divider, child: content);
    }
    // The divider paints under an IgnorePointer so it never claims a
    // touch; the content sits above it, itself transparent except for
    // the buttons.
    return Stack(
      children: [
        Positioned.fill(
          child: IgnorePointer(child: DecoratedBox(decoration: divider)),
        ),
        content,
      ],
    );
  }
}
