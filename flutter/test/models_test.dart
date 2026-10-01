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
}
