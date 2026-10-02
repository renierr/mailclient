import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';

import '../../ffi/mail_core.dart';
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
///
/// The WebView is a real Android view (Hybrid Composition), not a texture
/// Flutter composites: as a texture every scrolled WebView frame is copied
/// into Flutter's next frame, and on a high-refresh phone that hand-over
/// drops and doubles frames — flings judder however light the page is.
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

  /// Narrow pages loosen fixed-width newsletter layouts to fit (the core's
  /// `html::reader::fit_widths`). False keeps the mail's original fixed-width layout,
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

class _MailWebViewState extends State<MailWebView>
    with SingleTickerProviderStateMixin {
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

  /// Page width below which [MailWebView.html] is fitted (the core's
  /// `fit_below`), computed once per mail.
  String? _layoutFor;
  int _fitBelow = 0;

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

  /// Page scroll in physical pixels as last reported, advanced by our own
  /// `scrollBy`s: a forwarded header drag must not scroll above the top,
  /// which the WebView does not stop.
  double _pagePx = 0;

  /// Drag distance on the header overlay, flushed to the page as one
  /// `scrollBy` per frame instead of one platform-channel call per motion
  /// event.
  double _pendingDy = 0;
  bool _dragScheduled = false;

  /// Header-started fling, stepped once per vsync along Android's own
  /// fling curve, so it moves at the display rate like a native one.
  late final Ticker _fling = createTicker(_flingTick);
  Simulation? _flingSim;
  double _flingDone = 0;

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
      _pagePx = change.y;
      if (change.y < 0) {
        _stopFling();
        _controller.scrollTo(0, 0);
      }
      final y =
          (change.y < 0 ? 0.0 : change.y) /
          (_dpr ?? MediaQuery.devicePixelRatioOf(context));
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
    _fling.dispose();
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
    final theme = MailPalette.themeOf(context);
    final palette = MailPalette.of(context, widget.paint);
    // Behind the document, so nothing flashes white before it paints.
    if (palette.paper != _background) {
      _background = palette.paper;
      _controller.setBackgroundColor(palette.paper);
    }
    final top = widget.header == null ? 0.0 : _headerHeight;
    final width = _width;
    if (top == null || width == null || !_headerReady) return;
    final core = MailCore.instance;
    if (!identical(_layoutFor, widget.html)) {
      _layoutFor = widget.html;
      _fitBelow = core.readerFitBelow(widget.html);
    }
    // With [MailWebView.fitWidths] off the mail keeps its original fixed
    // widths and scrolls sideways, like the sender laid it out.
    final fit = widget.fitWidths && _fitBelow > 0 && width < _fitBelow;
    final key = (
      widget.html,
      widget.allowRemote,
      widget.paint,
      (theme.paper, theme.ink, theme.link, theme.quote, theme.rule),
      top.ceil(),
      fit,
    );
    _shown = true;
    if (key == _loaded) return;
    _loaded = key;
    _pagePx = 0;
    _controller.loadHtmlString(
      core.readerDocument(
        widget.html,
        ReaderDocumentOptions(
          paint: widget.paint,
          theme: theme,
          allowRemote: widget.allowRemote,
          topSpace: top.ceil(),
          // Text size goes through the WebView's own text zoom above.
          scale: 1,
          fit: fit,
        ),
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
      if (mounted && _scrollPage(dy) != dy) _pendingDy = 0;
    });
  }

  /// Scroll the page by [dy] physical pixels, never above its top; returns
  /// the distance actually scrolled.
  int _scrollPage(int dy) {
    final applied = max(dy, -_pagePx.floor());
    if (applied != 0) {
      _pagePx += applied;
      _controller.scrollBy(0, applied);
    }
    return applied;
  }

  /// A fling from a header swipe's release velocity (physical pixels per
  /// second). Native flings never reach the page because the overlay eats
  /// the gesture; without this a swipe starting on the header stops dead
  /// on release.
  void _flingFrom(double velocity) {
    _stopFling();
    if (!mounted || velocity.abs() < 50) return;
    _flingSim = ClampingScrollSimulation(position: 0, velocity: velocity);
    _flingDone = 0;
    _fling.start();
  }

  void _flingTick(Duration elapsed) {
    final sim = _flingSim;
    if (sim == null || !mounted) return;
    final t = elapsed.inMicroseconds / Duration.microsecondsPerSecond;
    final dy = (sim.x(t) - _flingDone).truncate();
    if (dy != 0) {
      _flingDone += dy;
      if (_scrollPage(dy) != dy) return _stopFling();
    }
    if (sim.isDone(t)) _stopFling();
  }

  void _stopFling() {
    if (_fling.isActive) _fling.stop();
    _flingSim = null;
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
    final web = WebViewWidget.fromPlatformCreationParams(
      params: AndroidWebViewWidgetCreationParams(
        controller: _controller.platform,
        displayWithHybridComposition: true,
      ),
    );
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
            // fling on Android's curve, so a swipe off the header keeps
            // moving like a native one instead of stopping dead.
            onVerticalDragUpdate: (d) => _forwardDrag(-d.delta.dy * dpr),
            onVerticalDragEnd: (d) =>
                _flingFrom(-(d.primaryVelocity ?? 0) * dpr),
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
