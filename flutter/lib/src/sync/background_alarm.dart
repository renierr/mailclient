/// Android exact-alarm background mail check: an alternative scheduler to
/// the WorkManager periodic worker in `background_sync.dart`.
///
/// WorkManager tasks are deferrable by design — in Doze they only run in
/// maintenance windows, so notifications land when the phone is unlocked.
/// An exact alarm (`setExactAndAllowWhileIdle`, via
/// `android_alarm_manager_plus`) fires on time in standby at the cost of a
/// wakeup per check. Same check, same notification: the alarm callback runs
/// [runBackgroundCheck], so marks, dedup and tap targets are shared.
///
/// WorkManager stays the default; Settings switches the mode per the
/// `background_scheduler` Rust setting. Everything here is Android-only and
/// a no-op elsewhere.
library;

import 'dart:io';

import 'package:android_alarm_manager_plus/android_alarm_manager_plus.dart';

import 'background_sync.dart';

/// Alarm id for the periodic mail check. Must fit in 31 bits.
const alarmSyncId = 1001;

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

/// Register (or cancel, when the interval is 0) the exact-alarm check.
/// Cancels nothing else — the caller stops the WorkManager task when the
/// alarm mode is active.
Future<void> scheduleAlarmSync({required int intervalMinutes}) async {
  if (!Platform.isAndroid) return;
  final effective = effectiveAlarmMinutes(intervalMinutes);
  if (effective <= 0) {
    await cancelAlarmSync();
    return;
  }
  await AndroidAlarmManager.periodic(
    Duration(minutes: effective),
    alarmSyncId,
    alarmCheckCallback,
    exact: true,
    wakeup: true,
    allowWhileIdle: true,
    rescheduleOnReboot: true,
  );
}

/// Stop the exact-alarm check, if any.
Future<void> cancelAlarmSync() async {
  if (!Platform.isAndroid) return;
  await AndroidAlarmManager.cancel(alarmSyncId);
}

/// The alarm entry point: runs in a headless isolate, so it must stay a
/// top-level function. Same check the WorkManager dispatcher runs.
@pragma('vm:entry-point')
Future<void> alarmCheckCallback() => runBackgroundCheck();
