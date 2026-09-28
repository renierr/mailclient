import 'package:flutter/material.dart';

/// How a reader paints one HTML body.
enum MailPaint {
  /// The mail sets no colours: app theme colours, like a plain-text mail.
  theme,

  /// The sender's colours on the light sheet they were designed for.
  original,

  /// The designed mail, inverted to match a dark theme. Images are inverted
  /// back, so photos and logos keep their real colours.
  darkened,
}

/// Pick the paint for one mail. [colored] is the core's `html_colored`;
/// [keepOriginal] is the reader's per-message toggle.
MailPaint mailPaintFor({
  required bool colored,
  required bool dark,
  required bool keepOriginal,
}) {
  if (!colored) return MailPaint.theme;
  return dark && !keepOriginal ? MailPaint.darkened : MailPaint.original;
}

/// Colours a document is written with. For [MailPaint.darkened] these are
/// the *pre-inversion* values: the whole body goes through [darkInvert].
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

  /// The light sheet designed mail expects.
  static const light = MailPalette(
    paper: Color(0xFFFFFFFF),
    ink: Color(0xFF202124),
    link: Color(0xFF1A5FD0),
    quote: Color(0xFF5F6368),
    rule: Color(0xFFD0D4DA),
  );

  factory MailPalette.of(BuildContext context, MailPaint paint) {
    final scheme = Theme.of(context).colorScheme;
    return switch (paint) {
      MailPaint.theme => MailPalette(
        paper: scheme.surface,
        ink: scheme.onSurface,
        link: scheme.primary,
        quote: scheme.onSurfaceVariant,
        rule: scheme.outlineVariant,
      ),
      MailPaint.original => light,
      // Inverted, the sheet lands exactly on the theme's surface.
      MailPaint.darkened => MailPalette(
        paper: invertColor(scheme.surface),
        ink: light.ink,
        link: light.link,
        quote: light.quote,
        rule: light.rule,
      ),
    };
  }
}

/// `invert(1) hue-rotate(180deg)` as one colour matrix: lightness flips,
/// hues stay (red text stays red). Applying it twice gives the original
/// back, which is how images are restored inside an inverted body.
const darkInvert = ColorFilter.matrix(<double>[
  0.574, -1.430, -0.144, 0, 255, //
  -0.426, -0.430, -0.144, 0, 255, //
  -0.426, -1.430, 0.856, 0, 255, //
  0, 0, 0, 1, 0, //
]);

/// The same as CSS, for the WebView and Qt documents.
const darkInvertCss = 'invert(1) hue-rotate(180deg)';

/// [darkInvert] applied to one colour.
Color invertColor(Color c) {
  double ch(double v) => v.clamp(0.0, 1.0);
  final r = c.r, g = c.g, b = c.b;
  return Color.from(
    alpha: c.a,
    red: ch(1 - (-0.574 * r + 1.430 * g + 0.144 * b)),
    green: ch(1 - (0.426 * r + 0.430 * g + 0.144 * b)),
    blue: ch(1 - (0.426 * r + 1.430 * g - 0.856 * b)),
  );
}

/// `#rrggbb` for CSS.
String cssHex(Color c) =>
    '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';
