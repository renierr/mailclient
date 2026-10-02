import 'package:flutter/material.dart';

/// Jump to the top / bottom of a long list: a small pair floating over its
/// bottom-right corner. Each button shows only while that end is out of
/// view, and neither until the list is a couple of screens long. Taps, not
/// hover, so it works the same on a phone.
///
/// Owns the [ScrollController] and hands it to [builder]; the list keeps
/// its own `PageStorageKey`, which the controller still restores.
class ScrollJumpOverlay extends StatefulWidget {
  const ScrollJumpOverlay({super.key, required this.builder});

  final Widget Function(ScrollController controller) builder;

  @override
  State<ScrollJumpOverlay> createState() => _ScrollJumpOverlayState();
}

class _ScrollJumpOverlayState extends State<ScrollJumpOverlay> {
  final _controller = ScrollController();
  bool _showTop = false;
  bool _showBottom = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  bool _onNotification(Notification n) {
    final ScrollMetrics m;
    if (n is ScrollNotification && n.depth == 0) {
      m = n.metrics;
    } else if (n is ScrollMetricsNotification && n.depth == 0) {
      m = n.metrics;
    } else {
      return false;
    }
    final long = m.maxScrollExtent > m.viewportDimension;
    final top = long && m.pixels > m.minScrollExtent;
    final bottom = long && m.pixels < m.maxScrollExtent;
    if (top != _showTop || bottom != _showBottom) {
      setState(() {
        _showTop = top;
        _showBottom = bottom;
      });
    }
    return false;
  }

  void _toTop() {
    if (_controller.hasClients) _controller.jumpTo(0);
  }

  /// A lazy list only estimates its extent from the rows built so far:
  /// jump, let the frame lay out the new rows, and repeat until the end
  /// holds still.
  Future<void> _toBottom() async {
    for (var i = 0; i < 8; i++) {
      if (!mounted || !_controller.hasClients) return;
      final p = _controller.position;
      if (p.pixels >= p.maxScrollExtent) return;
      _controller.jumpTo(p.maxScrollExtent);
      await WidgetsBinding.instance.endOfFrame;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        Positioned.fill(
          child: NotificationListener<Notification>(
            onNotification: _onNotification,
            child: widget.builder(_controller),
          ),
        ),
        Positioned(
          right: 12,
          bottom: 12,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (_showTop)
                IconButton.filledTonal(
                  tooltip: 'Jump to top',
                  icon: const Icon(Icons.vertical_align_top),
                  onPressed: _toTop,
                ),
              if (_showBottom)
                IconButton.filledTonal(
                  tooltip: 'Jump to bottom',
                  icon: const Icon(Icons.vertical_align_bottom),
                  onPressed: _toBottom,
                ),
            ],
          ),
        ),
      ],
    );
  }
}
