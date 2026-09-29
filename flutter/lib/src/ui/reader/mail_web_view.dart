import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';

import 'mail_paint.dart';
import 'measure_size.dart';

/// Sanitized mail HTML in the system WebView (Android).
///
/// The input is `mailcore::html::sanitize` output and nothing else; the view
/// only adds belt and braces around it:
/// - JavaScript off, file and content access off;
/// - a Content-Security-Policy that allows no network load at all, except
///   remote images when the user allowed them for this message;
/// - every navigation stopped and handed to [onTapUrl], so a link never
///   opens inside the reader.
///
/// [header] scrolls with the page: it overlays the top of the WebView,
/// follows its scroll position, and the document starts with a spacer of the
/// header's height. The WebView keeps its own scroller — sizing it to the
/// whole mail would make one enormous platform surface for a long newsletter.
class MailWebView extends StatefulWidget {
  const MailWebView({
    super.key,
    required this.html,
    required this.allowRemote,
    required this.paint,
    this.textScale = 1.0,
    this.onTapUrl,
    this.header,
  });

  final Widget? header;

  /// Theme colours, the sender's, or the sender's darkened.
  final MailPaint paint;

  /// Sanitized HTML. Never raw mail source.
  final String html;

  /// Remote images were allowed, so the CSP lets them load.
  final bool allowRemote;

  /// The reader's text size preference, as a multiplier.
  final double textScale;

  final void Function(String url)? onTapUrl;

  @override
  State<MailWebView> createState() => _MailWebViewState();
}

class _MailWebViewState extends State<MailWebView> {
  late final WebViewController _controller;
  String? _loaded;
  int? _zoom;
  Color? _background;

  /// Header height the document reserves; null until first measured, and
  /// nothing loads before that (it would load twice).
  double? _headerHeight;

  /// Page scroll in logical pixels, for the header overlay. A notifier, so
  /// scrolling moves the header without rebuilding the WebView.
  final _scrollY = ValueNotifier<double>(0);

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.disabled)
      ..setNavigationDelegate(
        NavigationDelegate(onNavigationRequest: _onNavigation),
      );
    final platform = _controller.platform;
    // Android reports the View's scroll in physical pixels.
    _controller.setOnScrollPositionChange((change) {
      if (!mounted) return;
      _scrollY.value = change.y / MediaQuery.devicePixelRatioOf(context);
    });
    if (platform is AndroidWebViewController) {
      platform
        ..setAllowFileAccess(false)
        ..setAllowContentAccess(false)
        ..setGeolocationEnabled(false)
        ..setMediaPlaybackRequiresUserGesture(true);
    }
  }

  NavigationDecision _onNavigation(NavigationRequest request) {
    // The document itself arrives as `about:blank` (no base URL).
    if (request.url.startsWith('about:')) return NavigationDecision.navigate;
    if (request.isMainFrame) widget.onTapUrl?.call(request.url);
    return NavigationDecision.prevent;
  }

  @override
  void dispose() {
    _scrollY.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(MailWebView oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  /// Load only what changed: a reload resets the scroll position.
  void _sync() {
    final zoom = MediaQuery.textScalerOf(context)
        .scale(100 * widget.textScale)
        .round();
    final platform = _controller.platform;
    if (zoom != _zoom && platform is AndroidWebViewController) {
      _zoom = zoom;
      platform.setTextZoom(zoom);
    }
    final surface = Theme.of(context).colorScheme.surface;
    final palette = MailPalette.of(context, widget.paint);
    // Behind the document, so nothing flashes white before it paints.
    final background = widget.paint == MailPaint.darkened
        ? surface
        : palette.paper;
    if (background != _background) {
      _background = background;
      _controller.setBackgroundColor(background);
    }
    final top = widget.header == null ? 0.0 : _headerHeight;
    if (top == null) return;
    final doc = mailDocument(
      widget.html,
      allowRemote: widget.allowRemote,
      palette: palette,
      darkenedOn: widget.paint == MailPaint.darkened ? surface : null,
      topSpace: top,
    );
    if (doc != _loaded) {
      _loaded = doc;
      _controller.loadHtmlString(doc);
    }
  }

  @override
  Widget build(BuildContext context) {
    final header = widget.header;
    final web = WebViewWidget(controller: _controller);
    if (header == null) return web;
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return Stack(
      children: [
        Positioned.fill(child: web),
        ValueListenableBuilder<double>(
          valueListenable: _scrollY,
          builder: (context, y, child) =>
              Positioned(top: -y, left: 0, right: 0, child: child!),
          child: GestureDetector(
            // Dragging on the header scrolls the page underneath, as if
            // the header were part of it.
            onVerticalDragUpdate: (d) =>
                _controller.scrollBy(0, (-d.delta.dy * dpr).round()),
            child: MeasureSize(
              onChange: (size) {
                if (!mounted || size.height == _headerHeight) return;
                // The spacer only matters at the top of the page, where a
                // header change (details expanded) happens anyway; the
                // reload that resizes it resets the scroll.
                _headerHeight = size.height;
                _sync();
              },
              child: Material(child: header),
            ),
          ),
        ),
      ],
    );
  }
}

/// The full document handed to the WebView: CSP first, then the sheet
/// styling, then the sanitized body.
///
/// With [darkenedOn] set, the body sits in one inverted block on that
/// colour (see [MailPaint.darkened]); images inside are inverted back.
///
/// Nothing from the mail can reach `<head>` — the sanitizer drops `head`,
/// `meta` and `style` — and a second CSP could only narrow this one anyway.
String mailDocument(
  String body, {
  required bool allowRemote,
  MailPalette palette = MailPalette.light,
  Color? darkenedOn,
  double topSpace = 0,
}) {
  // `cid:` images arrive already embedded as `data:` by the core.
  final img = allowRemote ? "data: https: http:" : "data:";
  final csp = [
    "default-src 'none'",
    "img-src $img",
    "style-src 'unsafe-inline'",
    "font-src 'none'",
    "media-src 'none'",
    "frame-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
  ].join('; ');
  final sheet =
      'background:${cssHex(palette.paper)};color:${cssHex(palette.ink)};'
      'font-family:sans-serif;font-size:15px;line-height:1.5;'
      'overflow-wrap:break-word';
  final layout = darkenedOn == null
      ? 'html,body{background:${cssHex(palette.paper)}}'
            'body{margin:0 16px 16px;$sheet}#mc-top{margin-bottom:16px}'
      : 'html,body{background:${cssHex(darkenedOn)};margin:0}'
            '#mail{$sheet;padding:16px;min-height:100vh;box-sizing:border-box;'
            'filter:$darkInvertCss}'
            '#mail img{filter:$darkInvertCss}';
  // Room for the header overlaying the top of the page (see [MailWebView]).
  final spacer = '<div id="mc-top" style="height:${topSpace.ceil()}px"></div>';
  final content =
      spacer + (darkenedOn == null ? body : '<div id="mail">$body</div>');
  return '<!DOCTYPE html><html><head><meta charset="utf-8">'
      '<meta http-equiv="Content-Security-Policy" '
      'content="${const HtmlEscape(HtmlEscapeMode.attribute).convert(csp)}">'
      // CSP does not cover DNS prefetching of link hosts; this does.
      '<meta http-equiv="x-dns-prefetch-control" content="off">'
      '<meta name="viewport" content="width=device-width, initial-scale=1">'
      '<style>$layout'
      'a{color:${cssHex(palette.link)}}'
      // Fixed-width newsletter tables and images shrink to the screen
      // instead of scrolling sideways; author CSS beats their attributes.
      'img{max-width:100%!important;height:auto!important}'
      'table{max-width:100%!important}'
      'pre{white-space:pre-wrap}'
      'blockquote{margin:8px 0;padding-left:12px;'
      'border-left:3px solid ${cssHex(palette.rule)};'
      'color:${cssHex(palette.quote)}}'
      '</style></head><body>$content</body></html>';
}
