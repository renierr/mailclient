import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/mail_dark.dart';
import 'package:mailclient/src/ui/reader/mail_paint.dart';

String inverted(String hex) {
  final rgb = int.parse(hex.substring(1), radix: 16);
  final c = Color.fromARGB(
    255,
    (rgb >> 16) & 0xFF,
    (rgb >> 8) & 0xFF,
    rgb & 0xFF,
  );
  return cssHex(invertColor(c));
}

void main() {
  group('darkenMailColors', () {
    test('inverts hex colours with the filter matrix', () {
      expect(
        darkenMailColors('<p style="color:#ffffff;">x</p>'),
        '<p style="color:${inverted('#ffffff')};">x</p>',
      );
      expect(
        darkenMailColors('<div style="background-color:#000000">x</div>'),
        '<div style="background-color:${inverted('#000000')}">x</div>',
      );
    });

    test('converts keywords, rgb() and attributes', () {
      expect(
        darkenMailColors('<p style="color:red;">x</p>'),
        '<p style="color:${inverted('#ff0000')};">x</p>',
      );
      expect(
        darkenMailColors('<td bgcolor="navy">x</td>'),
        '<td bgcolor="${inverted('#000080')}">x</td>',
      );
      expect(
        darkenMailColors('<font color="#123456">x</font>'),
        '<font color="${inverted('#123456')}">x</font>',
      );
    });

    test('keeps alpha and !important', () {
      final out = darkenMailColors(
        '<p style="color:rgba(255,255,255,0.5)!important;">x</p>',
      );
      expect(out, contains('rgba('));
      // 0.5 rounds through 8-bit alpha (128) and back.
      expect(out, contains('0.502)!important'));
      expect(out, isNot(contains('255,255,255')));
    });

    test('converts the colour inside border shorthands', () {
      expect(
        darkenMailColors(
          '<td style="border:1px solid #dddddd;">x</td>',
        ),
        '<td style="border:1px solid ${inverted('#dddddd')};">x</td>',
      );
    });

    test('leaves the rest of the mail alone', () {
      const html = '<table width="600">'
          '<tr><td style="padding:4px;width:600px;">'
          'width="600" and plain text'
          '<img src="data:image/png;base64,Zm9v" width="600">'
          '</td></tr></table>'
          '<div style="background:linear-gradient(#fff,#000);">x</div>'
          '<p style="color:transparent;">x</p>';
      final out = darkenMailColors(html);
      expect(out, contains('padding:4px'));
      expect(out, contains('width="600"'));
      expect(out, contains('width:600px'));
      expect(out, contains('and plain text'));
      expect(out, contains('<img src="data:image/png;base64,Zm9v" width="600">'));
      expect(out, contains('linear-gradient(#fff,#000)'));
      expect(out, contains('color:transparent'));
    });
  });
}
