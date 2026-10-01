import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/models/account_settings.dart';
import 'package:mailclient/src/models/settings.dart';

void main() {
  group('AppSettings', () {
    test('decodes the core object with its real key names', () {
      final s = AppSettings.fromJson({
        'sent_copy_enabled': true,
        'load_remote_images': false,
        'compose_send_format': 'html',
        'compose_include_plain': true,
        'auto_mark_read': true,
        'mark_read_delay_secs': 5,
        'collect_sent_contacts': true,
        'confirm_delete': false,
        'list_density': 'compact',
        'reader_font_size': 'large',
        'link_click_action': 'browser',
        'sync_interval_minutes': 15,
        'signature_enabled': true,
        'signature_text': 'kind regards',
        'reply_below_quote': true,
        'request_mdn': false,
        'ui_scale': 1.25,
        'message_sort_field': 'from',
        'message_sort_desc': false,
      });
      expect(s.sendFormat, 'html');
      expect(s.markReadDelaySecs, 5);
      expect(s.confirmDelete, isFalse);
      expect(s.isCompact, isTrue);
      expect(s.readerScale, 18 / 14);
      expect(s.linkClickAction, 'browser');
      expect(s.syncIntervalMinutes, 15);
      expect(s.backgroundScheduler, 'workmanager');
      expect(s.uiScale, 1.25);
      expect(s.sortField, 'from');
      expect(s.sortDescending, isFalse);
    });

    test('missing fields fall back to the core defaults', () {
      final s = AppSettings.fromJson({});
      expect(s.sendFormat, 'auto');
      expect(s.autoMarkRead, isTrue);
      expect(s.confirmDelete, isTrue);
      expect(s.isCompact, isFalse);
      expect(s.readerScale, 1.0);
      expect(s.linkClickAction, 'examine');
      expect(s.backgroundScheduler, 'workmanager');
      expect(s.uiScale, 1.0);
      expect(s.sortDescending, isTrue);
    });

    test('numeric booleans decode like the SQLite-backed feeds produce', () {
      final s = AppSettings.fromJson({
        'sent_copy_enabled': 0,
        'auto_mark_read': 1,
      });
      expect(s.sentCopy, isFalse);
      expect(s.autoMarkRead, isTrue);
    });

    test('background scheduler sticks to the known values', () {
      expect(
        AppSettings.fromJson({'background_scheduler': 'alarm'})
            .backgroundScheduler,
        'alarm',
      );
      expect(
        AppSettings.fromJson({'background_scheduler': 'nonsense'})
            .backgroundScheduler,
        'workmanager',
      );
    });
  });

  group('SearchHit', () {
    test('decodes a rank-ordered FTS row', () {
      final h = SearchHit.fromJson({
        'uid': 42,
        'folder_id': 7,
        'folder': 'INBOX',
        'subject': 'hello',
        'from': 'a@example.com',
        'date': 'today',
        'snippet': '…match…',
        'unread': 1,
        'starred': 0,
        'has_attachments': 1,
      });
      expect(h.uid, 42);
      expect(h.folderId, 7);
      expect(h.folder, 'INBOX');
      expect(h.unread, isTrue);
      expect(h.starred, isFalse);
      expect(h.hasAttachments, isTrue);
    });
  });

  group('MessageHeaders', () {
    test('decodes the headers dialog payload', () {
      final h = MessageHeaders.fromJson({
        'from': 'a@example.com',
        'to': 'b@example.com',
        'cc': '',
        'date': '2026-09-22 10:00',
        'subject': 'hi',
        'message_id': '<1@x>',
        'reply_to': '',
        'raw': 'From: a@example.com\r\n',
      });
      expect(h.from, 'a@example.com');
      expect(h.messageId, '<1@x>');
      expect(h.raw, contains('From:'));
    });
  });

  group('AccountSettings', () {
    test('decodes overrides and effective values', () {
      final s = AccountSettings.fromJson({
        'overrides': {'sync_interval_minutes': '30'},
        'effective': {'sync_interval_minutes': '30', 'push_enabled': '0'},
      });
      expect(s.overrides, {'sync_interval_minutes': '30'});
      expect(s.syncIntervalMinutes, 30);
      expect(AccountSettings.empty.syncIntervalMinutes, 0);
    });

    test('decodes a frequent IDLE heartbeat', () {
      final s = AccountSettings.fromJson({
        'overrides': {},
        'effective': {},
        'frequent_heartbeat_secs': 120,
      });
      expect(s.frequentHeartbeatSecs, 120);
      expect(AccountSettings.empty.frequentHeartbeatSecs, isNull);
      expect(AccountSettings.describeGap(120), 'every 2 minutes');
      expect(AccountSettings.describeGap(45), 'every 45 seconds');
    });

    test('a plan runs something only when an account checks', () {
      final off = BackgroundPlan.fromJson({
        'push': false,
        'poll_minutes': 0,
        'poll_scheduler': 'workmanager',
      });
      expect(off.any, isFalse);
      final mixed = BackgroundPlan.fromJson({
        'push': true,
        'poll_minutes': 30,
        'poll_scheduler': 'alarm',
      });
      expect(mixed.any, isTrue);
      expect(mixed.pollMinutes, 30);
      expect(mixed.pollScheduler, 'alarm');
    });
  });
}
