/// Typed views of the settings object, search hits and header details.
///
/// Key strings must match `mailcore::store::settings` exactly — the core
/// rejects unknown keys on write, so a typo here fails loudly rather than
/// persisting a setting that never applies.
library;

/// Key names, mirroring `mailcore::store::settings`.
abstract final class SettingKeys {
  static const sentCopy = 'sent_copy_enabled';
  static const loadRemoteImages = 'load_remote_images';
  static const sendFormat = 'compose_send_format';
  static const includePlain = 'compose_include_plain';
  static const autoMarkRead = 'auto_mark_read';
  static const markReadDelay = 'mark_read_delay_secs';
  static const collectContacts = 'collect_sent_contacts';
  static const confirmDelete = 'confirm_delete';
  static const listDensity = 'list_density';
  static const readerFontSize = 'reader_font_size';
  static const syncInterval = 'sync_interval_minutes';
  static const signatureEnabled = 'signature_enabled';
  static const signatureText = 'signature_text';
  static const replyBelowQuote = 'reply_below_quote';
  static const requestMdn = 'request_mdn';
  static const uiScale = 'ui_scale';
  static const sortField = 'message_sort_field';
  static const sortDescending = 'message_sort_desc';
}

/// Every user preference, normalized the way the core normalized it.
class AppSettings {
  const AppSettings({
    required this.sentCopy,
    required this.loadRemoteImages,
    required this.sendFormat,
    required this.includePlain,
    required this.autoMarkRead,
    required this.markReadDelaySecs,
    required this.collectContacts,
    required this.confirmDelete,
    required this.density,
    required this.readerFontSize,
    required this.syncIntervalMinutes,
    required this.signatureEnabled,
    required this.signatureText,
    required this.replyBelowQuote,
    required this.requestMdn,
    required this.uiScale,
    required this.sortField,
    required this.sortDescending,
  });

  final bool sentCopy;
  final bool loadRemoteImages;
  final String sendFormat;
  final bool includePlain;
  final bool autoMarkRead;
  final int markReadDelaySecs;
  final bool collectContacts;
  final bool confirmDelete;
  final String density;
  final String readerFontSize;
  final int syncIntervalMinutes;
  final bool signatureEnabled;
  final String signatureText;
  final bool replyBelowQuote;
  final bool requestMdn;
  final double uiScale;
  final String sortField;
  final bool sortDescending;

  /// The core's own defaults, for a settings row that was never written and
  /// for tests that never opened a database.
  static const defaults = AppSettings(
    sentCopy: true,
    loadRemoteImages: false,
    sendFormat: 'auto',
    includePlain: true,
    autoMarkRead: true,
    markReadDelaySecs: 0,
    collectContacts: true,
    confirmDelete: true,
    density: 'comfortable',
    readerFontSize: 'normal',
    syncIntervalMinutes: 0,
    signatureEnabled: false,
    signatureText: '',
    replyBelowQuote: false,
    requestMdn: false,
    uiScale: 1.0,
    sortField: 'date',
    sortDescending: true,
  );

  factory AppSettings.fromJson(Map<String, dynamic> j) => AppSettings(
        sentCopy: _flag(j[SettingKeys.sentCopy], orElse: true),
        loadRemoteImages: _flag(j[SettingKeys.loadRemoteImages]),
        sendFormat: _str(j[SettingKeys.sendFormat], orElse: 'auto'),
        includePlain: _flag(j[SettingKeys.includePlain], orElse: true),
        autoMarkRead: _flag(j[SettingKeys.autoMarkRead], orElse: true),
        markReadDelaySecs: _int(j[SettingKeys.markReadDelay]),
        collectContacts: _flag(j[SettingKeys.collectContacts], orElse: true),
        confirmDelete: _flag(j[SettingKeys.confirmDelete], orElse: true),
        density: _str(j[SettingKeys.listDensity], orElse: 'comfortable'),
        readerFontSize: _str(j[SettingKeys.readerFontSize], orElse: 'normal'),
        syncIntervalMinutes: _int(j[SettingKeys.syncInterval]),
        signatureEnabled: _flag(j[SettingKeys.signatureEnabled]),
        signatureText: _str(j[SettingKeys.signatureText]),
        replyBelowQuote: _flag(j[SettingKeys.replyBelowQuote]),
        requestMdn: _flag(j[SettingKeys.requestMdn]),
        uiScale: _dbl(j[SettingKeys.uiScale], orElse: 1.0),
        sortField: _str(j[SettingKeys.sortField], orElse: 'date'),
        sortDescending: _flag(j[SettingKeys.sortDescending], orElse: true),
      );

  AppSettings copyWith({
    bool? sentCopy,
    bool? loadRemoteImages,
    String? sendFormat,
    bool? includePlain,
    bool? autoMarkRead,
    int? markReadDelaySecs,
    bool? collectContacts,
    bool? confirmDelete,
    String? density,
    String? readerFontSize,
    int? syncIntervalMinutes,
    bool? signatureEnabled,
    String? signatureText,
    bool? replyBelowQuote,
    bool? requestMdn,
    double? uiScale,
    String? sortField,
    bool? sortDescending,
  }) =>
      AppSettings(
        sentCopy: sentCopy ?? this.sentCopy,
        loadRemoteImages: loadRemoteImages ?? this.loadRemoteImages,
        sendFormat: sendFormat ?? this.sendFormat,
        includePlain: includePlain ?? this.includePlain,
        autoMarkRead: autoMarkRead ?? this.autoMarkRead,
        markReadDelaySecs: markReadDelaySecs ?? this.markReadDelaySecs,
        collectContacts: collectContacts ?? this.collectContacts,
        confirmDelete: confirmDelete ?? this.confirmDelete,
        density: density ?? this.density,
        readerFontSize: readerFontSize ?? this.readerFontSize,
        syncIntervalMinutes: syncIntervalMinutes ?? this.syncIntervalMinutes,
        signatureEnabled: signatureEnabled ?? this.signatureEnabled,
        signatureText: signatureText ?? this.signatureText,
        replyBelowQuote: replyBelowQuote ?? this.replyBelowQuote,
        requestMdn: requestMdn ?? this.requestMdn,
        uiScale: uiScale ?? this.uiScale,
        sortField: sortField ?? this.sortField,
        sortDescending: sortDescending ?? this.sortDescending,
      );

  /// Plain-text size multiplier for the reader. HTML mail brings its own
  /// sizes; this only affects the plain-text view.
  double get readerScale => switch (readerFontSize) {
        'small' => 0.85,
        'large' => 1.2,
        _ => 1.0,
      };

  /// Compact rows drop the snippet line, like the Qt frontend's density.
  bool get isCompact => density == 'compact';
}

/// One account-wide FTS hit, in rank order.
class SearchHit {
  const SearchHit({
    required this.uid,
    required this.folderId,
    required this.folder,
    required this.subject,
    required this.from,
    required this.date,
    required this.snippet,
    required this.unread,
    required this.starred,
    required this.hasAttachments,
  });

  final int uid;
  final int folderId;
  final String folder;
  final String subject;
  final String from;
  final String date;
  final String snippet;
  final bool unread;
  final bool starred;
  final bool hasAttachments;

  factory SearchHit.fromJson(Map<String, dynamic> j) => SearchHit(
        uid: _int(j['uid']),
        folderId: _int(j['folder_id']),
        folder: _str(j['folder']),
        subject: _str(j['subject'], orElse: '(no subject)'),
        from: _str(j['from'], orElse: '?'),
        date: _str(j['date']),
        snippet: _str(j['snippet']),
        unread: _truthy(j['unread']),
        starred: _truthy(j['starred']),
        hasAttachments: _truthy(j['has_attachments']),
      );
}

/// The reader's "Headers" dialog payload.
class MessageHeaders {
  const MessageHeaders({
    required this.from,
    required this.to,
    required this.cc,
    required this.date,
    required this.subject,
    required this.messageId,
    required this.replyTo,
    required this.raw,
  });

  final String from;
  final String to;
  final String cc;
  final String date;
  final String subject;
  final String messageId;
  final String replyTo;
  final String raw;

  factory MessageHeaders.fromJson(Map<String, dynamic> j) => MessageHeaders(
        from: _str(j['from']),
        to: _str(j['to']),
        cc: _str(j['cc']),
        date: _str(j['date']),
        subject: _str(j['subject']),
        messageId: _str(j['message_id']),
        replyTo: _str(j['reply_to']),
        raw: _str(j['raw']),
      );
}

bool _flag(Object? v, {bool orElse = false}) => switch (v) {
      bool b => b,
      num n => n != 0,
      'true' || '1' => true,
      'false' || '0' => false,
      _ => orElse,
    };

bool _truthy(Object? v) => _flag(v);

int _int(Object? v) => switch (v) {
      int n => n,
      num n => n.toInt(),
      String s => int.tryParse(s) ?? 0,
      _ => 0,
    };

double _dbl(Object? v, {double orElse = 0}) => switch (v) {
      double d => d,
      num n => n.toDouble(),
      String s => double.tryParse(s) ?? orElse,
      _ => orElse,
    };

String _str(Object? v, {String orElse = ''}) => switch (v) {
      String s when s.isNotEmpty => s,
      null || '' => orElse,
      _ => v.toString(),
    };
