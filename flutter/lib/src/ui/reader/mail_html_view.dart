import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

import '../../ffi/mail_core.dart';
import 'image_baseline.dart';
import 'mail_paint.dart';
import 'mail_web_view.dart';

/// Renders a message body that has already been sanitized by `mailcore`,
/// and scrolls it together with [header] above it, as one page.
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
    this.paint = MailPaint.original,
    this.allowRemote = false,
    this.textScale = 1.0,
    this.onTapUrl,
    this.onHoverUrl,
    this.header,
    this.headerReady,
    this.fitWidths = true,
  });

  /// Scrolls away with the body (the reader's header and attachments).
  final Widget? header;

  /// Completes once [header] has its final content (see [MailWebView]).
  final Future<Object?>? headerReady;

  /// Narrow pages loosen fixed-width newsletter layouts to fit. False keeps
  /// the mail's original fixed-width layout — paired with the reader's
  /// "show original colours" toggle, the mail is shown as sent.
  final bool fitWidths;

  /// Whether this platform renders mail in a WebView.
  static bool get usesWebView => !kIsWeb && Platform.isAndroid;

  /// Sanitized HTML. Never raw mail source.
  final String html;

  /// Theme colours, the sender's, or the sender's darkened.
  final MailPaint paint;

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
        paint: widget.paint,
        textScale: widget.textScale,
        onTapUrl: widget.onTapUrl,
        header: widget.header,
        headerReady: widget.headerReady,
        fitWidths: widget.fitWidths,
      );
    }
    final palette = MailPalette.of(context, widget.paint);
    final base = Theme.of(context).textTheme.bodyMedium;
    final style = base?.copyWith(
      color: palette.ink,
      fontSize: (base.fontSize ?? 14) * widget.textScale,
    );
    final key = (widget.html, style, widget.paint, palette.paper, palette.link);
    if (_body == null || _bodyKey != key) {
      _bodyKey = key;
      // A darkened mail arrives with its colours rewritten by the core.
      final html = widget.paint == MailPaint.darkened
          ? MailCore.instance.readerBody(widget.html, widget.paint)
          : widget.html;
      _body = _build(html, style, palette);
    }
    // The header rebuilds freely; the body sliver is the cached instance.
    return SelectionArea(
      child: CustomScrollView(
        slivers: [
          if (widget.header case final Widget header)
            SliverToBoxAdapter(child: header),
          _body!,
          // Paper down to the bottom of a short mail.
          SliverFillRemaining(
            hasScrollBody: false,
            child: ColoredBox(color: palette.paper),
          ),
        ],
      ),
    );
  }

  /// The body as a sliver, built lazily as it scrolls into view.
  Widget _build(String body, TextStyle? style, MailPalette palette) {
    final outline = Theme.of(context).colorScheme.outline;
    final html = HtmlWidget(
      body,
      // A new paint needs a new factory, which only initState builds.
      key: ValueKey(widget.paint),
      renderMode: RenderMode.sliverList,
      factoryBuilder: () => _LinkHoverWidgetFactory(
        onHoverUrl: (url) => widget.onHoverUrl?.call(url),
      ),
      textStyle: style,
      customStylesBuilder: (element) =>
          element.localName == 'a' ? {'color': cssHex(palette.link)} : null,
      // Read through `widget`, so a cached body still calls the current
      // callback.
      onTapUrl: (url) {
        final tap = widget.onTapUrl;
        if (tap == null) return false;
        tap(url);
        return true;
      },
      // The sanitizer keeps `cid:` and `data:` images (they travelled with
      // the mail) and strips remote ones unless allowed, so whatever
      // reaches here is already the user's choice. This only has to not
      // widen it.
      onErrorBuilder: (context, element, error) =>
          Text('[unrenderable content]', style: TextStyle(color: outline)),
    );
    return DecoratedSliver(
      decoration: BoxDecoration(color: palette.paper),
      sliver: SliverPadding(
        padding: const EdgeInsets.all(16),
        // Sliver mode builds top-level blocks lazily as they scroll into
        // view instead of laying out the whole mail up front.
        sliver: html,
      ),
    );
  }
}

class _LinkHoverWidgetFactory extends WidgetFactory {
  _LinkHoverWidgetFactory({this.onHoverUrl});

  final void Function(String? url)? onHoverUrl;

  @override
  Widget? buildImageWidget(BuildTree tree, ImageSource src) {
    final image = super.buildImageWidget(tree, src);
    if (image == null) return null;
    return ImageBaseline(child: image);
  }

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
