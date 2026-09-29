import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/mail_paint.dart';
import 'package:mailclient/src/ui/reader/mail_web_view.dart';

void main() {
  group('mailPaintFor', () {
    test('a mail without colours always takes the theme', () {
      for (final dark in [true, false]) {
        expect(
          mailPaintFor(colored: false, dark: dark, keepOriginal: false),
          MailPaint.theme,
        );
      }
    });

    test('a designed mail darkens in a dark theme unless kept original', () {
      expect(
        mailPaintFor(colored: true, dark: true, keepOriginal: false),
        MailPaint.darkened,
      );
      expect(
        mailPaintFor(colored: true, dark: true, keepOriginal: true),
        MailPaint.original,
      );
      expect(
        mailPaintFor(colored: true, dark: false, keepOriginal: false),
        MailPaint.original,
      );
    });
  });

  group('invertColor', () {
    test('white turns black and greys mirror', () {
      final black = invertColor(const Color(0xFFFFFFFF));
      expect(black.r, closeTo(0, 0.01));
      expect(black.g, closeTo(0, 0.01));
      expect(black.b, closeTo(0, 0.01));
      final g = invertColor(const Color(0xFF333333));
      expect(g.r, closeTo(1 - 0x33 / 255, 0.01));
    });

    test('applied twice gives the colour back, so images restore', () {
      const c = Color(0xFF3B82F6);
      final back = invertColor(invertColor(c));
      expect(back.r, closeTo(c.r, 0.01));
      expect(back.g, closeTo(c.g, 0.01));
      expect(back.b, closeTo(c.b, 0.01));
    });
  });

  test('a darkened document needs no runtime filter', () {
    final doc = mailDocument(
      '<p style="color:#ffffff;">x</p>',
      allowRemote: false,
      darkenedOn: const Color(0xFF16181D),
    );
    // The page sits straight on the dark surface: no invert wrapper, no
    // filter anywhere, images untouched.
    expect(doc, isNot(contains('id="mail"')));
    expect(doc, isNot(contains('filter:')));
    expect(doc, contains('html,body{background:#16181d}'));
    // …with the sender's colours pre-inverted instead.
    expect(doc, isNot(contains('color:#ffffff')));
  });
}
