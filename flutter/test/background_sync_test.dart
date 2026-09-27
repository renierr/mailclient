import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_sync.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  group('effectiveBackgroundMinutes', () {
    test('zero and negative disable the worker', () {
      expect(effectiveBackgroundMinutes(0), 0);
      expect(effectiveBackgroundMinutes(-5), 0);
    });

    test('sub-minimum intervals clamp to the OS floor', () {
      expect(effectiveBackgroundMinutes(1), androidMinIntervalMinutes);
      expect(effectiveBackgroundMinutes(5), 15);
      expect(effectiveBackgroundMinutes(14), 15);
    });

    test('intervals at or above the floor pass through', () {
      expect(effectiveBackgroundMinutes(15), 15);
      expect(effectiveBackgroundMinutes(30), 30);
      expect(effectiveBackgroundMinutes(60), 60);
    });
  });

  group('notification payload', () {
    test('round-trips account, folder and uid', () {
      final payload = encodeOpenPayload(accountId: 3, folderId: 12, uid: 456);
      final parsed = parseOpenPayload(payload);
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

  group('readNotifiedMark', () {
    test('a missing mark is never-notified', () async {
      SharedPreferences.setMockInitialValues({});
      final prefs = await SharedPreferences.getInstance();
      expect(readNotifiedMark(prefs, 1, 2), isNull);
    });

    test('reads a validity-scoped mark', () async {
      SharedPreferences.setMockInitialValues({'notified_uid_1_2': '42:9'});
      final prefs = await SharedPreferences.getInstance();
      expect(readNotifiedMark(prefs, 1, 2), (validity: 42, uid: 9));
    });

    test('keeps a legacy bare UID so already-alerted mail stays quiet', () async {
      SharedPreferences.setMockInitialValues({'notified_uid_1_2': 9});
      final prefs = await SharedPreferences.getInstance();
      expect(readNotifiedMark(prefs, 1, 2), (validity: 0, uid: 9));
    });
  });

  group('decodeBackgroundReport', () {
    test('decodes the Rust report shape', () {
      final report = decodeBackgroundReport('''
        {"skipped": false, "total_unread": 2, "errors": [],
         "new": [{"account_id": 1, "account_email": "a@x.y",
                  "folder_id": 4, "folder": "INBOX", "uid": 9,
                  "from": "bob", "subject": "hi", "date": "today"}]}
      ''');
      expect(report['skipped'], false);
      expect(report['total_unread'], 2);
      expect((report['new'] as List).length, 1);
    });
  });
}
