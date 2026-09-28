import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';

/// Mail bodies render on a light sheet in every theme. The sanitizer keeps
/// the sender's colours, and a mail that sets dark text on no background is
/// unreadable on a dark one.
const mailPaperColor = Color(0xFFFFFFFF);
const mailInkColor = Color(0xFF202124);
const mailLinkColor = Color(0xFF1A5FD0);

/// Sanitized mail HTML in the system WebView (Android).
///
/// The input is `mailcore::html::sanitize` output and nothing else; the view
/// only adds belt and braces around it:
/// - JavaScript off, file and content access off;
/// - a Content-Security-Policy that allows no network load at all, except
///   remote images when the user allowed them for this message;
/// - every navigation stopped and handed to [onTapUrl], so a link never
///   opens inside the reader.
class MailWebView extends StatefulWidget {
  const MailWebView({
    super.key,
    required this.html,
    required this.allowRemote,
    this.textScale = 1.0,
    this.onTapUrl,
  });

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

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.disabled)
      ..setBackgroundColor(mailPaperColor)
      ..setNavigationDelegate(
        NavigationDelegate(onNavigationRequest: _onNavigation),
      );
    final platform = _controller.platform;
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
    final doc = mailDocument(widget.html, allowRemote: widget.allowRemote);
    if (doc != _loaded) {
      _loaded = doc;
      _controller.loadHtmlString(doc);
    }
  }

  @override
  Widget build(BuildContext context) => WebViewWidget(controller: _controller);
}

/// The full document handed to the WebView: CSP first, then the sheet
/// styling, then the sanitized body.
///
/// Nothing from the mail can reach `<head>` — the sanitizer drops `head`,
/// `meta` and `style` — and a second CSP could only narrow this one anyway.
String mailDocument(String body, {required bool allowRemote}) {
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
  String hex(Color c) =>
      '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';
  return '<!DOCTYPE html><html><head><meta charset="utf-8">'
      '<meta http-equiv="Content-Security-Policy" '
      'content="${const HtmlEscape(HtmlEscapeMode.attribute).convert(csp)}">'
      // CSP does not cover DNS prefetching of link hosts; this does.
      '<meta http-equiv="x-dns-prefetch-control" content="off">'
      '<meta name="viewport" content="width=device-width, initial-scale=1">'
      '<style>'
      'html,body{background:${hex(mailPaperColor)}}'
      'body{margin:16px;color:${hex(mailInkColor)};font-family:sans-serif;'
      'font-size:15px;line-height:1.5;overflow-wrap:break-word}'
      'a{color:${hex(mailLinkColor)}}'
      // Fixed-width newsletter tables and images shrink to the screen
      // instead of scrolling sideways; author CSS beats their attributes.
      'img{max-width:100%!important;height:auto!important}'
      'table{max-width:100%!important}'
      'pre{white-space:pre-wrap}'
      'blockquote{margin:8px 0;padding-left:12px;'
      'border-left:3px solid #d0d4da;color:#5f6368}'
      '</style></head><body>$body</body></html>';
}
