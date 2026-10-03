import 'package:flutter/material.dart';

/// Offers to download embedded images that are not stored locally.
///
/// Only older mail needs it: sync now keeps inline image bytes. Opening a
/// mail never goes online by itself, so the download waits for this tap —
/// and it goes to the user's own mail server, not to the sender.
class InlineImagesBanner extends StatelessWidget {
  const InlineImagesBanner({
    super.key,
    required this.count,
    required this.busy,
    required this.onDownload,
    this.passThrough = false,
  });

  final int count;
  final bool busy;
  final VoidCallback onDownload;

  /// The banner overlays a WebView that owns the scroll (Android): the
  /// label lets touches fall through to the page underneath, so drags and
  /// flings starting on it stay native. The button stays tappable.
  final bool passThrough;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final content = Wrap(
      crossAxisAlignment: WrapCrossAlignment.center,
      spacing: 8,
      runSpacing: 4,
      children: [
        _Label(count: count, passThrough: passThrough),
        TextButton(
          onPressed: busy ? null : onDownload,
          child: Text(busy ? 'Downloading…' : 'Download'),
        ),
      ],
    );
    const padding = EdgeInsets.symmetric(horizontal: 16, vertical: 8);
    if (!passThrough) {
      return Container(
        color: scheme.surfaceContainerHighest,
        padding: padding,
        child: content,
      );
    }
    // The background paints under an IgnorePointer so it never claims a
    // touch; the label passes through the same way, the button stays live.
    return Stack(
      children: [
        Positioned.fill(
          child: IgnorePointer(
            child: Container(color: scheme.surfaceContainerHighest),
          ),
        ),
        Padding(padding: padding, child: content),
      ],
    );
  }
}

/// The banner's display text: an icon plus one line. Never handles a
/// touch when the banner passes through — semantics stay on.
class _Label extends StatelessWidget {
  const _Label({required this.count, required this.passThrough});

  final int count;
  final bool passThrough;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final label = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(Icons.image_outlined, size: 18, color: scheme.outline),
        ConstrainedBox(
          constraints: BoxConstraints(
            maxWidth: MediaQuery.sizeOf(context).width - 48,
          ),
          child: Text(
            count == 1
                ? '1 embedded image is not downloaded yet.'
                : '$count embedded images are not downloaded yet.',
          ),
        ),
      ],
    );
    if (!passThrough) return label;
    return IgnorePointer(child: label);
  }
}
