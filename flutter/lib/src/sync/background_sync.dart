/// Android background mail check: WorkManager scheduling plus the worker
/// that syncs and notifies while the app is dead.
///
/// The in-app `Timer` in `MailState` only fires while the UI lives. This is
/// the part that survives it: a battery-safe periodic worker (OS-enforced
/// minimum 15 minutes, only runs when a network is connected, Doze-aware via
/// WorkManager) that calls the Rust headless check and posts a system
/// notification for mail that arrived since the previous run.
///
/// Flow: `scheduleBackgroundSync` (called from `MailState` whenever the
/// sync-interval setting loads or changes) registers or cancels the worker;
/// the OS wakes `backgroundSyncDispatcher` in a headless Dart isolate, which
/// runs `runBackgroundCheck`, shows the notification, and returns.
/// Each mail notifies once: SharedPreferences stores the highest notified
/// inbox UID per account and folder, independent of the Rust sync watermark.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:workmanager/workmanager.dart';

import '../ffi/mail_core.dart';

/// WorkManager unique name for the periodic check.
const backgroundSyncTask = 'mail-background-sync';

/// Android's minimum periodic interval. The settings UI still offers 5/10
/// minutes for the *foreground* timer; the background worker clamps to this.
const androidMinIntervalMinutes = 15;

/// Notification channel for new-mail alerts.
const newMailChannelId = 'mail_new';
const newMailChannelName = 'New mail';

/// Payload prefix for "open this message" taps: `mail:<account>:<folder>:<uid>`.
const openPayloadPrefix = 'mail:';

/// What the background worker should run at: 0 disables, anything below the
/// OS minimum clamps up to it. Pure, so it is unit-tested.
int effectiveBackgroundMinutes(int settingMinutes) {
  if (settingMinutes <= 0) return 0;
  return settingMinutes < androidMinIntervalMinutes
      ? androidMinIntervalMinutes
      : settingMinutes;
}

/// Encode a notification tap target. The worker stamps it on the
/// notification; the UI parses it back with [parseOpenPayload].
String encodeOpenPayload({
  required int accountId,
  required int folderId,
  required int uid,
}) => '$openPayloadPrefix$accountId:$folderId:$uid';

/// The parsed form of [encodeOpenPayload], or null for anything else
/// (null payload, foreign taps). Pure, so it is unit-tested.
({int accountId, int folderId, int uid})? parseOpenPayload(String? payload) {
  if (payload == null || !payload.startsWith(openPayloadPrefix)) return null;
  final parts = payload.substring(openPayloadPrefix.length).split(':');
  if (parts.length != 3) return null;
  final ids = <int>[];
  for (final part in parts) {
    final id = int.tryParse(part);
    if (id == null || id < 0) return null;
    ids.add(id);
  }
  return (accountId: ids[0], folderId: ids[1], uid: ids[2]);
}

Future<void> scheduleBackgroundSync({required int intervalMinutes}) async {
  if (!Platform.isAndroid) return;
  final effective = effectiveBackgroundMinutes(intervalMinutes);
  if (effective <= 0) {
    await Workmanager().cancelByUniqueName(backgroundSyncTask);
    return;
  }
  await Workmanager().registerPeriodicTask(
    backgroundSyncTask,
    backgroundSyncTask,
    frequency: Duration(minutes: effective),
    initialDelay: Duration(minutes: effective),
    constraints: Constraints(networkType: NetworkType.connected),
    existingWorkPolicy: ExistingPeriodicWorkPolicy.update,
    backoffPolicy: BackoffPolicy.exponential,
  );
}

@pragma('vm:entry-point')
void backgroundSyncDispatcher() {
  Workmanager().executeTask((task, _) async {
    if (task != backgroundSyncTask) return true;
    try {
      await runBackgroundCheck();
      final minutes = (await (await MailCore.load()).settings()).syncIntervalMinutes;
      await scheduleBackgroundSync(intervalMinutes: minutes);
      return true;
    } catch (_) {
      return false;
    }
  });
}

Future<void> runBackgroundCheck() async {
  final core = await MailCore.load();
  final report = await core.backgroundCheckNow();
  if (report['skipped'] == true) return;
  final settings = await core.settings();
  final items = (report['new'] as List<dynamic>? ?? const [])
      .whereType<Map<String, dynamic>>()
      .toList(growable: false);
  if (items.isEmpty) return;
  final prefs = await SharedPreferences.getInstance();
  final pending = <_PendingMail>[];
  for (final item in items) {
    final mail = _PendingMail.from(item);
    if (mail == null) continue;
    final mark = readNotifiedMark(prefs, mail.accountId, mail.folderId);
    if (mark != null &&
        (mark.validity == mail.uidValidity || mark.validity == 0) &&
        mail.uid <= mark.uid) {
      continue;
    }
    pending.add(mail);
  }
  if (pending.isEmpty) return;
  // Alerts off: remember what this run saw so turning them back on does not
  // ding for mail that arrived in the gap. The Rust cursor has already moved.
  if (!settings.notificationsEnabled) {
    await _advanceNotifiedMarks(prefs, pending);
    return;
  }
  try {
    await showNewMailNotification(pending.map((m) => m.item).toList(growable: false));
  } catch (_) {
    // Leave the marks where they are so the next run retries the post.
    return;
  }
  await _advanceNotifiedMarks(prefs, pending);
}

/// Post a mock notification with all display options, for testing from
/// settings. Uses a fixed payload that opens the app's default view.
Future<void> showTestNotification() async {
  await showNewMailNotification([
    {
      'account_id': -1,
      'account_email': 'test@mailclient',
      'folder_id': -1,
      'folder': 'INBOX',
      'uid': 0,
      'from': 'Mailclient Test',
      'subject': 'Test notification — tap to open the app',
      'date': DateTime.now().toIso8601String(),
    },
  ]);
}

/// Post the system notification for [items] (never empty when called).
/// One mail notifies directly; several collapse into a single inbox-style
/// summary whose tap opens the newest — a ding per message would be spam.
Future<void> showNewMailNotification(
  List<Map<String, dynamic>> items, {
  FlutterLocalNotificationsPlugin? plugin,
}) async {
  final notifications = plugin ?? FlutterLocalNotificationsPlugin();
  if (plugin == null) {
    await notifications.initialize(
      settings: const InitializationSettings(
        android: AndroidInitializationSettings('@mipmap/ic_launcher'),
      ),
    );
  }
  final newest = items.first;
  final payload = encodeOpenPayload(
    accountId: _asInt(newest['account_id']),
    folderId: _asInt(newest['folder_id']),
    uid: _asInt(newest['uid']),
  );
  if (items.length == 1) {
    await notifications.show(
      id: 0,
      title: _titleOf(newest),
      body: _bodyOf(newest),
      notificationDetails: const NotificationDetails(
        android: AndroidNotificationDetails(
          newMailChannelId,
          newMailChannelName,
          channelDescription:
              'Alerts for mail that arrived while the app was closed',
          importance: Importance.high,
          priority: Priority.high,
          groupKey: newMailChannelId,
        ),
      ),
      payload: payload,
    );
    return;
  }
  final lines = items
      .take(5)
      .map((m) => '${_titleOf(m)} — ${_bodyOf(m)}')
      .toList(growable: false);
  await notifications.show(
    id: 0,
    title: '${items.length} new messages',
    body: '${_titleOf(newest)} — ${_bodyOf(newest)}',
    notificationDetails: NotificationDetails(
      android: AndroidNotificationDetails(
        newMailChannelId,
        newMailChannelName,
        channelDescription:
            'Alerts for mail that arrived while the app was closed',
        importance: Importance.high,
        priority: Priority.high,
        styleInformation: InboxStyleInformation(
          lines,
          contentTitle: '${items.length} new messages',
          summaryText: items.length > 5 ? '+${items.length - 5} more' : null,
        ),
        groupKey: newMailChannelId,
        setAsGroupSummary: true,
      ),
    ),
    payload: payload,
  );
}

String _titleOf(Map<String, dynamic> m) {
  final from = (m['from'] as String? ?? '').trim();
  if (from.isNotEmpty) return from;
  return (m['account_email'] as String? ?? '').trim();
}

String _bodyOf(Map<String, dynamic> m) {
  final subject = (m['subject'] as String? ?? '').trim();
  return subject.isNotEmpty ? subject : '(no subject)';
}

int _asInt(Object? v) => switch (v) {
  int n => n,
  num n => n.toInt(),
  String s => int.tryParse(s) ?? -1,
  _ => -1,
};

/// Highest UID already handled for one folder, scoped to a UIDVALIDITY.
/// A server reset changes the validity and the old UID must not silence
/// mail that reuses it.
class _PendingMail {
  const _PendingMail(this.item, this.accountId, this.folderId, this.uid, this.uidValidity);

  final Map<String, dynamic> item;
  final int accountId;
  final int folderId;
  final int uid;
  final int uidValidity;

  static _PendingMail? from(Map<String, dynamic> item) {
    final accountId = _asInt(item['account_id']);
    final folderId = _asInt(item['folder_id']);
    final uid = _asInt(item['uid']);
    final validity = _asInt(item['uid_validity']);
    if (accountId < 0 || folderId < 0 || uid < 0 || validity < 0) return null;
    return _PendingMail(item, accountId, folderId, uid, validity);
  }
}

({int validity, int uid})? readNotifiedMark(SharedPreferences prefs, int accountId, int folderId) {
  final key = _notifiedKey(accountId, folderId);
  // A key previously stored as an int throws on getString. That is the
  // legacy mark, handled below.
  final raw = _stringOrNull(prefs, key);
  if (raw != null) {
    final parts = raw.split(':');
    if (parts.length != 2) return null;
    final validity = int.tryParse(parts[0]);
    final uid = int.tryParse(parts[1]);
    if (validity == null || uid == null) return null;
    return (validity: validity, uid: uid);
  }
  // Earlier builds stored a bare UID. Keep it so the format change does not
  // re-notify mail already alerted; the next advance rewrites the string.
  final legacy = prefs.getInt(key);
  if (legacy == null) return null;
  return (validity: 0, uid: legacy);
}

String _notifiedKey(int accountId, int folderId) => 'notified_uid_${accountId}_$folderId';

String? _stringOrNull(SharedPreferences prefs, String key) {
  try {
    return prefs.getString(key);
  } catch (_) {
    return null;
  }
}

Future<void> _advanceNotifiedMarks(SharedPreferences prefs, List<_PendingMail> pending) async {
  final top = <(int, int), ({int validity, int uid})>{};
  for (final mail in pending) {
    final key = (mail.accountId, mail.folderId);
    final current = top[key];
    if (current == null || mail.uidValidity != current.validity || mail.uid > current.uid) {
      top[key] = (validity: mail.uidValidity, uid: mail.uid);
    }
  }
  for (final entry in top.entries) {
    final stored = readNotifiedMark(prefs, entry.key.$1, entry.key.$2);
    final next = entry.value;
    if (stored != null && stored.validity == next.validity && stored.uid >= next.uid) {
      continue;
    }
    await prefs.setString(_notifiedKey(entry.key.$1, entry.key.$2), '${next.validity}:${next.uid}');
  }
}

/// Ask Android for the runtime notification permission (API 33+ shows a
/// system prompt; older versions grant it at install time and the native
/// side answers `true` immediately). Only worth calling when background
/// checks are enabled — no worker, no notifications, no prompt.
Future<bool> requestNotificationPermission() async {
  if (!Platform.isAndroid) return true;
  const channel = MethodChannel('mailclient/permissions');
  try {
    return await channel.invokeMethod<bool>('requestNotifications') ?? false;
  } catch (_) {
    return false;
  }
}

/// The JSON `background_check_now` shape, decoded for tests and callers.
Map<String, dynamic> decodeBackgroundReport(String raw) =>
    jsonDecode(raw) as Map<String, dynamic>;
