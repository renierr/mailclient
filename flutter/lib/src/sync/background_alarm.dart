/// Android exact-alarm background mail check: an alternative scheduler to
/// the WorkManager periodic worker in `background_sync.dart`.
///
/// WorkManager periodic tasks are deferrable by design: in Doze they only
/// run in maintenance windows, so notifications land when the phone is
/// unlocked. This scheduler fires on time in standby at the cost of a
/// wakeup per check. The platform half is `MailAlarm.kt`: a self-rearming
/// exact one-shot alarm (`setExactAndAllowWhileIdle`) whose receiver
/// enqueues the check as expedited WorkManager work (`alarmCheckTask`),
/// which Doze does not defer the way it defers plain jobs. Same dispatcher,
/// same check, same notification as the periodic worker; the run history
/// tells them apart.
///
/// The alarm re-arms itself natively after every shot, a reboot or an app
/// update, so Dart only arms or cancels it from the foreground, whenever
/// the interval or scheduler setting changes.
///
/// WorkManager stays the default; Settings switches the mode per the
/// `background_scheduler` Rust setting. Everything here is Android-only and
/// a no-op elsewhere.
library;

import 'dart:io';

import 'package:flutter/services.dart';

import 'background_power.dart';

/// Values of the `background_scheduler` setting.
const schedulerWorkmanager = 'workmanager';
const schedulerAlarm = 'alarm';

/// Normalize a stored scheduler value. Unknown, empty or null values fall
/// back to WorkManager — the safe default a hand-edited row gets. Pure, so
/// it is unit-tested.
String normalizeScheduler(Object? raw) {
  final v = '${raw ?? ''}'.trim().toLowerCase();
  return v == schedulerAlarm ? schedulerAlarm : schedulerWorkmanager;
}

/// What the alarm should run at: 0 disables, anything else passes through.
/// Unlike WorkManager there is no 15-minute OS floor — the settings steps
/// (5/10/…) are honoured as-is. Pure, so it is unit-tested.
int effectiveAlarmMinutes(int settingMinutes) {
  if (settingMinutes <= 0) return 0;
  return settingMinutes;
}

/// Arm the exact-alarm check (or cancel it, when the interval is 0).
/// Cancels nothing else: the caller stops the WorkManager task when the
/// alarm mode is active. Needs the foreground engine (the platform channel
/// lives in `MainActivity`).
Future<void> scheduleAlarmSync({required int intervalMinutes}) async {
  if (!Platform.isAndroid) return;
  final effective = effectiveAlarmMinutes(intervalMinutes);
  if (effective <= 0) {
    await cancelAlarmSync();
    return;
  }
  await _invoke('armAlarm', effective);
}

/// Stop the exact-alarm check, if any.
Future<void> cancelAlarmSync() async {
  if (!Platform.isAndroid) return;
  await _invoke('cancelAlarm');
}

Future<void> _invoke(String method, [Object? argument]) async {
  try {
    await powerChannel.invokeMethod<bool>(method, argument);
  } on PlatformException {
    // Scheduling is best effort; the next app start tries again.
  } on MissingPluginException {
    // Tests, or an engine without MainActivity.
  }
}
