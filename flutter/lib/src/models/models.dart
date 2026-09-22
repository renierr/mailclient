/// Dart views of the JSON that `mailcore` produces.
///
/// The Rust side already serialises accounts, folders, message rows and reader
/// payloads — the Qt frontend consumes exactly the same strings. Rather than
/// mirror every field through the FFI type system a second time, the JSON
/// crosses as a string and is decoded here, so there is one definition of what
/// a folder row contains and it lives in `mailcore::feed`.
///
/// Every constructor is defensive about missing and mistyped fields: a feed
/// written by a newer core must render, not throw, so an unknown shape
/// degrades to a sensible default instead of taking the list down.
library;

/// A configured mail account.
class Account {
  const Account({
    required this.id,
    required this.name,
    required this.email,
    required this.imapHost,
    required this.smtpHost,
  });

  final int id;
  final String name;
  final String email;
  final String imapHost;
  final String smtpHost;

  factory Account.fromJson(Map<String, dynamic> j) => Account(
        id: _int(j['id']),
        name: _str(j['name']),
        email: _str(j['email']),
        imapHost: _str(j['imap_host']),
        smtpHost: _str(j['smtp_host']),
      );

  /// What to show when an account has no name of its own.
  String get displayName => name.isNotEmpty ? name : email;
}

/// What a folder is for, as IMAP SPECIAL-USE reported it.
///
/// The role drives more than an icon: Trash suppresses unread counts, Junk is
/// deleted from rather than moved out of, and Drafts and Sent are where the
/// composer files things.
enum FolderRole {
  inbox,
  sent,
  drafts,
  trash,
  junk,
  archive,
  custom;

  static FolderRole parse(String raw) => FolderRole.values.firstWhere(
        (r) => r.name == raw,
        orElse: () => FolderRole.custom,
      );
}

/// One folder of one account.
class Folder {
  const Folder({
    required this.id,
    required this.path,
    required this.role,
    required this.unread,
    required this.total,
    required this.subscribed,
    required this.delimiter,
  });

  final int id;

  /// The full IMAP path, e.g. `Work/Client`. Hierarchy lives in the path, so
  /// [depth] and [leafName] derive from it rather than from a parent link.
  final String path;
  final FolderRole role;
  final int unread;
  final int total;

  /// Whether the sidebar shows it. Display-only: an unsubscribed folder keeps
  /// its cache and still syncs.
  final bool subscribed;

  /// The server's hierarchy separator, usually `/` or `.`.
  final String delimiter;

  factory Folder.fromJson(Map<String, dynamic> j) => Folder(
        id: _int(j['id']),
        path: _str(j['name']),
        role: FolderRole.parse(_str(j['role'])),
        unread: _int(j['unread']),
        total: _int(j['count']),
        subscribed: _bool(j['subscribed'], orElse: true),
        delimiter: _str(j['delimiter'], orElse: '/'),
      );

  int get depth => delimiter.isEmpty ? 0 : path.split(delimiter).length - 1;

  String get leafName =>
      delimiter.isEmpty ? path : path.split(delimiter).last;
}

/// One row of the message list. Deliberately without a body: opening a folder
/// must not sanitize two hundred bodies.
class MessageSummary {
  const MessageSummary({
    required this.uid,
    required this.subject,
    required this.from,
    required this.date,
    required this.snippet,
    required this.unread,
    required this.starred,
    required this.hasAttachments,
  });

  final int uid;
  final String subject;
  final String from;

  /// Preformatted by the core, which knows the user's locale rules for
  /// "today" and "yesterday" better than a list item does.
  final String date;
  final String snippet;
  final bool unread;
  final bool starred;
  final bool hasAttachments;

  factory MessageSummary.fromJson(Map<String, dynamic> j) => MessageSummary(
        uid: _int(j['uid']),
        subject: _str(j['subject'], orElse: '(no subject)'),
        from: _str(j['from'], orElse: '?'),
        date: _str(j['date']),
        snippet: _str(j['snippet']),
        unread: _bool(j['unread']),
        starred: _bool(j['starred']),
        hasAttachments: _bool(j['has_attachments']),
      );

  MessageSummary copyWith({bool? unread, bool? starred}) => MessageSummary(
        uid: uid,
        subject: subject,
        from: from,
        date: date,
        snippet: snippet,
        unread: unread ?? this.unread,
        starred: starred ?? this.starred,
        hasAttachments: hasAttachments,
      );
}

/// The full payload for the message the reader is showing.
class MessageBody {
  const MessageBody({
    required this.uid,
    required this.subject,
    required this.from,
    required this.to,
    required this.cc,
    required this.replyTo,
    required this.date,
    required this.bodyText,
    required this.bodyHtml,
    required this.isHtml,
    required this.hasRemoteImages,
    required this.attachments,
  });

  final int uid;
  final String subject;
  final String from;
  final String to;
  final String cc;

  /// Set when replies should go somewhere other than [from]. The reader shows
  /// it, because answering the wrong address is not recoverable.
  final String replyTo;
  final String date;
  final String bodyText;

  /// Already sanitized by `mailcore`, with remote images stripped unless the
  /// user allowed them. Never render raw mail HTML.
  final String bodyHtml;
  final bool isHtml;

  /// Whether sanitizing actually removed remote references — what the
  /// "images were blocked" banner is about.
  final bool hasRemoteImages;
  final List<AttachmentInfo> attachments;

  factory MessageBody.fromJson(Map<String, dynamic> j) => MessageBody(
        uid: _int(j['uid']),
        subject: _str(j['subject'], orElse: '(no subject)'),
        from: _str(j['from']),
        to: _str(j['to']),
        cc: _str(j['cc']),
        replyTo: _str(j['reply_to']),
        date: _str(j['date']),
        bodyText: _str(j['body_text']),
        bodyHtml: _str(j['body_html']),
        isHtml: _bool(j['is_html']),
        hasRemoteImages: _bool(j['has_remote_images']),
        attachments: _list(j['attachments'])
            .map(AttachmentInfo.fromJson)
            .toList(growable: false),
      );
}

/// An attachment's metadata. Bytes are never in a feed — they are fetched on
/// explicit request and cached in SQLite.
class AttachmentInfo {
  const AttachmentInfo({
    required this.id,
    required this.filename,
    required this.mimeType,
    required this.size,
    required this.isInline,
  });

  final int id;
  final String filename;
  final String mimeType;
  final int size;

  /// Inline parts are the images the body already references by `cid:`; the
  /// attachment bar leaves them out.
  final bool isInline;

  factory AttachmentInfo.fromJson(Map<String, dynamic> j) => AttachmentInfo(
        id: _int(j['id']),
        filename: _str(j['filename'], orElse: 'attachment'),
        mimeType: _str(j['mime_type'], orElse: 'application/octet-stream'),
        size: _int(j['size']),
        isInline: _bool(j['is_inline']),
      );
}

/// A known recipient, for composer autocomplete.
class Contact {
  const Contact({
    required this.address,
    required this.name,
    required this.alias,
    required this.timesSeen,
  });

  final String address;

  /// The name as it was transferred in the mail.
  final String name;

  /// A name the user set themselves, which wins over [name].
  final String alias;
  final int timesSeen;

  factory Contact.fromJson(Map<String, dynamic> j) => Contact(
        address: _str(j['address']),
        name: _str(j['name']),
        alias: _str(j['alias']),
        timesSeen: _int(j['times_seen']),
      );

  String get displayName =>
      alias.isNotEmpty ? alias : (name.isNotEmpty ? name : address);
}

// --- decoding helpers ------------------------------------------------------
//
// A feed field that is missing, null or the wrong type must not take down the
// whole list, so each of these falls back rather than throwing.

int _int(Object? v) => switch (v) {
      int n => n,
      num n => n.toInt(),
      String s => int.tryParse(s) ?? 0,
      _ => 0,
    };

String _str(Object? v, {String orElse = ''}) => switch (v) {
      String s when s.isNotEmpty => s,
      null || '' => orElse,
      _ => v.toString(),
    };

bool _bool(Object? v, {bool orElse = false}) => switch (v) {
      bool b => b,
      num n => n != 0,
      'true' || '1' => true,
      'false' || '0' => false,
      _ => orElse,
    };

List<Map<String, dynamic>> _list(Object? v) => switch (v) {
      List<dynamic> items => items.whereType<Map<String, dynamic>>().toList(),
      _ => const [],
    };
