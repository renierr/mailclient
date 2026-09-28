import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_sync.dart';

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

  group('notifyDecision', () {
    Future<bool> yes() async => true;
    Future<bool> no() async => false;

    test('nothing new commits the baseline without asking', () async {
      var asked = false;
      final d = await notifyDecision(
        hasNew: false,
        alertsOn: true,
        permitted: () async => asked = true,
      );
      expect(d, NotifyDecision.commit);
      expect(asked, isFalse);
    });

    test('alerts off or not permitted commit without posting', () async {
      expect(
        await notifyDecision(hasNew: true, alertsOn: false, permitted: yes),
        NotifyDecision.commit,
      );
      expect(
        await notifyDecision(hasNew: true, alertsOn: true, permitted: no),
        NotifyDecision.commit,
      );
    });

    test('new mail with alerts on and permitted posts', () async {
      expect(
        await notifyDecision(hasNew: true, alertsOn: true, permitted: yes),
        NotifyDecision.post,
      );
    });
  });

  group('decodeBackgroundReport', () {
    test('decodes the Rust report shape', () {
      final report = decodeBackgroundReport('''
        {"skipped": false, "total_unread": 2, "errors": [],
         "new": [{"account_id": 1, "account_email": "user@example.com",
                  "folder_id": 4, "folder": "INBOX", "uid": 9,
                  "uid_validity": 7, "from": "sender@example.org",
                  "subject": "hi", "date": "today"}],
         "marks": [{"account_id": 1, "folder_id": 4,
                    "uid_validity": 7, "uid": 9}]}
      ''');
      expect(report['skipped'], false);
      expect(report['total_unread'], 2);
      expect((report['new'] as List).length, 1);
      expect((report['marks'] as List).single['uid'], 9);
    });
  });
}
