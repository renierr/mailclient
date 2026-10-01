/// Android background mail checks, from the Dart side: which mechanism runs,
/// and the taps on the notifications they post.
///
/// The checks themselves never run in Dart. WorkManager's periodic worker,
/// the exact alarm and the IMAP IDLE push service are native
/// (`MailCheckWorker.kt`, `MailAlarm.kt`, `MailPushService.kt`) and call
/// the Rust core directly, so no Flutter engine starts for them; the
/// notification is decided in `mailcore::sync::background::notify` and
/// posted by `MailNotifier.kt`. This file only tells the platform which
/// mechanisms to run ([scheduleBackgroundChecks], called from `MailState`
/// whenever a sync setting or the account list loads or changes) and routes
/// notification taps back into the app ([listenForBackgroundEvents]).
library;

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

import 'background_power.dart';

/// Payload prefix for "open this message" taps: `mail:<account>:<folder>:<uid>`
/// (built by `mailcore::sync::background::notify::open_payload`).
const openPayloadPrefix = 'mail:';

/// The parsed form of a tap payload, or null for anything else (null
/// payload, foreign taps). Pure, so it is unit-tested.
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

/// Run what the core plans from the current settings
/// (`MailSchedule.refresh`): the push service when any account pushes, the
/// poller (`workmanager` or `alarm`) when any account is polled, the
/// quiet-hours replan alarm, and stop everything else. The host reads the
/// plan itself, since it plans again on its own when quiet hours start or
/// end. Needs the foreground engine (the channel lives in `MainActivity`).
Future<void> scheduleBackgroundChecks() => _invoke('schedule');

/// Remove the new-mail notification: the user opened the app and sees the
/// list itself.
Future<void> clearMailNotification() => _invoke('clearNotification');

/// Post a sample of the new-mail notification, for testing from Settings.
Future<void> showTestNotification() => _invoke('showTestNotification');

/// Route taps on the new-mail notification to [onOpen] — the tap that
/// launched the app included — and new mail a check found while the app
/// was open to [onMailChanged].
Future<void> listenForBackgroundEvents({
  required Future<void> Function(String payload) onOpen,
  required Future<void> Function() onMailChanged,
}) async {
  if (!Platform.isAndroid) return;
  powerChannel.setMethodCallHandler((call) async {
    switch (call.method) {
      case 'openPayload':
        if (call.arguments is String) await onOpen(call.arguments as String);
      case 'mailChanged':
        await onMailChanged();
    }
    return null;
  });
  try {
    final launch = await powerChannel.invokeMethod<String>('takeLaunchPayload');
    if (launch != null) await onOpen(launch);
  } on PlatformException {
    // No launch tap to deliver.
  } on MissingPluginException {
    // Tests, or an engine without MainActivity.
  }
}

/// Ask Android for the runtime notification permission (API 33+ shows a
/// system prompt; older versions grant it at install time). Call it when
/// background checks get enabled or before a test notification — no
/// checks, no notifications, no prompt.
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

Future<void> _invoke(String method, [Object? argument]) async {
  if (!Platform.isAndroid) return;
  try {
    await powerChannel.invokeMethod<bool>(method, argument);
  } on PlatformException {
    // Best effort; the next app start or settings change tries again.
  } on MissingPluginException {
    // Tests, or an engine without MainActivity.
  }
}
