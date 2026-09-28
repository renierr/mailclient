/// Android exact-alarm background mail check: an alternative scheduler to
/// the WorkManager periodic worker in `background_sync.dart`.
///
/// WorkManager tasks are deferrable by design — in Doze they only run in
/// maintenance windows, so notifications land when the phone is unlocked.
/// This scheduler fires on time in standby at the cost of a wakeup per
/// check, via a self-perpetuating exact one-shot alarm
/// (`setExactAndAllowWhileIdle`, via `android_alarm_manager_plus`):
/// each firing runs [runBackgroundCheck] and then arms the next one-shot
/// from the current settings. Same check, same notification: the alarm
/// callback runs [runBackgroundCheck], so marks, dedup and tap targets are
/// shared.
///
/// A plugin-level `periodic` alarm cannot do this: the plugin maps
/// `periodic` to `setRepeating`/`setInexactRepeating` and ignores
/// `allowWhileIdle` there, so a periodic alarm is deferred in Doze just
/// like the WorkManager task it was meant to replace. Only the one-shot
/// path reaches `setExactAndAllowWhileIdle`/`setAndAllowWhileIdle`.
///
/// WorkManager stays the default; Settings switches the mode per the
/// `background_scheduler` Rust setting. Everything here is Android-only and
/// a no-op elsewhere.
library;

import 'dart:io';

import 'package:android_alarm_manager_plus/android_alarm_manager_plus.dart';

import '../ffi/mail_core.dart';
import 'background_power.dart';
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

/// Arm the exact-alarm check (or cancel it, when the interval is 0).
/// Cancels nothing else — the caller stops the WorkManager task when the
/// alarm mode is active.
///
/// This arms a single one-shot alarm, not a repeating one (see the library
/// docs for why); [alarmCheckCallback] re-arms the next shot after every
/// run, so the chain follows settings changes within one interval.
/// Without the exact-alarm grant the shot is still armed via AllowWhileIdle
/// (`setAndAllowWhileIdle`), just not at the exact minute — an inexact
/// alarm that fires beats an exact one the OS silently drops.
Future<void> scheduleAlarmSync({required int intervalMinutes}) async {
  if (!Platform.isAndroid) return;
  final effective = effectiveAlarmMinutes(intervalMinutes);
  if (effective <= 0) {
    await cancelAlarmSync();
    return;
  }
  await AndroidAlarmManager.oneShot(
    Duration(minutes: effective),
    alarmSyncId,
    alarmCheckCallback,
    exact: await exactAlarmPermitted(),
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
/// top-level function. Same check the WorkManager dispatcher runs, then
/// re-arm the next one-shot so the chain keeps going while the alarm
/// scheduler is active. Re-arming lives in `finally` so a failed network
/// run does not silently end the chain; a scheduler switch or disable
/// meanwhile simply skips the re-arm (the foreground already owns the new
/// schedule then).
@pragma('vm:entry-point')
Future<void> alarmCheckCallback() async {
  try {
    await runBackgroundCheck();
  } finally {
    try {
      final settings = await MailCore.load().then((c) => c.settings());
      if (normalizeScheduler(settings.backgroundScheduler) ==
          schedulerAlarm) {
        await scheduleAlarmSync(
          intervalMinutes: settings.syncIntervalMinutes,
        );
      }
    } catch (_) {
      // No settings, no re-arm: the next app start reschedules.
    }
  }
}
