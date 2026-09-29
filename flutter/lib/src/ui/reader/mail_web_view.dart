import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';

import 'mail_dark.dart';
import 'mail_fit.dart';
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
    this.headerReady,
    this.fitWidths = true,
  });

  final Widget? header;

  /// Narrow pages loosen fixed-width newsletter layouts to fit (see
  /// [fitMailWidths]). False keeps the mail's original fixed-width layout,
  /// sideways scroll included — used with the reader's "show original
  /// colours" toggle, where the mail is shown as sent in both respects.
  final bool fitWidths;

  /// Completes once [header] has its final content (the full headers are
  /// read after the body). The first load waits for it: a header that grew
  /// afterwards would reload the whole mail to resize the spacer.
  final Future<Object?>? headerReady;

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

  /// Inputs of the loaded document. Compared instead of the document
  /// itself, which for a mail with embedded images is megabytes long.
  Object? _loaded;
  int? _zoom;
  Color? _background;

  /// Header height the document reserves; null until first measured, and
  /// nothing loads before that (it would load twice).
  double? _headerHeight;

  /// Page width in logical (= CSS) pixels; null until laid out.
  double? _width;

  /// [mailLayoutWidth] of [MailWebView.html], computed once per mail.
  String? _layoutFor;
  int _layoutWidth = 0;

  /// [MailWebView.headerReady] has completed for the shown mail.
  bool _headerReady = false;

  /// The shown mail has been loaded once. Until then, triggers are
  /// coalesced so the header settles and the mail loads once.
  bool _shown = false;
  Timer? _settle;

  /// Page scroll in logical pixels, for the header overlay. A notifier, so
  /// scrolling moves the header without rebuilding the WebView.
  final _scrollY = ValueNotifier<double>(0);

  /// Cached display density: the scroll callback fires per scroll delta and
  /// must not do a [MediaQuery] lookup on every one.
  double? _dpr;

  /// Latest scroll offset, flushed to [_scrollY] at most once per frame: the
  /// platform reports many deltas per vsync and each notifier set rebuilds
  /// the overlay.
  double _pendingY = 0;
  bool _scrollScheduled = false;

  /// Drag distance on the header overlay, flushed to the page as one
  /// `scrollBy` per frame instead of one platform-channel call per motion
  /// event.
  double _pendingDy = 0;
  bool _dragScheduled = false;
  Timer? _fling;

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
      final y = change.y / (_dpr ?? MediaQuery.devicePixelRatioOf(context));
      // Once the header is off the top it stays put: the notifier holds
      // one value and nothing rebuilds while the mail scrolls.
      final gone = (_headerHeight ?? 0) + 1;
      _pendingY = y < gone ? y : gone;
      if (_scrollScheduled || _scrollY.value == _pendingY) return;
      _scrollScheduled = true;
      SchedulerBinding.instance.scheduleFrameCallback((_) {
        _scrollScheduled = false;
        if (!mounted) return;
        if (_scrollY.value != _pendingY) _scrollY.value = _pendingY;
      });
    });
    if (platform is AndroidWebViewController) {
      platform
        ..setAllowFileAccess(false)
        ..setAllowContentAccess(false)
        ..setGeolocationEnabled(false)
        ..setMediaPlaybackRequiresUserGesture(true);
    }
    _watchHeader();
  }

  NavigationDecision _onNavigation(NavigationRequest request) {
    // The document itself arrives as `about:blank` (no base URL).
    if (request.url.startsWith('about:')) return NavigationDecision.navigate;
    if (request.isMainFrame) widget.onTapUrl?.call(request.url);
    return NavigationDecision.prevent;
  }

  @override
  void dispose() {
    _settle?.cancel();
    _fling?.cancel();
    _scrollY.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _dpr = MediaQuery.devicePixelRatioOf(context);
    _requestSync();
  }

  @override
  void didUpdateWidget(MailWebView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.headerReady != oldWidget.headerReady) _watchHeader();
    if (widget.html != oldWidget.html) _stopFling();
    _requestSync();
  }

  /// A new mail: hold its first load until the header has its final
  /// content, so the spacer is sized once instead of reloading the mail.
  void _watchHeader() {
    final ready = widget.headerReady;
    _shown = false;
    _headerReady = ready == null;
    ready
        ?.then<void>((_) {}, onError: (_) {})
        .timeout(const Duration(seconds: 1), onTimeout: () {})
        .then((_) {
          if (!mounted || widget.headerReady != ready) return;
          _headerReady = true;
          _requestSync();
        });
  }

  /// Before the first load, wait for the triggers to go quiet (the header
  /// rebuilds and is measured a frame after its content arrives); after
  /// it, apply changes right away.
  void _requestSync() {
    if (_shown) {
      _sync();
      return;
    }
    _settle?.cancel();
    _settle = Timer(const Duration(milliseconds: 50), () {
      if (mounted) _sync();
    });
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
    final width = _width;
    if (top == null || width == null || !_headerReady) return;
    if (!identical(_layoutFor, widget.html)) {
      _layoutFor = widget.html;
      _layoutWidth = mailLayoutWidth(widget.html);
    }
    // The page's own side margins (16px each) are not the mail's to use.
    // With [MailWebView.fitWidths] off the mail keeps its original fixed
    // widths and scrolls sideways, like the sender laid it out.
    final fit = widget.fitWidths && _layoutWidth > width - 32;
    final darkenedOn = widget.paint == MailPaint.darkened ? surface : null;
    final key = (
      widget.html,
      widget.allowRemote,
      (palette.paper, palette.ink, palette.link, palette.quote, palette.rule),
      darkenedOn,
      top.ceil(),
      fit,
    );
    _shown = true;
    if (key == _loaded) return;
    _loaded = key;
    _controller.loadHtmlString(
      mailDocument(
        widget.html,
        allowRemote: widget.allowRemote,
        palette: palette,
        darkenedOn: darkenedOn,
        topSpace: top,
        fit: fit,
      ),
    );
  }

  /// Queue a header drag distance, flushed as one `scrollBy` per frame.
  void _forwardDrag(double dy) {
    _stopFling();
    _pendingDy += dy;
    if (_dragScheduled) return;
    _dragScheduled = true;
    SchedulerBinding.instance.scheduleFrameCallback((_) {
      _dragScheduled = false;
      final dy = _pendingDy.truncate();
      _pendingDy -= dy;
      if (dy != 0 && mounted) _controller.scrollBy(0, dy);
    });
  }

  /// A short decaying fling from a header swipe's release velocity
  /// (physical pixels per millisecond). Native flings never reach the page
  /// because the overlay eats the gesture; without this a swipe starting
  /// on the header stops dead on release.
  void _flingFrom(double velocity) {
    _stopFling();
    if (!mounted || velocity.abs() < 0.5) return;
    var v = velocity;
    _fling = Timer.periodic(const Duration(milliseconds: 16), (t) {
      if (!mounted) {
        t.cancel();
        return;
      }
      v *= 0.92;
      if (v.abs() < 0.5) {
        t.cancel();
        _fling = null;
        return;
      }
      _controller.scrollBy(0, (v * 16).round());
    });
  }

  void _stopFling() {
    _fling?.cancel();
    _fling = null;
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        if (width != _width) {
          _width = width;
          // A rotation reloads only if it changes whether the mail fits.
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted) _requestSync();
          });
        }
        return _page(context);
      },
    );
  }

  Widget _page(BuildContext context) {
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
            // the header were part of it. Motion events are batched into
            // one `scrollBy` per frame; the release velocity becomes a
            // short decaying fling, so a swipe off the header keeps moving
            // like a native one instead of stopping dead.
            onVerticalDragUpdate: (d) => _forwardDrag(-d.delta.dy * dpr),
            onVerticalDragEnd: (d) =>
                _flingFrom(-(d.primaryVelocity ?? 0) * dpr / 1000),
            onVerticalDragCancel: _stopFling,
            child: RepaintBoundary(
              // Its own layer: scrolling only re-composites the header's
              // bitmap instead of repainting it on every frame.
              child: MeasureSize(
                onChange: (size) {
                  if (!mounted || size.height == _headerHeight) return;
                  // The spacer only matters at the top of the page, where a
                  // header change (details expanded) happens anyway; the
                  // reload that resizes it resets the scroll.
                  _headerHeight = size.height;
                  _requestSync();
                },
                child: Material(child: header),
              ),
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
/// With [darkenedOn] set, the page sits directly on that dark colour with
/// pre-inverted defaults and sender colours (see [darkenMailColors]) —
/// deliberately no runtime `filter`, which would re-render every scrolled
/// frame. Images keep their real colours without any double inversion.
///
/// Nothing from the mail can reach `<head>` — the sanitizer drops `head`,
/// `meta` and `style` — and a second CSP could only narrow this one anyway.
String mailDocument(
  String body, {
  required bool allowRemote,
  MailPalette palette = MailPalette.light,
  Color? darkenedOn,
  double topSpace = 0,
  bool fit = false,
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
  // A darkened page carries no runtime `filter` (see [darkenMailColors]):
  // it sits directly on the dark surface with pre-inverted defaults, so
  // scrolling repaints nothing through a filter. The inverted defaults
  // use the same matrix as the old filter, hence the same colours.
  final darkened = darkenedOn != null;
  final paper = darkened ? darkenedOn : palette.paper;
  final ink = darkened ? invertColor(palette.ink) : palette.ink;
  final link = darkened ? invertColor(palette.link) : palette.link;
  final quote = darkened ? invertColor(palette.quote) : palette.quote;
  final rule = darkened ? invertColor(palette.rule) : palette.rule;
  final layout =
      'html,body{background:${cssHex(paper)}}'
      'body{margin:0 16px 16px;'
      'background:${cssHex(paper)};color:${cssHex(ink)};'
      'font-family:sans-serif;font-size:15px;line-height:1.5;'
      'overflow-wrap:break-word}'
      '#mc-top{margin-bottom:16px}';
  // Room for the header overlaying the top of the page (see [MailWebView]).
  final spacer = '<div id="mc-top" style="height:${topSpace.ceil()}px"></div>';
  // A layout wider than the page is loosened to fit it (see [fitMailWidths]).
  final shown = fit ? fitMailWidths(body) : body;
  final content = spacer + (darkened ? darkenMailColors(shown) : shown);
  return '<!DOCTYPE html><html><head><meta charset="utf-8">'
      '<meta http-equiv="Content-Security-Policy" '
      'content="${const HtmlEscape(HtmlEscapeMode.attribute).convert(csp)}">'
      // CSP does not cover DNS prefetching of link hosts; this does.
      '<meta http-equiv="x-dns-prefetch-control" content="off">'
      '<meta name="viewport" content="width=device-width, initial-scale=1">'
      '<style>$layout'
      'a{color:${cssHex(link)}}'
      // Fixed-width newsletter tables and images shrink to the screen
      // instead of scrolling sideways; author CSS beats their attributes.
      'img{max-width:100%!important;height:auto!important}'
      'table{max-width:100%!important}'
      // A long URL in a cell would otherwise set the cell's minimum width.
      'td,th{overflow-wrap:anywhere}'
      // Loosened widths are caps; padding must fit inside them.
      '${fit ? 'div,table{box-sizing:border-box}' : ''}'
      'pre{white-space:pre-wrap}'
      'blockquote{margin:8px 0;padding-left:12px;'
      'border-left:3px solid ${cssHex(rule)};'
      'color:${cssHex(quote)}}'
      '</style></head><body>$content</body></html>';
}
