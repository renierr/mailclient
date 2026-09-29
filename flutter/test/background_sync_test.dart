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

  group('notifyAction', () {
    NotifyAction act({
      bool hasNew = false,
      bool alertsOn = true,
      bool permitted = true,
      String? shown,
      String? wanted,
    }) => notifyAction(
      hasNew: hasNew,
      alertsOn: alertsOn,
      permitted: permitted,
      shown: shown,
      wanted: wanted,
    );

    test('new mail alerts, unless alerts are off or blocked', () {
      expect(act(hasNew: true, wanted: 'a'), NotifyAction.alert);
      expect(act(hasNew: true, shown: 'a', wanted: 'a'), NotifyAction.alert);
      expect(act(hasNew: true, alertsOn: false), NotifyAction.alertsOff);
      expect(act(hasNew: true, permitted: false), NotifyAction.blocked);
    });

    test('nothing new leaves a dismissed notification alone', () {
      expect(act(wanted: 'a'), NotifyAction.none);
      expect(act(), NotifyAction.none);
    });

    test('nothing new keeps a visible notification in step', () {
      expect(act(shown: 'a', wanted: 'a'), NotifyAction.none);
      expect(act(shown: 'a', wanted: 'b'), NotifyAction.update);
      expect(act(shown: 'a'), NotifyAction.clear);
    });
  });

  group('notificationText', () {
    Map<String, dynamic> mail(int uid, String from) => {
      'uid': uid,
      'from': from,
      'subject': 'subject $uid',
    };

    test('one mail names its sender and subject', () {
      final t = notificationText([mail(1, 'a@example.com')]);
      expect(t.title, 'a@example.com');
      expect(t.body, 'subject 1');
      expect(t.lines, isEmpty);
    });

    test('several mails count, newest first, capped at five lines', () {
      final t = notificationText([
        for (var i = 1; i <= 7; i++) mail(i, 's$i@example.com'),
      ]);
      expect(t.title, '7 new messages');
      expect(t.body, 's7@example.com — subject 7');
      expect(t.lines.length, 5);
      expect(t.lines.first, startsWith('s7@'));
      expect(t.summary, '+2 more');
    });

    test('the signature matches what the posted notification reads back', () {
      final t = notificationText([mail(1, 'a@example.com'), mail(2, 'b')]);
      expect(NotificationText.signatureOf(t.title, t.body), t.signature);
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
