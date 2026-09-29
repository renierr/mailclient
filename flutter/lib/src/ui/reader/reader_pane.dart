import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import 'inline_images_banner.dart';
import 'link_safety.dart';
import 'mail_html_view.dart';
import 'mail_paint.dart';
import 'reader_widgets.dart';

/// The selected message.
///
/// The body arrives already sanitized. The one thing this pane may do with
/// remote content is ask the core to re-sanitize with images allowed, and only
/// because the user pressed the button that says so.
class ReaderPane extends StatefulWidget {
  const ReaderPane({super.key, this.onClose, this.allowFullscreen = true});

  /// Back affordance in the reader header — the two-pane layout, where the
  /// app bar has no back button of its own.
  final VoidCallback? onClose;

  /// Only the three-pane layout offers fullscreen; narrower ones already give
  /// the reader every pixel.
  final bool allowFullscreen;

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

  /// The inline-image download was asked for on the shown message.
  bool _inlineRequested = false;

  /// Designed mail in a dark theme: show the sender's colours instead of
  /// darkening them. Per message, like "Show once".
  bool _originalColors = false;

  /// Link under the mouse. A notifier rather than state, so hovering
  /// repaints the bubble and not the whole pane with the mail in it.
  final _hoveredLink = ValueNotifier<String?>(null);

  @override
  void dispose() {
    _hoveredLink.dispose();
    super.dispose();
  }

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
      _hoveredLink.value = null;
      _shownFor = key;
      _details = false;
      _inlineRequested = false;
      _originalColors = false;
    }
    if (_headersFor != key) {
      _headersFor = key;
      _headersFuture = MailCore.instance
          .messageHeaders(folderId, message.uid)
          .then<MessageHeaders?>((h) => h)
          .catchError((_) => null);
    }

    final theme = Theme.of(context);
    final dark = theme.brightness == Brightness.dark;
    final paint = mailPaintFor(
      colored: message.htmlColored,
      dark: dark,
      keepOriginal: _originalColors,
    );
    final canToggleColors = message.isHtml && message.htmlColored && dark;
    // Expand: the only non-positioned child is the hover bubble, empty when
    // no link is hovered, so a loose parent (the shell's Row) would size the
    // Stack — and the whole reader with it — to nothing.
    return Stack(
      fit: StackFit.expand,
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
                allowFullscreen: widget.allowFullscreen,
                originalColors: _originalColors,
                onToggleColors: canToggleColors
                    ? () => setState(() => _originalColors = !_originalColors)
                    : null,
              ),
              const Divider(height: 1),
              if (message.hasRemoteImages &&
                  _htmlWithRemoteImages == null &&
                  !loadRemote)
                RemoteImagesBanner(
                  onShowOnce: () => _showRemoteImages(message),
                ),
              if (message.isHtml && message.missingInlineImages > 0)
                InlineImagesBanner(
                  count: message.missingInlineImages,
                  busy: _inlineRequested,
                  onDownload: () => _downloadInlineImages(message),
                ),
              Expanded(
                child: message.isHtml
                    ? MailHtmlView(
                        html: _htmlWithRemoteImages ?? message.bodyHtml,
                        paint: paint,
                        allowRemote:
                            loadRemote || _htmlWithRemoteImages != null,
                        textScale: scale,
                        onHoverUrl: (url) => _hoveredLink.value = url,
                        onTapUrl: (url) => _handleLinkUrl(context, url),
                      )
                    : SingleChildScrollView(
                        padding: const EdgeInsets.all(16),
                        child: MediaQuery(
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
        ValueListenableBuilder<String?>(
          valueListenable: _hoveredLink,
          builder: (context, hovered, _) {
            if (hovered == null || hovered.isEmpty) {
              return const SizedBox.shrink();
            }
            return Positioned(
              left: 16,
              right: 16,
              bottom: 16,
              child: GestureDetector(
                // Desktop hover bubble doubles as a touch affordance: tap
                // copies the URL, so long-press is not the only way.
                onTap: () {
                  Clipboard.setData(ClipboardData(text: hovered));
                  ScaffoldMessenger.of(
                    context,
                  ).showSnackBar(const SnackBar(content: Text('Link copied')));
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
                      border: Border.all(
                        color: theme.colorScheme.outlineVariant,
                      ),
                      boxShadow: [
                        BoxShadow(
                          color: Colors.black.withValues(alpha: 0.12),
                          blurRadius: 4,
                          offset: const Offset(0, 2),
                        ),
                      ],
                    ),
                    child: Text(
                      hovered,
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
            );
          },
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

  /// Fetches the message's parts from the user's own server; the finished
  /// job re-reads the open message, which then embeds them.
  Future<void> _downloadInlineImages(MessageBody message) async {
    final state = context.read<MailState>();
    setState(() => _inlineRequested = true);
    try {
      await MailCore.instance.downloadAttachments(
        state.accountId,
        _shownFor.$1,
        message.uid,
      );
    } catch (e) {
      if (!mounted) return;
      setState(() => _inlineRequested = false);
      state.showStatus('Could not download images: $e', isError: true);
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
