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
  });

  final int count;
  final bool busy;
  final VoidCallback onDownload;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      color: scheme.surfaceContainerHighest,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Wrap(
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: 8,
        runSpacing: 4,
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
          TextButton(
            onPressed: busy ? null : onDownload,
            child: Text(busy ? 'Downloading…' : 'Download'),
          ),
        ],
      ),
    );
  }
}
