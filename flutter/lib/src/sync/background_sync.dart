/// Android background mail check: WorkManager scheduling plus the worker
/// that syncs and notifies while the app is dead.
///
/// The in-app `Timer` in `MailState` only fires while the UI lives. This is
/// the part that survives it: a battery-safe periodic worker (OS-enforced
/// minimum 15 minutes, only runs when a network is connected) that calls the
/// Rust headless check and posts a system notification for mail that arrived
/// since the previous run. While the phone sleeps, Doze and App Standby defer
/// the worker unless the app is exempt from battery optimisation — see
/// `background_power.dart`, which Settings uses to ask for that.
///
/// Flow: `scheduleBackgroundSync` (called from `MailState` whenever the
/// sync-interval setting loads or changes) registers or cancels the worker;
/// the OS wakes `backgroundSyncDispatcher` in a headless Dart isolate, which
/// runs `runBackgroundCheck`, shows the notification, and returns.
/// Each mail notifies once: the Rust check reports mail above a per-folder
/// mark and hands the new marks back, and they are committed only after the
/// notification was posted — a failed post is reported again next run.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_local_notifications/flutter_local_notifications.dart';
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

/// Status-bar icon. Must be the single-colour layer: the adaptive launcher
/// icon renders as a white blob there.
const notificationIcon = '@drawable/ic_launcher_monochrome';

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

/// Stop the WorkManager periodic check, if any. Used when the exact-alarm
/// scheduler takes over, so the two never run side by side.
Future<void> cancelBackgroundSync() async {
  if (!Platform.isAndroid) return;
  await Workmanager().cancelByUniqueName(backgroundSyncTask);
}

/// WorkManager name of the check the native exact alarm enqueues
/// (`MailAlarmReceiver.kt`); runs through [backgroundSyncDispatcher] like
/// the periodic task, so only the run history can tell them apart.
const alarmCheckTask = 'mail-alarm-check';

@pragma('vm:entry-point')
void backgroundSyncDispatcher() {
  Workmanager().executeTask((task, _) async {
    final trigger = switch (task) {
      backgroundSyncTask => 'worker',
      alarmCheckTask => 'alarm',
      _ => null,
    };
    if (trigger == null) return true;
    try {
      await runBackgroundCheck(trigger: trigger);
      return true;
    } catch (_) {
      return false;
    }
  });
}

Future<void> runBackgroundCheck({required String trigger}) async {
  final core = await MailCore.load();
  final report = await core.backgroundCheckNow(trigger);
  if (report['skipped'] == true) return;
  final run = '${report['run'] ?? ''}';
  Future<void> note(String outcome) async {
    if (run.isEmpty) return;
    try {
      await core.backgroundRecordOutcome(run, outcome);
    } catch (_) {
      // Diagnostics only.
    }
  }

  final fresh = _mailList(report['new']);
  final pending = _mailList(report['pending']);
  // Pending covers fresh; the fallback only guards an older core.
  final listed = pending.isEmpty ? fresh : pending;
  final marks = jsonEncode(report['marks'] ?? const []);
  final plugin = await _initializedPlugin();
  final action = notifyAction(
    hasNew: fresh.isNotEmpty,
    alertsOn: (await core.settings()).notificationsEnabled,
    permitted: await notificationsPermitted(),
    shown: await _shownText(plugin),
    wanted: listed.isEmpty ? null : notificationText(listed).signature,
  );
  try {
    switch (action) {
      case NotifyAction.alert:
        await showNewMailNotification(listed, plugin: plugin);
      case NotifyAction.update:
        await showNewMailNotification(listed, plugin: plugin, silent: true);
      case NotifyAction.clear:
        await plugin.cancel(id: newMailNotificationId);
      case NotifyAction.alertsOff:
      case NotifyAction.blocked:
      case NotifyAction.none:
        break;
    }
  } catch (e) {
    // Marks stay uncommitted, so the next run reports this mail again.
    await note('notification failed: $e');
    return;
  }
  await core.commitBackgroundMarks(marks);
  final outcome = switch (action) {
    NotifyAction.alert => 'notified (${listed.length})',
    NotifyAction.update => 'notification updated',
    NotifyAction.clear => 'notification cleared',
    NotifyAction.alertsOff => 'new mail, alerts are off',
    NotifyAction.blocked => 'new mail, notifications blocked',
    NotifyAction.none => null,
  };
  if (outcome != null) await note(outcome);
}

List<Map<String, dynamic>> _mailList(Object? raw) =>
    (raw as List<dynamic>? ?? const [])
        .whereType<Map<String, dynamic>>()
        .toList(growable: false);

/// What a background run does with the notification.
enum NotifyAction {
  /// New mail: post (or replace) the notification and make a sound.
  alert,

  /// Nothing new, but the visible notification lists mail that has been
  /// read meanwhile: repost it quietly.
  update,

  /// Everything the visible notification listed has been read: remove it.
  clear,

  /// New mail, but the user turned alerts off.
  alertsOff,

  /// New mail, but Android does not let the app notify.
  blocked,

  /// Nothing to do.
  none,
}

/// Pure decision for [runBackgroundCheck], so it is unit-tested.
///
/// [shown] is the signature of the notification on screen (null when there
/// is none — never posted, or swiped away) and [wanted] that of what it
/// should list now (null when nothing is unseen). A swiped-away notification
/// only comes back for new mail.
NotifyAction notifyAction({
  required bool hasNew,
  required bool alertsOn,
  required bool permitted,
  required String? shown,
  required String? wanted,
}) {
  if (hasNew) {
    if (!alertsOn) return NotifyAction.alertsOff;
    return permitted ? NotifyAction.alert : NotifyAction.blocked;
  }
  if (shown == null) return NotifyAction.none;
  if (wanted == null) return NotifyAction.clear;
  return shown == wanted ? NotifyAction.none : NotifyAction.update;
}

/// Whether the OS will actually show a notification. `show()` does not
/// throw when the user denied it, so this is checked up front.
Future<bool> notificationsPermitted() async {
  if (!Platform.isAndroid) return true;
  final android = FlutterLocalNotificationsPlugin()
      .resolvePlatformSpecificImplementation<
        AndroidFlutterLocalNotificationsPlugin
      >();
  return await android?.areNotificationsEnabled() ?? true;
}

/// The one new-mail notification: every post replaces it.
const newMailNotificationId = 0;

/// Remove the new-mail notification: the user opened the app and sees the
/// list itself.
Future<void> clearMailNotification() async {
  if (!Platform.isAndroid) return;
  try {
    await FlutterLocalNotificationsPlugin().cancel(id: newMailNotificationId);
  } catch (_) {
    // Nothing shown, or the plugin is not ready: nothing to clear.
  }
}

Future<FlutterLocalNotificationsPlugin> _initializedPlugin() async {
  final plugin = FlutterLocalNotificationsPlugin();
  await plugin.initialize(
    settings: const InitializationSettings(
      android: AndroidInitializationSettings(notificationIcon),
    ),
  );
  return plugin;
}

/// Signature of the new-mail notification on screen, or null.
Future<String?> _shownText(FlutterLocalNotificationsPlugin plugin) async {
  try {
    final active = await plugin
        .resolvePlatformSpecificImplementation<
          AndroidFlutterLocalNotificationsPlugin
        >()
        ?.getActiveNotifications();
    final mine = active?.where((n) => n.id == newMailNotificationId);
    if (mine == null || mine.isEmpty) return null;
    return NotificationText.signatureOf(mine.first.title, mine.first.body);
  } catch (_) {
    return null;
  }
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

/// Title, body and inbox lines of the notification for some mail.
class NotificationText {
  const NotificationText({
    required this.title,
    required this.body,
    required this.lines,
    this.summary,
  });

  final String title;
  final String body;
  final List<String> lines;
  final String? summary;

  /// What [signatureOf] reads back from the posted notification.
  String get signature => signatureOf(title, body);

  static String signatureOf(String? title, String? body) =>
      '${title ?? ''}\n${body ?? ''}';
}

/// Pure layout of the notification for [items] (never empty), newest first
/// or oldest first as the report orders them: the title names the sender
/// of the last one, or the count when there are several. Unit-tested.
NotificationText notificationText(List<Map<String, dynamic>> items) {
  final newest = items.last;
  if (items.length == 1) {
    return NotificationText(
      title: _titleOf(newest),
      body: _bodyOf(newest),
      lines: const [],
    );
  }
  final recent = items.reversed.take(5).toList(growable: false);
  return NotificationText(
    title: '${items.length} new messages',
    body: '${_titleOf(newest)} — ${_bodyOf(newest)}',
    lines: [for (final m in recent) '${_titleOf(m)} — ${_bodyOf(m)}'],
    summary: items.length > 5 ? '+${items.length - 5} more' : null,
  );
}

/// Post the system notification for [items] (never empty when called):
/// one notification, replaced on every post. Several mails show as an
/// inbox-style list whose tap opens the newest. A lone group summary is
/// avoided on purpose: many Android versions do not show a summary without
/// child notifications. [silent] reposts without sound or vibration.
Future<void> showNewMailNotification(
  List<Map<String, dynamic>> items, {
  FlutterLocalNotificationsPlugin? plugin,
  bool silent = false,
}) async {
  final notifications = plugin ?? await _initializedPlugin();
  final newest = items.last;
  final payload = encodeOpenPayload(
    accountId: _asInt(newest['account_id']),
    folderId: _asInt(newest['folder_id']),
    uid: _asInt(newest['uid']),
  );
  final text = notificationText(items);
  await notifications.show(
    id: newMailNotificationId,
    title: text.title,
    body: text.body,
    notificationDetails: NotificationDetails(
      android: AndroidNotificationDetails(
        newMailChannelId,
        newMailChannelName,
        channelDescription:
            'Alerts for mail that arrived while the app was closed',
        importance: Importance.high,
        priority: Priority.high,
        silent: silent,
        number: items.length,
        styleInformation: text.lines.isEmpty
            ? null
            : InboxStyleInformation(
                text.lines,
                contentTitle: text.title,
                summaryText: text.summary,
              ),
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
/// system prompt; older versions grant it at install time). Call it when
/// background checks get enabled or before a test notification — no
/// worker, no notifications, no prompt.
Future<bool> requestNotificationPermission() async {
  if (!Platform.isAndroid) return true;
  final android = FlutterLocalNotificationsPlugin()
      .resolvePlatformSpecificImplementation<
        AndroidFlutterLocalNotificationsPlugin
      >();
  try {
    return await android?.requestNotificationsPermission() ?? true;
  } catch (_) {
    return false;
  }
}

/// The JSON `background_check_now` shape, decoded for tests and callers.
Map<String, dynamic> decodeBackgroundReport(String raw) =>
    jsonDecode(raw) as Map<String, dynamic>;
