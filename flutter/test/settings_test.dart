import 'package:flutter_test/flutter_test.dart';
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
      expect(s.readerScale, 1.2);
      expect(s.syncIntervalMinutes, 15);
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
}
