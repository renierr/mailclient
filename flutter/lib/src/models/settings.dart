/// Typed views of the settings object, search hits and header details.
///
/// Key strings must match `mailcore::store::settings` exactly — the core
/// rejects unknown keys on write, so a typo here fails loudly rather than
/// persisting a setting that never applies.
library;

import 'models.dart';

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
  static const linkClickAction = 'link_click_action';
  static const syncInterval = 'sync_interval_minutes';
  static const backgroundScheduler = 'background_scheduler';
  static const notificationsEnabled = 'notifications_enabled';

  /// Quiet hours: no background checks between [quietStart] and [quietEnd]
  /// (`HH:MM`, device local time). Accounts inherit all three.
  static const quietEnabled = 'quiet_hours_enabled';
  static const quietStart = 'quiet_hours_start';
  static const quietEnd = 'quiet_hours_end';
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
    required this.linkClickAction,
    required this.syncIntervalMinutes,
    required this.backgroundScheduler,
    required this.notificationsEnabled,
    required this.quietEnabled,
    required this.quietStart,
    required this.quietEnd,
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
  final String linkClickAction;
  final int syncIntervalMinutes;
  final String backgroundScheduler;
  final bool notificationsEnabled;
  final bool quietEnabled;
  final String quietStart;
  final String quietEnd;
  final bool signatureEnabled;
  final String signatureText;
  final bool replyBelowQuote;
  final bool requestMdn;
  final double uiScale;
  final String sortField;
  final bool sortDescending;

  /// Before the core's settings have loaded (the first frames of startup,
  /// and widget tests without a database). Neutral, not the defaults: those
  /// are the core's (`SettingChoices`), and the real values replace this
  /// before anything acts on it.
  static final placeholder = AppSettings.fromJson(const {});

  /// The core sends every key, normalized (`settings_json`); a missing one
  /// reads as off / empty / zero rather than as a default kept here.
  factory AppSettings.fromJson(Map<String, dynamic> j) => AppSettings(
    sentCopy: _flag(j[SettingKeys.sentCopy]),
    loadRemoteImages: _flag(j[SettingKeys.loadRemoteImages]),
    sendFormat: _str(j[SettingKeys.sendFormat]),
    includePlain: _flag(j[SettingKeys.includePlain]),
    autoMarkRead: _flag(j[SettingKeys.autoMarkRead]),
    markReadDelaySecs: _int(j[SettingKeys.markReadDelay]),
    collectContacts: _flag(j[SettingKeys.collectContacts]),
    confirmDelete: _flag(j[SettingKeys.confirmDelete]),
    density: _str(j[SettingKeys.listDensity]),
    readerFontSize: _str(j[SettingKeys.readerFontSize]),
    linkClickAction: _str(j[SettingKeys.linkClickAction]),
    syncIntervalMinutes: _int(j[SettingKeys.syncInterval]),
    backgroundScheduler: _scheduler(j[SettingKeys.backgroundScheduler]),
    notificationsEnabled: _flag(j[SettingKeys.notificationsEnabled]),
    quietEnabled: _flag(j[SettingKeys.quietEnabled]),
    quietStart: _str(j[SettingKeys.quietStart]),
    quietEnd: _str(j[SettingKeys.quietEnd]),
    signatureEnabled: _flag(j[SettingKeys.signatureEnabled]),
    signatureText: _str(j[SettingKeys.signatureText]),
    replyBelowQuote: _flag(j[SettingKeys.replyBelowQuote]),
    requestMdn: _flag(j[SettingKeys.requestMdn]),
    // 1.0 is the identity scale, not a preference: zero would paint nothing.
    uiScale: _dbl(j[SettingKeys.uiScale], orElse: 1.0),
    sortField: _str(j[SettingKeys.sortField]),
    sortDescending: _flag(j[SettingKeys.sortDescending]),
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
    String? linkClickAction,
    int? syncIntervalMinutes,
    String? backgroundScheduler,
    bool? notificationsEnabled,
    bool? quietEnabled,
    String? quietStart,
    String? quietEnd,
    bool? signatureEnabled,
    String? signatureText,
    bool? replyBelowQuote,
    bool? requestMdn,
    double? uiScale,
    String? sortField,
    bool? sortDescending,
  }) => AppSettings(
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
    linkClickAction: linkClickAction ?? this.linkClickAction,
    syncIntervalMinutes: syncIntervalMinutes ?? this.syncIntervalMinutes,
    backgroundScheduler: backgroundScheduler ?? this.backgroundScheduler,
    notificationsEnabled: notificationsEnabled ?? this.notificationsEnabled,
    quietEnabled: quietEnabled ?? this.quietEnabled,
    quietStart: quietStart ?? this.quietStart,
    quietEnd: quietEnd ?? this.quietEnd,
    signatureEnabled: signatureEnabled ?? this.signatureEnabled,
    signatureText: signatureText ?? this.signatureText,
    replyBelowQuote: replyBelowQuote ?? this.replyBelowQuote,
    requestMdn: requestMdn ?? this.requestMdn,
    uiScale: uiScale ?? this.uiScale,
    sortField: sortField ?? this.sortField,
    sortDescending: sortDescending ?? this.sortDescending,
  );

  /// Reader text size multiplier, on top of the interface scale. The same
  /// steps as the Qt reader's 12 / 14 / 18 px.
  double get readerScale => switch (readerFontSize) {
    'small' => 12 / 14,
    'large' => 18 / 14,
    _ => 1.0,
  };

  /// Compact rows drop the snippet line, like the Qt frontend's density.
  bool get isCompact => density == 'compact';
}

/// What each preference may hold and what it starts as, from the core
/// (`mailcore::store::settings::choices`). Forms list [values] and label
/// them; they never keep a list or a default of their own.
class SettingChoices {
  const SettingChoices(this._choices);

  final Map<String, ({Object? defaultValue, List<Object> values})> _choices;

  factory SettingChoices.fromJson(Map<String, dynamic> j) => SettingChoices({
    for (final e in j.entries)
      if (e.value is Map)
        e.key: (
          defaultValue: (e.value as Map)['default'],
          values: [
            for (final v in ((e.value as Map)['values'] as List?) ?? const [])
              if (v != null) v as Object,
          ],
        ),
  });

  /// The values `key` offers, in display order (empty for a switch or text).
  List<T> values<T>(String key) => [
    for (final v in _choices[key]?.values ?? const <Object>[])
      if (_as<T>(v) case final T t) t,
  ];

  /// The built-in default of `key`, typed like [AppSettings] holds it.
  T? defaultOf<T>(String key) => _as<T>(_choices[key]?.defaultValue);

  /// Every default as one settings object.
  AppSettings get defaults => AppSettings.fromJson({
    for (final e in _choices.entries) e.key: e.value.defaultValue,
  });

  static T? _as<T>(Object? v) => switch (v) {
    final T t => t,
    // JSON keeps `1.0` as `1`: a double setting still reads as a double.
    final num n when T == double => n.toDouble() as T,
    _ => null,
  };
}

/// A search hit's identity: a UID is only unique within its folder.
typedef HitKey = ({String folder, int uid});

/// One account-wide FTS hit, newest first.
class SearchHit {
  const SearchHit({
    required this.uid,
    required this.folderId,
    required this.folder,
    required this.subject,
    required this.from,
    required this.date,
    this.dateKey = '',
    required this.snippet,
    required this.unread,
    required this.starred,
    required this.hasAttachments,
    this.fromName = '',
    this.badge = SenderBadge.none,
  });

  final int uid;
  final int folderId;
  final String folder;
  final String subject;
  final String from;

  /// Sender display name (`""` = address only), named like the list row.
  final String fromName;
  final SenderBadge badge;
  final String date;

  /// Names the date cases that are a word rather than a number
  /// (`mailcore::feed`), carried into [summary] so hits draw like the list.
  final String dateKey;
  final String snippet;
  final bool unread;
  final bool starred;
  final bool hasAttachments;

  HitKey get key => (folder: folder, uid: uid);

  /// The hit as a folder row, so results draw exactly like the list.
  MessageSummary get summary => MessageSummary(
    uid: uid,
    subject: subject,
    from: from,
    fromName: fromName,
    date: date,
    dateKey: dateKey,
    snippet: snippet,
    unread: unread,
    starred: starred,
    hasAttachments: hasAttachments,
    badge: badge,
  );

  factory SearchHit.fromJson(Map<String, dynamic> j) => SearchHit(
    uid: _int(j['uid']),
    folderId: _int(j['folder_id']),
    folder: _str(j['folder']),
    subject: _str(j['subject'], orElse: '(no subject)'),
    from: _str(j['from'], orElse: '?'),
    fromName: _str(j['from_name']),
    badge: SenderBadge.fromJson(j),
    date: _str(j['date']),
    dateKey: _str(j['date_key']),
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

/// Scheduler for the Android background check: `push`, `alarm` or
/// `workmanager`. Anything else falls back to WorkManager, the default —
/// mirrors `mailcore::store::settings::normalize_background_scheduler`.
String _scheduler(Object? v) {
  final s = '${v ?? ''}'.trim().toLowerCase();
  return s == 'alarm' || s == 'push' ? s : 'workmanager';
}
