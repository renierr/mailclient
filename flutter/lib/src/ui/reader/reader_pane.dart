import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import 'link_safety.dart';
import 'mail_html_view.dart';
import 'reader_widgets.dart';

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
  State<ReaderPane> createState() => ReaderPaneState();
}

class ReaderPaneState extends State<ReaderPane> {
  /// Remote images the user allowed for *this* message only. Reset whenever
  /// the selection changes, because "show once" has to mean once. Keyed on
  /// folder *and* uid: uids are only unique within a folder.
  String? _htmlWithRemoteImages;
  (int, int) _shownFor = (-1, -1);
  bool _details = false;
  String? _hoveredLinkUrl;

  /// Full `From:` header for the display name. The list feed only carries the
  /// bare address, so the name comes from here — the same source the Qt
  /// reader uses. Null while loading or when the headers are gone.
  Future<MessageHeaders?>? _headersFuture;
  (int, int) _headersFor = (-1, -1);

  @override
  Widget build(BuildContext context) {
    final openUid = context.select<MailState, int>((s) => s.openUid);
    final folderId = context.select<MailState, int>((s) => s.folderId);
    final message = context.select<MailState, MessageBody?>((s) => s.openBody);
    final loadRemote = context.select<MailState, bool>(
      (s) => s.settings.loadRemoteImages,
    );
    final scale = context.select<MailState, double>(
      (s) => s.settings.readerScale,
    );

    if (openUid < 0) {
      return const ReaderPlaceholder(text: 'Select a message');
    }
    if (message == null) {
      return const Center(child: CircularProgressIndicator());
    }
    final key = (folderId, message.uid);
    if (_shownFor != key) {
      _htmlWithRemoteImages = null;
      _hoveredLinkUrl = null;
      _shownFor = key;
      _details = false;
    }
    if (_headersFor != key) {
      _headersFor = key;
      _headersFuture = MailCore.instance
          .messageHeaders(folderId, message.uid)
          .then<MessageHeaders?>((h) => h)
          .catchError((_) => null);
    }

    final theme = Theme.of(context);
    return Stack(
      children: [
        Positioned.fill(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              ReaderHeader(
                message: message,
                headersFuture: _headersFuture,
                details: _details,
                onToggleDetails: () => setState(() => _details = !_details),
                onClose: widget.onClose,
              ),
              const Divider(height: 1),
              if (message.hasRemoteImages &&
                  _htmlWithRemoteImages == null &&
                  !loadRemote)
                RemoteImagesBanner(
                  onShowOnce: () => _showRemoteImages(message),
                ),
              Expanded(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.all(16),
                  child: message.isHtml
                      ? MailHtmlView(
                          html: _htmlWithRemoteImages ?? message.bodyHtml,
                          textScale: scale,
                          onHoverUrl: (url) {
                            if (_hoveredLinkUrl != url) {
                              setState(() => _hoveredLinkUrl = url);
                            }
                          },
                          onTapUrl: (url) => _handleLinkUrl(context, url),
                        )
                      : MediaQuery(
                          data: MediaQuery.of(context)
                              .copyWith(textScaler: TextScaler.linear(scale)),
                          child: SelectableText(message.bodyText),
                        ),
                ),
              ),
              if (message.attachments.any((a) => !a.isInline))
                AttachmentBar(message: message),
            ],
          ),
        ),
        if (_hoveredLinkUrl != null && _hoveredLinkUrl!.isNotEmpty)
          Positioned(
            left: 16,
            right: 16,
            bottom: 16,
            child: GestureDetector(
              // Desktop hover bubble doubles as a touch affordance: tap
              // copies the URL, so long-press is not the only way.
              onTap: () {
                final url = _hoveredLinkUrl;
                if (url != null && url.isNotEmpty) {
                  Clipboard.setData(ClipboardData(text: url));
                  ScaffoldMessenger.of(
                    context,
                  ).showSnackBar(const SnackBar(content: Text('Link copied')));
                }
              },
              child: Align(
                alignment: Alignment.centerLeft,
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 10,
                    vertical: 6,
                  ),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.surfaceContainerHighest,
                    borderRadius: BorderRadius.circular(6),
                    border: Border.all(color: theme.colorScheme.outlineVariant),
                    boxShadow: [
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.12),
                        blurRadius: 4,
                        offset: const Offset(0, 2),
                      ),
                    ],
                  ),
                  child: Text(
                    _hoveredLinkUrl!,
                    style: theme.textTheme.bodySmall?.copyWith(
                      fontFamily: 'monospace',
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }

  Future<void> _handleLinkUrl(BuildContext context, String url) async {
    if (!LinkSafety.isWebScheme(url)) return;
    final action = LinkSafety.actionFor(
      context.read<MailState>().settings.linkClickAction,
    );
    if (action == 'browser') {
      await LinkSafety.openUrl(url);
      if (context.mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Opened in browser')));
      }
    } else {
      if (context.mounted) {
        await ExamineLinkDialog.show(context, url);
      }
    }
  }

  Future<void> _showRemoteImages(MessageBody message) async {
    final key = _shownFor;
    // Re-sanitized by the core rather than patched here: the list feed stripped
    // the remote references entirely, so there is nothing local to un-strip.
    final html = await MailCore.instance.messageHtmlWithRemoteImages(
      key.$1,
      message.uid,
    );
    // The user may have moved on while it loaded.
    if (!mounted || _shownFor != key) return;
    setState(() => _htmlWithRemoteImages = html);
  }
}
