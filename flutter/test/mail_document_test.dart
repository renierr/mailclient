import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/mail_web_view.dart';

String csp(String doc) =>
    RegExp(r'Content-Security-Policy" content="([^"]*)"').firstMatch(doc)![1]!;

void main() {
  group('mailDocument', () {
    test('blocks every network load by default', () {
      final policy = csp(mailDocument('<p>hi</p>', allowRemote: false));
      expect(policy, contains("default-src 'none'"));
      expect(policy, contains('img-src data:;'));
      expect(policy, isNot(contains('https:')));
      expect(policy, contains("font-src 'none'"));
    });

    test('lets remote images through only when allowed', () {
      final policy = csp(mailDocument('<p>hi</p>', allowRemote: true));
      expect(policy, contains('img-src data: https: http:;'));
      expect(policy, contains("default-src 'none'"));
    });

    test('the policy comes before the body', () {
      final doc = mailDocument('<p>marker</p>', allowRemote: false);
      expect(
        doc.indexOf('Content-Security-Policy'),
        lessThan(doc.indexOf('marker')),
      );
      expect(doc, contains('<p>marker</p></body>'));
      expect(doc, contains('x-dns-prefetch-control" content="off"'));
    });

    test('a spacer reserves room for the header overlay', () {
      final doc = mailDocument(
        '<p>marker</p>',
        allowRemote: false,
        topSpace: 120.4,
      );
      expect(
        doc,
        contains('<body><div id="mc-top" style="height:121px"></div><p>'),
      );
    });
  });
}
