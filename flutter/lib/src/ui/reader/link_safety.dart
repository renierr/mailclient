import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:url_launcher/url_launcher.dart';

import '../dialogs/mail_dialog.dart';

/// Pure link-safety helpers for the reader: which URLs may ever leave the
/// app, what a click does with one, and how the examine dialog displays it.
///
/// The allow-list shape mirrors `normalize_link_click` / `safe_href` in
/// mailcore: unknown input fails closed (inert text, examine-first).
abstract final class LinkSafety {
  /// Only these schemes ever leave the app (user-gated). Everything else
  /// (javascript:, data:, file:, …) is inert.
  static bool isWebScheme(String? url) {
    final s = (url ?? '').trim().toLowerCase();
    return s.startsWith('http://') ||
        s.startsWith('https://') ||
        s.startsWith('mailto:');
  }

  /// Normalized click action: anything but "browser" examines first.
  static String actionFor(String? linkClickAction) {
    return linkClickAction == 'browser' ? 'browser' : 'examine';
  }

  static String schemeOf(String? u) {
    final s = (u ?? '').trim();
    final scheme = s.indexOf('://');
    if (scheme > 0) {
      return s.substring(0, scheme).toLowerCase();
    }
    if (s.toLowerCase().startsWith('mailto:')) {
      return 'mailto';
    }
    return '—';
  }

  static String hostOf(String? u) {
    final s = (u ?? '').trim();
    final scheme = s.indexOf('://');
    var rest = scheme >= 0 ? s.substring(scheme + 3) : s;
    final end = rest.indexOf('/');
    var host = end >= 0 ? rest.substring(0, end) : rest;
    final at = host.lastIndexOf('@');
    if (at >= 0) {
      host = host.substring(at + 1);
    }
    final colon = host.indexOf(':');
    if (colon >= 0) {
      host = host.substring(0, colon);
    }
    return host.isEmpty ? '—' : host;
  }

  static String pathOf(String? u) {
    final s = (u ?? '').trim();
    final scheme = s.indexOf('://');
    final rest = scheme >= 0 ? s.substring(scheme + 3) : s;
    final slash = rest.indexOf('/');
    if (slash < 0) {
      return '—';
    }
    final p = rest.substring(slash);
    return p.isEmpty ? '—' : p;
  }

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
    final scheme = LinkSafety.schemeOf(url);
    final host = LinkSafety.hostOf(url);
    final path = LinkSafety.pathOf(url);

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
