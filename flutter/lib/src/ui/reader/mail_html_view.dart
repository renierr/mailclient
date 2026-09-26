import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

/// Renders a message body that has already been sanitized by `mailcore`.
///
/// Two implementations, picked by platform, because no single one covers the
/// targets:
///
/// - **Linux** has no `flutter_inappwebview` backend at all (the plugin ships
///   android/ios/macos/web/windows), and Linux is this project's primary OS.
///   It gets the pure-Dart widget renderer.
/// - **Windows and Android** get the same widget renderer today, with the
///   webview kept behind this one seam so swapping it in is a change to this
///   file and nothing else. See `flutter/README.md` for what that swap costs.
///
/// What must never change is the input: the HTML comes from
/// `mailcore::html::sanitize`, with remote images already stripped unless the
/// user asked for them. Mail HTML is hostile by default — nothing here may
/// start fetching, executing or otherwise trusting it.
class MailHtmlView extends StatelessWidget {
  const MailHtmlView({
    super.key,
    required this.html,
    this.textScale = 1.0,
    this.onTapUrl,
    this.onHoverUrl,
  });

  /// Sanitized HTML. Never raw mail source.
  final String html;

  /// The reader's text size preference, as a multiplier.
  final double textScale;

  /// Action invoked when user clicks an allowed link.
  final void Function(String url)? onTapUrl;

  /// Callback when user hovers or un-hovers over a link.
  final void Function(String? url)? onHoverUrl;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SelectionArea(
      child: HtmlWidget(
        html,
        factoryBuilder: () =>
            _LinkHoverWidgetFactory(onHoverUrl: onHoverUrl),
        textStyle: theme.textTheme.bodyMedium?.copyWith(
          fontSize: (theme.textTheme.bodyMedium?.fontSize ?? 14) * textScale,
        ),
        onTapUrl: (url) {
          if (onTapUrl != null) {
            onTapUrl!(url);
            return true;
          }
          return false;
        },
        // The sanitizer keeps `cid:` and `data:` images (they travelled with
        // the mail) and strips remote ones unless allowed, so whatever reaches
        // here is already the user's choice. This only has to not widen it.
        onErrorBuilder: (context, element, error) => Text(
          '[unrenderable content]',
          style: theme.textTheme.bodySmall
              ?.copyWith(color: theme.colorScheme.outline),
        ),
      ),
    );
  }
}

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

/// Whether a real browser engine is available for the reader on this platform.
///
/// Used by the About view to explain which renderer is in use, so "that mail
/// looks wrong" has an answer that does not require reading the source.
bool get hasWebviewRenderer =>
    Platform.isWindows || Platform.isAndroid || Platform.isMacOS;
