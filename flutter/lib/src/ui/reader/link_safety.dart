import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../ffi/mail_core.dart';
import '../dialogs/mail_dialog.dart';

/// Links in the reader. Whether one may leave the app at all, and how the
/// examine dialog splits it, is the core's (`html::link_info`, the
/// sanitizer's own rule); only opening it is the toolkit's.
abstract final class LinkSafety {
  static Future<void> openUrl(String url) async {
    final uri = Uri.tryParse(url.trim());
    if (uri != null) {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    }
  }
}

/// Dialog for inspecting suspicious or external URLs before navigating.
class ExamineLinkDialog extends StatelessWidget {
  const ExamineLinkDialog({super.key, required this.url});

  final String url;

  static Future<void> show(BuildContext context, String url) {
    return MailDialog.show(
      context,
      builder: (_) => ExamineLinkDialog(url: url),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final info = MailCore.instance.linkInfo(url);
    String shown(String part) => part.isEmpty ? '—' : part;
    final scheme = shown(info.scheme);
    final host = shown(info.host);
    final path = shown(info.path);

    return AlertDialog(
      title: const Text('Examine link'),
      content: SizedBox(
        // Never a fixed 480px: clamps to phones instead of overflowing.
        width: MailDialog.maxWidth(context, 480),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Address',
                style: theme.textTheme.labelMedium?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
              const SizedBox(height: 4),
              SelectableText(
                url,
                style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              ),
              const SizedBox(height: 12),
              Text(
                'Scheme',
                style: theme.textTheme.labelMedium?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
              const SizedBox(height: 2),
              SelectableText(scheme),
              const SizedBox(height: 12),
              Text(
                'Domain',
                style: theme.textTheme.labelMedium?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
              const SizedBox(height: 2),
              SelectableText(host),
              const SizedBox(height: 12),
              Text(
                'Path',
                style: theme.textTheme.labelMedium?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
              const SizedBox(height: 2),
              SelectableText(path),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () {
            Clipboard.setData(ClipboardData(text: url));
            ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(content: Text('Link copied to clipboard')),
            );
          },
          child: const Text('Copy'),
        ),
        FilledButton(
          onPressed: () async {
            Navigator.of(context).pop();
            await LinkSafety.openUrl(url);
          },
          child: const Text('Open in browser'),
        ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }
}
