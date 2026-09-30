/// Values of the `background_scheduler` setting: which Android mechanism
/// checks for mail while the app is closed (see `background_sync.dart`).
///
/// - WorkManager: a deferrable periodic worker. Cheapest, but Doze runs it
///   only in maintenance windows, so notifications may wait for an unlock.
/// - Alarm: an exact alarm per check (`MailAlarm.kt`), on time in standby
///   at the cost of a wakeup per check.
/// - Push: IMAP IDLE in a foreground service (`MailPushService.kt`). The
///   server announces new mail as it arrives; between arrivals the phone
///   only wakes for a keep-alive.
library;

const schedulerWorkmanager = 'workmanager';
const schedulerAlarm = 'alarm';
const schedulerPush = 'push';

/// Normalize a stored scheduler value. Unknown, empty or null values fall
/// back to WorkManager — the safe default a hand-edited row gets. Mirrors
/// `mailcore::store::settings::normalize_background_scheduler`. Pure, so
/// it is unit-tested.
String normalizeScheduler(Object? raw) {
  final v = '${raw ?? ''}'.trim().toLowerCase();
  return switch (v) {
    schedulerAlarm || schedulerPush => v,
    _ => schedulerWorkmanager,
  };
}
