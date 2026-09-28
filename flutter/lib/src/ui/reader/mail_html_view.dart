import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

import 'mail_web_view.dart';

/// Renders a message body that has already been sanitized by `mailcore`,
/// and scrolls it.
///
/// Two renderers, picked by platform:
///
/// - **Android** uses the system WebView ([MailWebView]): a real engine
///   lays out table-heavy newsletters properly and renders long mails far
///   faster than a widget tree.
/// - **Linux and Windows** use the pure-Dart `flutter_widget_from_html_core`.
///   `webview_flutter` has no backend for either, and Linux is this
///   project's primary OS.
///
/// What must never change is the input: the HTML comes from
/// `mailcore::html::sanitize`, with remote images already stripped unless the
/// user asked for them. Mail HTML is hostile by default — nothing here may
/// start fetching, executing or otherwise trusting it.
class MailHtmlView extends StatefulWidget {
  const MailHtmlView({
    super.key,
    required this.html,
    this.allowRemote = false,
    this.textScale = 1.0,
    this.onTapUrl,
    this.onHoverUrl,
  });

  /// Whether this platform renders mail in a WebView.
  static bool get usesWebView => !kIsWeb && Platform.isAndroid;

  /// Sanitized HTML. Never raw mail source.
  final String html;

  /// Remote images were allowed for this view (the WebView's CSP follows).
  final bool allowRemote;

  /// The reader's text size preference, as a multiplier.
  final double textScale;

  /// Action invoked when user clicks an allowed link.
  final void Function(String url)? onTapUrl;

  /// Callback when user hovers or un-hovers over a link (desktop only).
  final void Function(String? url)? onHoverUrl;

  @override
  State<MailHtmlView> createState() => _MailHtmlViewState();
}

class _MailHtmlViewState extends State<MailHtmlView> {
  /// The built body, reused while its inputs are unchanged. `HtmlWidget`
  /// parses long mails asynchronously and then does not cache: rebuilt by
  /// its parent, it shows its loading state again for a frame, which blanks
  /// the body and jumps the scroll position on every header toggle or link
  /// hover. Returning the same widget instance skips that subtree entirely.
  Widget? _body;
  Object? _bodyKey;

  @override
  Widget build(BuildContext context) {
    if (MailHtmlView.usesWebView) {
      return MailWebView(
        html: widget.html,
        allowRemote: widget.allowRemote,
        textScale: widget.textScale,
        onTapUrl: widget.onTapUrl,
      );
    }
    final base = Theme.of(context).textTheme.bodyMedium;
    final style = base?.copyWith(
      color: mailInkColor,
      fontSize: (base.fontSize ?? 14) * widget.textScale,
    );
    final key = (widget.html, style);
    if (_body == null || _bodyKey != key) {
      _bodyKey = key;
      _body = _build(style);
    }
    return _body!;
  }

  Widget _build(TextStyle? style) {
    final outline = Theme.of(context).colorScheme.outline;
    return ColoredBox(
      color: mailPaperColor,
      child: SelectionArea(
        child: CustomScrollView(
          slivers: [
            SliverPadding(
              padding: const EdgeInsets.all(16),
              // Sliver mode builds top-level blocks lazily as they scroll
              // into view instead of laying out the whole mail up front.
              sliver: HtmlWidget(
                widget.html,
                renderMode: RenderMode.sliverList,
                factoryBuilder: () => _LinkHoverWidgetFactory(
                  onHoverUrl: (url) => widget.onHoverUrl?.call(url),
                ),
                textStyle: style,
                customStylesBuilder: (element) => element.localName == 'a'
                    ? {'color': _cssHex(mailLinkColor)}
                    : null,
                // Read through `widget`, so a cached body still calls the
                // current callback.
                onTapUrl: (url) {
                  final tap = widget.onTapUrl;
                  if (tap == null) return false;
                  tap(url);
                  return true;
                },
                // The sanitizer keeps `cid:` and `data:` images (they
                // travelled with the mail) and strips remote ones unless
                // allowed, so whatever reaches here is already the user's
                // choice. This only has to not widen it.
                onErrorBuilder: (context, element, error) => Text(
                  '[unrenderable content]',
                  style: TextStyle(color: outline),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

String _cssHex(Color c) =>
    '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';

class _LinkHoverWidgetFactory extends WidgetFactory {
  _LinkHoverWidgetFactory({this.onHoverUrl});

  final void Function(String? url)? onHoverUrl;
  final Expando<String> _recognizerUrls = Expando<String>();

  @override
  GestureRecognizer? buildGestureRecognizer(
    BuildTree tree, {
    GestureTapCallback? onTap,
  }) {
    final recognizer = super.buildGestureRecognizer(tree, onTap: onTap);
    if (recognizer != null) {
      final href = tree.element.attributes['href'];
      if (href != null) {
        final url = urlFull(href) ?? href;
        _recognizerUrls[recognizer] = url;
      }
    }
    return recognizer;
  }

  @override
  InlineSpan? buildTextSpan({
    List<InlineSpan>? children,
    GestureRecognizer? recognizer,
    TextStyle? style,
    String? text,
  }) {
    if (text?.isEmpty == true) {
      if (children == null) return null;
      if (children.length == 1) return children.first;
    }

    final url = recognizer != null ? _recognizerUrls[recognizer] : null;

    return TextSpan(
      children: children,
      mouseCursor: recognizer != null ? SystemMouseCursors.click : null,
      recognizer: recognizer,
      onEnter: url != null ? (_) => onHoverUrl?.call(url) : null,
      onExit: url != null ? (_) => onHoverUrl?.call(null) : null,
      style: style,
      text: text,
    );
  }

  @override
  Widget? buildGestureDetector(
    BuildTree tree,
    Widget child,
    GestureRecognizer recognizer,
  ) {
    final widget = super.buildGestureDetector(tree, child, recognizer);
    if (widget == null) return null;
    final url = _recognizerUrls[recognizer];
    if (url != null && onHoverUrl != null) {
      return MouseRegion(
        onEnter: (_) => onHoverUrl?.call(url),
        onExit: (_) => onHoverUrl?.call(null),
        child: widget,
      );
    }
    return widget;
  }
}
