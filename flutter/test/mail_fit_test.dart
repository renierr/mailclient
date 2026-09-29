import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/mail_fit.dart';
import 'package:mailclient/src/ui/reader/mail_web_view.dart';

void main() {
  group('mailLayoutWidth', () {
    test('takes the widest fixed width of tables, cells and blocks', () {
      const html =
          '<table width="600"><tr><td width="200">a</td>'
          '<td style="color:#333;width:640px;">b</td></tr></table>'
          '<div style="min-width:700px;">c</div>';
      expect(mailLayoutWidth(html), 700);
    });

    test('ignores percentages and images', () {
      const html =
          '<table width="100%"><tr><td style="width:50%;">'
          '<img src="data:image/png;base64,Zm9v" width="900"></td></tr></table>';
      expect(mailLayoutWidth(html), 0);
    });
  });

  group('fitMailWidths', () {
    test('cells lose their pixel widths, percentages stay', () {
      expect(
        fitMailWidths('<td width="300" style="width:300px;padding:4px;">'),
        '<td style=";padding:4px;">',
      );
      expect(fitMailWidths('<td width="50%">'), '<td width="50%">');
    });

    test('tables and blocks keep their width as a cap', () {
      expect(
        fitMailWidths('<table width="600" align="center">'),
        '<table align="center" style="width:100%;max-width:600px;">',
      );
      expect(
        fitMailWidths('<div style="color:red;width:600px;">'),
        '<div style="color:red;width:100%;max-width:600px;">',
      );
    });

    test('the element style still beats the attribute cap', () {
      expect(
        fitMailWidths('<table width="600" style="width:80%;">'),
        '<table style="width:100%;max-width:600px;width:80%;">',
      );
    });

    test('drops pixel min-widths, leaves max-width and images alone', () {
      expect(
        fitMailWidths('<div style="min-width:600px;max-width:600px;">'),
        '<div style=";max-width:600px;">',
      );
      const img = '<img src="data:image/png;base64,Zm9v" width="600">';
      expect(fitMailWidths(img), img);
    });

    test('text and attribute values are not touched', () {
      const html =
          '<p>width="600" and width:600px</p>'
          '<a href="https://example.com/?w=600" rel="noopener">x</a>';
      expect(fitMailWidths(html), html);
    });
  });

  group('mailDocument fit', () {
    const body = '<table width="600"><tr><td width="600">x</td></tr></table>';

    test('loosens the layout only when asked', () {
      final plain = mailDocument(body, allowRemote: false);
      expect(plain, contains(body));
      expect(plain, isNot(contains('box-sizing')));
      final fitted = mailDocument(body, allowRemote: false, fit: true);
      expect(fitted, contains('<td>x</td>'));
      expect(fitted, contains('div,table{box-sizing:border-box}'));
    });

    test('long words in cells may break', () {
      expect(
        mailDocument(body, allowRemote: false),
        contains('td,th{overflow-wrap:anywhere}'),
      );
    });

    test('the original layout keeps fixed widths (original-colours toggle)', () {
      final doc = mailDocument(body, allowRemote: false, fit: false);
      expect(doc, contains(body));
      expect(doc, isNot(contains('max-width:600px')));
      expect(doc, isNot(contains('box-sizing')));
    });
  });
}
