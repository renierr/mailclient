import 'package:flutter/material.dart';

import '../../ffi/mail_core.dart';

/// How a reader paints one HTML body: the theme's colours, the sender's, or
/// the sender's rewritten for a dark theme. Decided by the core
/// (`mailcore::html::reader`), the same rules as the Qt reader.
typedef MailPaint = ReaderPaint;

/// Pick the paint for one mail. [colored] is the core's `html_colored`;
/// [keepOriginal] is the reader's per-message toggle.
MailPaint mailPaintFor({
  required bool colored,
  required bool dark,
  required bool keepOriginal,
}) => MailCore.instance.readerPaint(colored, dark, keepOriginal);

/// Colours a page is written in, as the core picks them for a paint.
class MailPalette {
  const MailPalette({
    required this.paper,
    required this.ink,
    required this.link,
    required this.quote,
    required this.rule,
  });

  final Color paper;
  final Color ink;
  final Color link;
  final Color quote;
  final Color rule;

  /// The theme colours the core builds a page from: background, text,
  /// accent, muted text, border.
  static ReaderPalette themeOf(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ReaderPalette(
      paper: _rgb(scheme.surface),
      ink: _rgb(scheme.onSurface),
      link: _rgb(scheme.primary),
      quote: _rgb(scheme.onSurfaceVariant),
      rule: _rgb(scheme.outlineVariant),
    );
  }

  factory MailPalette.of(BuildContext context, MailPaint paint) {
    final p = MailCore.instance.readerPalette(paint, themeOf(context));
    return MailPalette(
      paper: _color(p.paper),
      ink: _color(p.ink),
      link: _color(p.link),
      quote: _color(p.quote),
      rule: _color(p.rule),
    );
  }

  static int _rgb(Color c) => c.toARGB32() & 0xFFFFFF;

  static Color _color(int rgb) => Color(0xFF000000 | rgb);
}

/// `#rrggbb` for CSS.
String cssHex(Color c) =>
    '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';
