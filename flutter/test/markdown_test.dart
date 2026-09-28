import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/composer/markdown.dart';

void main() {
  group('MarkdownMail.hasFormatting', () {
    test('plain prose has none', () {
      expect(MarkdownMail.hasFormatting('Hello,\n\nsee attached.\n'), isFalse);
      expect(MarkdownMail.hasFormatting('2*3=6 and a * b'), isFalse);
    });

    test('toolbar syntax counts', () {
      expect(MarkdownMail.hasFormatting('a **bold** word'), isTrue);
      expect(MarkdownMail.hasFormatting('an *italic* word'), isTrue);
      expect(MarkdownMail.hasFormatting('> quoted'), isTrue);
      expect(MarkdownMail.hasFormatting('- bullet'), isTrue);
      expect(MarkdownMail.hasFormatting('[docs](https://example.com)'), isTrue);
    });
  });

  group('MarkdownMail.toHtml', () {
    test('bold and italic render inline', () {
      expect(
        MarkdownMail.toHtml('a **bold** and *italic* word'),
        '<p>a <b>bold</b> and <i>italic</i> word</p>',
      );
    });

    test('quotes group into one blockquote', () {
      expect(
        MarkdownMail.toHtml('Hi\n> line one\n> line two\nBye'),
        '<p>Hi</p><blockquote>line one<br>line two</blockquote><p>Bye</p>',
      );
    });

    test('bullets group into a list', () {
      expect(
        MarkdownMail.toHtml('- one\n- two'),
        '<ul><li>one</li><li>two</li></ul>',
      );
    });

    test('links allow web schemes only', () {
      expect(
        MarkdownMail.toHtml('[a](https://example.com/x)'),
        '<p><a href="https://example.com/x">a</a></p>',
      );
      expect(MarkdownMail.toHtml('[a](javascript:alert(1))'), '<p>a</p>');
    });

    test('raw html is escaped, not passed through', () {
      final html = MarkdownMail.toHtml('<script>alert(1)</script>');
      expect(html, isNot(contains('<script>')));
      expect(html, contains('&lt;script&gt;'));
    });

    test(
      'inline image tokens render their data URL, unknown ones their name',
      () {
        const url = 'data:image/png;base64,Zm9v';
        const text = 'see ![logo.png](inline:1) and ![gone](inline:2)';
        expect(MarkdownMail.hasFormatting(text), isTrue);
        expect(
          MarkdownMail.toHtml(text, images: {1: url}),
          '<p>see <img alt="logo.png" src="$url"> and gone</p>',
        );
      },
    );
  });
}
