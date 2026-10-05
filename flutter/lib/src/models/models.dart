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

/// A sender's avatar badge, built once by `mailcore::badge` for both
/// frontends: the letters plus a background colour per theme (`#rrggbb`),
/// with white text on either.
class SenderBadge {
  const SenderBadge({
    required this.initials,
    required this.light,
    required this.dark,
  });

  /// What a row from an older core, without badge fields, shows.
  static const none = SenderBadge(initials: '?', light: '', dark: '');

  final String initials;
  final String light;
  final String dark;

  factory SenderBadge.fromJson(Map<String, dynamic> j) => SenderBadge(
    initials: _str(j['initials'], orElse: '?'),
    light: _str(j['avatar_light']),
    dark: _str(j['avatar_dark']),
  );
}

/// A configured mail account.
class Account {
  const Account({
    required this.id,
    required this.name,
    required this.email,
    required this.fromName,
    required this.imapHost,
    required this.smtpHost,
    this.badge = SenderBadge.none,
  });

  final int id;
  final String name;
  final String email;

  /// Sender display name for `From:` (`""` = address only). New in the feed;
  /// older cores omit it and it degrades to the account name.
  final String fromName;
  final String imapHost;
  final String smtpHost;

  /// The account's own avatar, as others see it as a sender.
  final SenderBadge badge;

  factory Account.fromJson(Map<String, dynamic> j) => Account(
    id: _int(j['id']),
    name: _str(j['name']),
    email: _str(j['email']),
    fromName: _str(j['from_name']),
    imapHost: _str(j['imap_host']),
    smtpHost: _str(j['smtp_host']),
    badge: SenderBadge.fromJson(j),
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
    this.deleteIsPermanent = true,
    required this.depth,
    required this.leafName,
  });

  final int id;

  /// The full IMAP path, e.g. `Work/Client`.
  final String path;
  final FolderRole role;
  final int unread;
  final int total;

  /// Whether the sidebar shows it. Display-only: an unsubscribed folder keeps
  /// its cache and still syncs.
  final bool subscribed;

  /// The server's hierarchy separator, usually `/` or `.`.
  final String delimiter;

  /// Deleting here destroys instead of moving to Trash (the core's
  /// `undo::delete_is_permanent`).
  final bool deleteIsPermanent;

  /// Subfolder nesting from the feed (`mailcore::feed::folder_depth`).
  final int depth;

  /// Short name from the feed (`mailcore::feed::folder_leaf`).
  final String leafName;

  factory Folder.fromJson(Map<String, dynamic> j) {
    final path = _str(j['name']);
    final delimiter = _str(j['delimiter'], orElse: '/');
    return Folder(
      id: _int(j['id']),
      path: path,
      role: FolderRole.parse(_str(j['role'])),
      unread: _int(j['unread']),
      total: _int(j['count']),
      subscribed: _bool(j['subscribed'], orElse: true),
      delimiter: delimiter,
      deleteIsPermanent: _bool(j['delete_is_permanent'], orElse: true),
      // A payload without the fields still indents like the core would.
      depth:
          _optInt(j['depth']) ??
          (delimiter.isEmpty ? 0 : path.split(delimiter).length - 1),
      leafName:
          _optStr(j['leaf']) ??
          (delimiter.isEmpty ? path : path.split(delimiter).last),
    );
  }
}

/// One row of the message list. Deliberately without a body: opening a folder
/// must not sanitize two hundred bodies.
class MessageSummary {
  const MessageSummary({
    required this.uid,
    required this.subject,
    required this.from,
    required this.fromName,
    required this.date,
    this.dateKey = '',
    this.dateRaw = '',
    required this.snippet,
    required this.unread,
    required this.starred,
    required this.hasAttachments,
    this.badge = SenderBadge.none,
  });

  final int uid;
  final String subject;
  final String from;
  final SenderBadge badge;

  /// Sender display name from the core (`""` = address only).
  final String fromName;

  /// What the list row shows: the sent name, or the address when the mail
  /// carries no name.
  String get senderName => fromName.isNotEmpty ? fromName : from;

  /// Preformatted by the core, which knows the user's locale rules for
  /// "today" and "yesterday" better than a list item does.
  final String date;

  /// Names the date cases that are a word rather than a number
  /// (`mailcore::feed`: `"yesterday"`, else `""`) — the word itself is the
  /// UI's to supply, as in Qt.
  final String dateKey;

  /// Raw UTC timestamp (`mailcore::feed`: `date_raw`) for the list date
  /// quick-filter; [date] above is display text.
  final String dateRaw;

  /// What the row shows: the localized word for a named [dateKey], else
  /// the core's text.
  String get displayDate => displayMailDate(date, dateKey);

  final String snippet;
  final bool unread;
  final bool starred;
  final bool hasAttachments;

  factory MessageSummary.fromJson(Map<String, dynamic> j) => MessageSummary(
    uid: _int(j['uid']),
    subject: _str(j['subject'], orElse: '(no subject)'),
    from: _str(j['from'], orElse: '?'),
    fromName: _str(j['from_name']),
    date: _str(j['date']),
    dateKey: _str(j['date_key']),
    dateRaw: _str(j['date_raw']),
    snippet: _str(j['snippet']),
    unread: _bool(j['unread']),
    starred: _bool(j['starred']),
    hasAttachments: _bool(j['has_attachments']),
    badge: SenderBadge.fromJson(j),
  );

  MessageSummary copyWith({bool? unread, bool? starred}) => MessageSummary(
    uid: uid,
    subject: subject,
    from: from,
    fromName: fromName,
    date: date,
    dateKey: dateKey,
    dateRaw: dateRaw,
    snippet: snippet,
    unread: unread ?? this.unread,
    starred: starred ?? this.starred,
    hasAttachments: hasAttachments,
    badge: badge,
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
    this.dateKey = '',
    required this.bodyText,
    required this.bodyHtml,
    required this.isHtml,
    required this.hasRemoteImages,
    this.missingInlineImages = 0,
    this.htmlColored = false,
    required this.attachments,
    this.badge = SenderBadge.none,
    this.fromName = '',
    this.replyTarget = '',
    this.replyToDiffers = false,
    this.event,
  });

  final int uid;
  final String subject;
  final String from;
  final SenderBadge badge;

  /// Sender display name from the core (`""` = address only).
  final String fromName;

  /// What the reader shows as the sender: the name, else the address.
  String get senderName => fromName.isNotEmpty ? fromName : from;
  final String to;
  final String cc;

  /// Set when replies should go somewhere other than [from]. The reader shows
  /// it, because answering the wrong address is not recoverable.
  final String replyTo;

  /// Where a reply goes (Reply-To, else From) and whether that is not the
  /// sender — decided by `mailcore::compose::reply_address`, as in Qt.
  final String replyTarget;
  final bool replyToDiffers;
  final String date;

  /// Names the date cases that are a word rather than a number
  /// (`mailcore::feed`: `"yesterday"`, else `""`).
  final String dateKey;

  /// What the reader shows: the localized word for a named [dateKey], else
  /// the core's text.
  String get displayDate => displayMailDate(date, dateKey);

  final String bodyText;

  /// Already sanitized by `mailcore`, with remote images stripped unless the
  /// user allowed them. Never render raw mail HTML.
  final String bodyHtml;
  final bool isHtml;

  /// Whether sanitizing actually removed remote references — what the
  /// "images were blocked" banner is about.
  final bool hasRemoteImages;

  /// Embedded (`cid:`) images whose bytes are not stored locally, so the
  /// body shows their alt text. Mail synced before inline images were kept.
  final int missingInlineImages;

  /// The HTML sets its own text or background colours (a designed mail),
  /// so the reader keeps or darkens them instead of applying the theme.
  final bool htmlColored;
  final List<AttachmentInfo> attachments;

  /// Parsed calendar event invitation metadata for reader preview card.
  final CalendarEventInfo? event;

  factory MessageBody.fromJson(Map<String, dynamic> j) => MessageBody(
    uid: _int(j['uid']),
    subject: _str(j['subject'], orElse: '(no subject)'),
    from: _str(j['from']),
    to: _str(j['to']),
    cc: _str(j['cc']),
    replyTo: _str(j['reply_to']),
    date: _str(j['date']),
    dateKey: _str(j['date_key']),
    bodyText: _str(j['body_text']),
    bodyHtml: _str(j['body_html']),
    isHtml: _bool(j['is_html']),
    hasRemoteImages: _bool(j['has_remote_images']),
    missingInlineImages: _int(j['missing_inline_images']),
    htmlColored: _bool(j['html_colored']),
    attachments: _list(j['attachments'])
        .map(AttachmentInfo.fromJson)
        .toList(growable: false),
    badge: SenderBadge.fromJson(j),
    fromName: _str(j['from_name']),
    replyTarget: _str(j['reply_target']),
    replyToDiffers: _bool(j['reply_to_differs']),
    event: j['event'] is Map<String, dynamic>
        ? CalendarEventInfo.fromJson(j['event'] as Map<String, dynamic>)
        : null,
  );
}

/// Parsed calendar event for the reader preview card
/// (`mailcore::calendar::CalendarEvent`).
class CalendarEventInfo {
  const CalendarEventInfo({
    required this.summary,
    this.location,
    this.organizer,
    required this.formattedTime,
    this.isCancelled = false,
    this.attachmentId,
    this.saveName,
  });

  final String summary;
  final String? location;
  final String? organizer;
  final String formattedTime;
  final bool isCancelled;
  final int? attachmentId;

  /// Filesystem-safe name for the `.ics` attachment (`mailcore::paths`);
  /// never build one from [summary].
  final String? saveName;

  factory CalendarEventInfo.fromJson(Map<String, dynamic> j) =>
      CalendarEventInfo(
        summary: _str(j['summary'], orElse: '(Event)'),
        location: j['location'] as String?,
        organizer: j['organizer'] as String?,
        formattedTime: _str(j['formatted_time']),
        isCancelled: _bool(j['is_cancelled']),
        attachmentId: j['attachment_id'] == null
            ? null
            : _int(j['attachment_id']),
        saveName: j['save_name'] as String?,
      );
}

/// A new, reply or forward draft prepared by `mailcore::compose::answer`,
/// the same one the Qt composer fills its fields from.
class AnswerDraft {
  const AnswerDraft({
    this.to = '',
    this.cc = '',
    this.subject = '',
    this.noticeAddr = '',
    this.noticeSender = '',
    this.signatureText = '',
    this.quoteHtml = '',
    this.quoteFirst = false,
  });

  final String to;
  final String cc;
  final String subject;

  /// Set when the reply goes to a Reply-To other than the sender.
  final String noticeAddr;
  final String noticeSender;

  /// The `-- ` signature block, or `""` without a signature.
  final String signatureText;

  /// Attribution or forward header plus the quoted original, as HTML.
  final String quoteHtml;

  /// Bottom-posting: the quote goes above the user's text.
  final bool quoteFirst;

  factory AnswerDraft.fromJson(Map<String, dynamic> j) => AnswerDraft(
    to: _str(j['to']),
    cc: _str(j['cc']),
    subject: _str(j['subject']),
    noticeAddr: _str(j['notice_addr']),
    noticeSender: _str(j['notice_sender']),
    signatureText: _str(j['signature_text']),
    quoteHtml: _str(j['quote_html']),
    quoteFirst: _bool(j['quote_first']),
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
    this.fileName = '',
    this.sizeText = '',
    this.openMime = '*/*',
  });

  final int id;

  /// What to show: the mail's name, or the core's fallback without one.
  final String filename;

  /// The name a file written for it gets (`mailcore::paths`): what a save
  /// dialog suggests. Never build a path from [filename].
  final String fileName;
  final String mimeType;

  /// What the OS opener and save picker get (`open_mime`,
  /// `mailcore::mime::open_mime`): the stored type canonicalized, or the
  /// extension's when it is missing or generic; `*/*` when unknown.
  final String openMime;
  final int size;

  /// Preformatted byte count from the feed (`size_text`,
  /// `mailcore::maintenance::format_bytes`): what the bar shows.
  final String sizeText;

  /// Inline parts are the images the body already references by `cid:`; the
  /// attachment bar leaves them out.
  final bool isInline;

  factory AttachmentInfo.fromJson(Map<String, dynamic> j) => AttachmentInfo(
    id: _int(j['id']),
    filename: _str(
      j['display_name'],
      orElse: _str(j['filename'], orElse: 'attachment-${_int(j['id'])}.bin'),
    ),
    fileName: _str(j['file_name'], orElse: 'attachment-${_int(j['id'])}.bin'),
    mimeType: _str(j['mime_type'], orElse: 'application/octet-stream'),
    openMime: _str(j['open_mime'], orElse: '*/*'),
    size: _int(j['size']),
    sizeText: _str(j['size_text']),
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
    this.sentCount = 0,
  });

  final String address;

  /// The name as it was transferred in the mail.
  final String name;

  /// A name the user set themselves, which wins over [name].
  final String alias;
  final int timesSeen;

  /// How often mail was sent *to* this address (vs. merely harvested from
  /// incoming mail). Missing on payloads from older cores.
  final int sentCount;

  factory Contact.fromJson(Map<String, dynamic> j) => Contact(
    address: _str(j['address']),
    name: _str(j['name']),
    alias: _str(j['alias']),
    timesSeen: _int(j['times_seen']),
    sentCount: _int(j['sent_count']),
  );

  String get displayName =>
      alias.isNotEmpty ? alias : (name.isNotEmpty ? name : address);
}

/// One row of the contacts cleanup review
/// (`mailcore::store::contacts::cleanup_candidates`): a contact plus
/// machine-readable `reasons` (`"automated"`, `"stale"`).
class CleanupCandidate {
  const CleanupCandidate({required this.contact, required this.reasons});

  final Contact contact;
  final List<String> reasons;

  factory CleanupCandidate.fromJson(Map<String, dynamic> j) => CleanupCandidate(
    contact: Contact.fromJson(j['contact'] as Map<String, dynamic>),
    reasons: (j['reasons'] as List? ?? const [])
        .map((r) => r.toString())
        .toList(),
  );

  /// One-line, human-readable explanation of why this row was suggested.
  String get reasonText {
    final parts = <String>[];
    if (reasons.contains('automated')) {
      parts.add('looks like an automated sender');
    }
    if (reasons.contains('stale')) {
      parts.add('seen only once, long ago');
    }
    for (final r in reasons) {
      if (r != 'automated' && r != 'stale') parts.add(r);
    }
    return parts.join('; ');
  }
}

/// One unsent mail (`mailcore::outbox::list_json`): everything the outbox
/// dialog shows, never the MIME bytes.
class OutboxEntry {
  const OutboxEntry({
    required this.id,
    required this.status,
    this.state = '',
    required this.lastError,
    required this.retries,
    required this.retryable,
    required this.dismissable,
    required this.hasBytes,
    required this.envelopeFrom,
    required this.envelopeTo,
    required this.subject,
    required this.createdAt,
  });

  final int id;
  final String status;

  /// The row's one-line state, phrased once by the core so both frontends
  /// show the same words (like the undo toast labels).
  final String state;
  final String lastError;
  final int retries;
  final bool retryable;

  /// Whether ✕ may forget the row (never one being sent right now).
  final bool dismissable;
  final bool hasBytes;
  final String envelopeFrom;
  final List<String> envelopeTo;
  final String subject;
  final String createdAt;

  factory OutboxEntry.fromJson(Map<String, dynamic> j) => OutboxEntry(
    id: _int(j['id']),
    status: _str(j['status']),
    state: _str(j['state']),
    lastError: _str(j['last_error']),
    retries: _int(j['retries']),
    retryable: _bool(j['retryable']),
    dismissable: _bool(j['dismissable']),
    hasBytes: _bool(j['has_bytes']),
    envelopeFrom: _str(j['envelope_from']),
    envelopeTo: switch (j['envelope_to']) {
      List<dynamic> items => items.map((e) => e.toString()).toList(),
      _ => const [],
    },
    subject: _str(j['subject']),
    createdAt: _str(j['created_at']),
  );
}

/// Outbox counts for one account (`mailcore::outbox::status`).
class OutboxStatus {
  const OutboxStatus({
    required this.queued,
    required this.sending,
    required this.failed,
    required this.retryable,
    required this.pending,
    this.label = '',
    this.hasFailures = false,
  });

  static const empty = OutboxStatus(
    queued: 0,
    sending: 0,
    failed: 0,
    retryable: 0,
    pending: 0,
  );

  final int queued;
  final int sending;
  final int failed;
  final int retryable;
  final int pending;

  /// The pill's words (`2 unsent (1 failed)`), phrased by mailcore.
  final String label;

  /// Danger styling for the pill.
  final bool hasFailures;

  bool get any => pending > 0;

  factory OutboxStatus.fromJson(Map<String, dynamic> j) => OutboxStatus(
    queued: _int(j['queued']),
    sending: _int(j['sending']),
    failed: _int(j['failed']),
    retryable: _int(j['retryable']),
    pending: _int(j['pending']),
    label: _str(j['label']),
    hasFailures: _bool(j['has_failures']),
  );
}

// --- decoding helpers ------------------------------------------------------
//
// A feed field that is missing, null or the wrong type must not take down the
// whole list, so each of these falls back rather than throwing.

/// What a row shows for the core's date: the localized word for a named key
/// (`"yesterday"`), else the core's text — the same decision Qt's list and
/// reader make from `date_key`.
String displayMailDate(String date, String dateKey) =>
    dateKey == 'yesterday' ? 'Yesterday' : date;

int _int(Object? v) => switch (v) {
  int n => n,
  num n => n.toInt(),
  String s => int.tryParse(s) ?? 0,
  _ => 0,
};

/// Null when the field is missing rather than a number, so callers can tell
/// "absent" apart from a real zero.
int? _optInt(Object? v) => switch (v) {
  int n => n,
  num n => n.toInt(),
  String s => int.tryParse(s),
  _ => null,
};

/// Null when the field is missing or empty, so callers can fall back.
String? _optStr(Object? v) => switch (v) {
  String s when s.isNotEmpty => s,
  _ => null,
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
