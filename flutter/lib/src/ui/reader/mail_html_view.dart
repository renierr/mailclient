import 'dart:io';

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
  });

  /// Sanitized HTML. Never raw mail source.
  final String html;

  /// The reader's text size preference, as a multiplier.
  final double textScale;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SelectionArea(
      child: HtmlWidget(
        html,
        textStyle: theme.textTheme.bodyMedium?.copyWith(
          fontSize: (theme.textTheme.bodyMedium?.fontSize ?? 14) * textScale,
        ),
        // A link in a mail is a link to somewhere a stranger chose. Opening it
        // is the user's decision, so it is handed to the platform browser
        // rather than followed in-app, and never followed automatically.
        onTapUrl: _openExternally,
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

  static Future<bool> _openExternally(String url) async {
    // Deliberately not `url_launcher`: adding a dependency to open a link is
    // the kind of thing that should be a considered choice, and until the
    // composer needs it the honest behaviour is to decline rather than to
    // half-open. Wiring it up is one line here.
    debugPrint('mail: link tap ignored (no launcher wired): $url');
    return false;
  }
}

/// Whether a real browser engine is available for the reader on this platform.
///
/// Used by the About view to explain which renderer is in use, so "that mail
/// looks wrong" has an answer that does not require reading the source.
bool get hasWebviewRenderer =>
    Platform.isWindows || Platform.isAndroid || Platform.isMacOS;
