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
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';


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

const _expeditedChannel = MethodChannel('mailclient/expedited_work');

Future<void> scheduleBackgroundSync({required int intervalMinutes}) async {
  if (!Platform.isAndroid) return;
  final effective = effectiveBackgroundMinutes(intervalMinutes);
  if (effective <= 0) {
    await _expeditedChannel.invokeMethod('cancelExpedited');
    return;
  }
  await _expeditedChannel.invokeMethod('enqueueExpedited', {
    'intervalMinutes': effective,
  });
}



const _backgroundSyncChannel = MethodChannel('mailclient/background_sync');

void registerBackgroundSyncHandler() {
  _backgroundSyncChannel.setMethodCallHandler((call) async {
    if (call.method == 'runBackgroundCheck') {
      try {
        await runBackgroundCheck();
        return true;
      } catch (_) {
        return false;
      }
    }
    return null;
  });
}

/// One headless tick: sync every account via the Rust headless check and
/// notify about mail first seen by this run. Skipped runs (sync lock held
/// elsewhere) and empty runs stay silent. Notifications can be disabled
/// independently — the sync still runs, only the alert is suppressed.
Future<void> runBackgroundCheck() async {
  // Per-isolate singleton: this re-opens the same database file and vault
  // the UI uses. `init_app` is idempotent for the same directory.
  final core = await MailCore.load();
  final report = await core.backgroundCheckNow();
  if (report['skipped'] == true) return;
  final settings = await core.settings();
  if (!settings.notificationsEnabled) return;
  final items = (report['new'] as List<dynamic>? ?? const [])
      .whereType<Map<String, dynamic>>()
      .toList(growable: false);
  if (items.isEmpty) return;
  await showNewMailNotification(items);
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
