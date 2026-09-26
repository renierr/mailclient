import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/link_safety.dart';

void main() {
  group('LinkSafety', () {
    test('web schemes pass', () {
      expect(LinkSafety.isWebScheme('https://example.com/x'), isTrue);
      expect(LinkSafety.isWebScheme('http://example.com/'), isTrue);
      expect(LinkSafety.isWebScheme('mailto:a@example.com'), isTrue);
      expect(LinkSafety.isWebScheme('  HTTPS://example.com  '), isTrue);
    });

    test('evil schemes fail closed', () {
      expect(LinkSafety.isWebScheme('javascript:alert(1)'), isFalse);
      expect(LinkSafety.isWebScheme('JaVaScRiPt:alert(1)'), isFalse);
      expect(LinkSafety.isWebScheme('java\tscript:alert(1)'), isFalse);
      expect(LinkSafety.isWebScheme('data:text/html,<p>x</p>'), isFalse);
      expect(LinkSafety.isWebScheme('file:///etc/passwd'), isFalse);
      expect(LinkSafety.isWebScheme('vbscript:msgbox(1)'), isFalse);
      expect(LinkSafety.isWebScheme('ftp://example.com/x'), isFalse);
      expect(LinkSafety.isWebScheme(''), isFalse);
      expect(LinkSafety.isWebScheme(null), isFalse);
      expect(LinkSafety.isWebScheme('#fragment'), isFalse);
    });

    test('action normalizes to examine', () {
      expect(LinkSafety.actionFor('browser'), 'browser');
      expect(LinkSafety.actionFor('examine'), 'examine');
      expect(LinkSafety.actionFor(''), 'examine');
      expect(LinkSafety.actionFor('weird'), 'examine');
      expect(LinkSafety.actionFor(null), 'examine');
    });

    test('display parsing', () {
      const u = 'https://user@example.com:443/a/b?x=1';
      expect(LinkSafety.schemeOf(u), 'https');
      expect(LinkSafety.hostOf(u), 'example.com');
      expect(LinkSafety.pathOf(u), '/a/b?x=1');
      expect(LinkSafety.schemeOf('mailto:a@example.com'), 'mailto');
      expect(LinkSafety.hostOf('https://example.com'), 'example.com');
      expect(LinkSafety.pathOf('https://example.com'), '—');
      expect(LinkSafety.schemeOf(''), '—');
      expect(LinkSafety.hostOf(''), '—');
      expect(LinkSafety.pathOf(''), '—');
      expect(LinkSafety.hostOf(null), '—');
    });
  });
}
