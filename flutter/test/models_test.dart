import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/models/models.dart';

/// These cover the one thing the Dart side can get wrong on its own: decoding
/// what `mailcore` sends. Everything with real behaviour lives in Rust and is
/// tested there, so there is nothing here worth mocking the FFI for.
void main() {
  group('Folder', () {
    test('derives hierarchy from the path and the server delimiter', () {
      final f = Folder.fromJson(
        jsonDecode('''
        {"id": 3, "name": "Work.Client.2024", "role": "custom", "unread": 2,
         "count": 40, "subscribed": true, "delimiter": "."}
      ''') as Map<String, dynamic>,
      );

      expect(f.depth, 2);
      expect(f.leafName, '2024');
    });

    test('prefers the feed depth and leaf over deriving them', () {
      final f = Folder.fromJson(
        jsonDecode('''
        {"id": 4, "name": "Work/Client", "role": "custom", "unread": 0,
         "count": 3, "subscribed": true, "delimiter": "/",
         "depth": 1, "leaf": "Client"}
      ''') as Map<String, dynamic>,
      );

      expect(f.depth, 1);
      expect(f.leafName, 'Client');
    });

    test('an unknown role degrades to custom instead of throwing', () {
      final f = Folder.fromJson(
        jsonDecode('{"id": 1, "name": "X", "role": "templates"}')
            as Map<String, dynamic>,
      );

      expect(f.role, FolderRole.custom);
      // A feed that omits it should still render, and a folder nobody hid is
      // a folder the sidebar shows.
      expect(f.subscribed, isTrue);
      expect(f.delimiter, '/');
    });
  });

  group('Account', () {
    test('reads the sender display name the composer prefills', () {
      final a = Account.fromJson(
        jsonDecode('''
        {"id": 1, "name": "Work", "email": "me@example.com",
         "from_name": "Me Myself", "imap_host": "imap.x", "smtp_host": "smtp.x"}
      ''') as Map<String, dynamic>,
      );

      expect(a.fromName, 'Me Myself');
    });

    test('a missing sender name degrades to empty, not to a throw', () {
      final a = Account.fromJson(
        jsonDecode('{"id": 1, "name": "", "email": "me@example.com"}')
            as Map<String, dynamic>,
      );

      expect(a.fromName, '');
      expect(a.displayName, 'me@example.com');
    });
  });

  group('MessageSummary', () {
    test('fills in the placeholders the list would otherwise show blank', () {
      final m = MessageSummary.fromJson(
        jsonDecode('{"uid": 7}') as Map<String, dynamic>,
      );

      expect(m.subject, '(no subject)');
      expect(m.from, '?');
      expect(m.unread, isFalse);
    });

    test('accepts the numeric booleans SQLite-backed feeds produce', () {
      final m = MessageSummary.fromJson(
        jsonDecode(
          '{"uid": 7, "unread": 1, "starred": 0, "has_attachments": true}',
        ) as Map<String, dynamic>,
      );

      expect(m.unread, isTrue);
      expect(m.starred, isFalse);
      expect(m.hasAttachments, isTrue);
    });

    test('prefers the sent display name, falling back to the address', () {
      final named = MessageSummary.fromJson(
        jsonDecode(
          '{"uid": 7, "from": "juergen@example.com", "from_name": "Jürgen Müller"}',
        ) as Map<String, dynamic>,
      );
      expect(named.senderName, 'Jürgen Müller');

      final unnamed = MessageSummary.fromJson(
        jsonDecode('{"uid": 8, "from": "plain@example.com"}')
            as Map<String, dynamic>,
      );
      expect(unnamed.fromName, '');
      expect(unnamed.senderName, 'plain@example.com');
    });

    test('a named date key shows the word, not the fallback text', () {
      final m = MessageSummary.fromJson(
        jsonDecode('{"uid": 7, "date": "Yesterday", "date_key": "yesterday"}')
            as Map<String, dynamic>,
      );
      expect(m.displayDate, 'Yesterday');

      final plain = MessageSummary.fromJson(
        jsonDecode('{"uid": 8, "date": "12:30"}') as Map<String, dynamic>,
      );
      expect(plain.displayDate, '12:30');
    });
  });

  group('MessageBody', () {
    test('reads the reader payload including its attachment list', () {
      final m = MessageBody.fromJson(
        jsonDecode('''
        {"uid": 12, "subject": "Hi", "from": "a@x.de", "to": "b@x.de",
         "body_html": "<p>hi</p>", "is_html": true, "has_remote_images": true,
         "missing_inline_images": 2,
         "attachments": [
           {"id": 1, "filename": "a.pdf", "mime_type": "application/pdf",
            "size": 2048, "is_inline": false},
           {"id": 2, "filename": "logo.png", "is_inline": true}
         ]}
      ''') as Map<String, dynamic>,
      );

      expect(m.hasRemoteImages, isTrue);
      expect(m.missingInlineImages, 2);
      expect(m.attachments, hasLength(2));
      expect(m.attachments.where((a) => !a.isInline).single.filename, 'a.pdf');
      // An attachment without a declared type is still openable; the default
      // keeps the bar from rendering an empty subtitle.
      expect(m.attachments.last.mimeType, 'application/octet-stream');
    });

    test('an older core without the inline count means none missing', () {
      final m = MessageBody.fromJson(
        jsonDecode('{"uid": 1, "is_html": true}') as Map<String, dynamic>,
      );
      expect(m.missingInlineImages, 0);
    });
  });

  group('Contact', () {
    test('a user-set alias wins over the name the mail carried', () {
      final c = Contact.fromJson(
        jsonDecode(
          '{"address": "a@x.de", "name": "A. Nonymous", "alias": "Alex"}',
        ) as Map<String, dynamic>,
      );

      expect(c.displayName, 'Alex');
    });

    test('falls back to the address when nothing names the contact', () {
      final c = Contact.fromJson(
        jsonDecode('{"address": "a@x.de"}') as Map<String, dynamic>,
      );

      expect(c.displayName, 'a@x.de');
    });
  });

  group('SenderBadge', () {
    test('rows carry the core badge', () {
      final m = MessageSummary.fromJson({
        'uid': 1,
        'from': 'alice@example.com',
        'initials': 'AE',
        'avatar_light': '#aa3366',
        'avatar_dark': '#882255',
      });
      expect(m.badge.initials, 'AE');
      expect(m.badge.light, '#aa3366');
      expect(m.badge.dark, '#882255');
      expect(m.copyWith(unread: true).badge.initials, 'AE');
    });

    test('a row from an older core degrades to a placeholder', () {
      final m = MessageSummary.fromJson({'uid': 1, 'from': 'a@example.com'});
      expect(m.badge.initials, '?');
      expect(m.badge.dark, '');
    });
  });

  test('reader payload carries the parsed sender and reply decision', () {
    final b = MessageBody.fromJson({
      'uid': 1,
      'from': 'alice@example.com',
      'from_name': 'Alice',
      'reply_to': 'list@example.org',
      'reply_target': 'list@example.org',
      'reply_to_differs': true,
    });
    expect(b.senderName, 'Alice');
    expect(b.replyTarget, 'list@example.org');
    expect(b.replyToDiffers, isTrue);
    expect(
      MessageBody.fromJson({'from': 'a@example.com'}).senderName,
      'a@example.com',
    );
  });

  test('answer drafts parse the core fields', () {
    final d = AnswerDraft.fromJson({
      'to': 'list@example.org',
      'cc': 'bob@example.com',
      'subject': 'Re: Plans',
      'notice_addr': 'list@example.org',
      'notice_sender': 'alice@example.com',
      'signature_text': '-- \nMe',
      'quote_html': '<blockquote>x</blockquote>',
      'quote_first': true,
    });
    expect(d.to, 'list@example.org');
    expect(d.noticeSender, 'alice@example.com');
    expect(d.signatureText, '-- \nMe');
    expect(d.quoteFirst, isTrue);
    expect(AnswerDraft.fromJson({}).quoteHtml, '');
  });

  test('attachment names come from the core', () {
    final a = AttachmentInfo.fromJson({
      'id': 7,
      'filename': '../con',
      'display_name': '../con',
      'file_name': '_con',
    });
    expect(a.filename, '../con');
    expect(a.fileName, '_con');
    // An older core without the fields still never yields an empty name.
    final old = AttachmentInfo.fromJson({'id': 3});
    expect(old.filename, 'attachment-3.bin');
    expect(old.fileName, 'attachment-3.bin');
    expect(old.sizeText, '');
  });

  test('attachment sizes come preformatted from the core', () {
    final a = AttachmentInfo.fromJson({
      'id': 7,
      'size': 2048,
      'size_text': '2.0 KB',
    });
    expect(a.sizeText, '2.0 KB');
  });

  group('Outbox', () {
    test('rows decode with recipients, error and retry state', () {
      final e = OutboxEntry.fromJson({
        'id': 4,
        'status': 'failed',
        'state': 'Failed — will retry on the next sync',
        'last_error': 'connection refused',
        'retries': 1,
        'retryable': true,
        'has_bytes': true,
        'envelope_from': 'me@example.com',
        'envelope_to': ['you@example.com'],
        'subject': 'hello',
        'created_at': '2026-01-01T00:00:00Z',
      });

      expect(e.subject, 'hello');
      expect(e.envelopeTo, ['you@example.com']);
      expect(e.state, 'Failed — will retry on the next sync');
    });

    test('a row without a state still renders', () {
      final e = OutboxEntry.fromJson({'id': 5, 'status': 'failed'});

      expect(e.subject, '');
      expect(e.retryable, isFalse);
      expect(e.state, '');
    });

    test('counts decode and empty means no pill', () {
      const empty = OutboxStatus.empty;
      expect(empty.any, isFalse);

      final s = OutboxStatus.fromJson({
        'queued': 1,
        'sending': 0,
        'failed': 2,
        'retryable': 1,
        'pending': 3,
      });
      expect(s.any, isTrue);
      expect(s.pending, 3);
    });
  });
}
