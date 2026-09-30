import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_sync.dart';

void main() {
  // What the notification carries is built in Rust
  // (`notify::open_payload`) and pinned by its tests; this is the Dart end.
  group('notification payload', () {
    test('parses account, folder and uid', () {
      final parsed = parseOpenPayload('mail:3:12:456');
      expect(parsed, isNotNull);
      expect(parsed!.accountId, 3);
      expect(parsed.folderId, 12);
      expect(parsed.uid, 456);
    });

    test('rejects anything it did not stamp', () {
      expect(parseOpenPayload(null), isNull);
      expect(parseOpenPayload(''), isNull);
      expect(parseOpenPayload('other:1:2:3'), isNull);
      expect(parseOpenPayload('mail:1:2'), isNull);
      expect(parseOpenPayload('mail:1:2:3:4'), isNull);
      expect(parseOpenPayload('mail:a:b:c'), isNull);
      expect(parseOpenPayload('mail:-1:2:3'), isNull);
    });
  });
}
