/// Whether Android lets the background worker run on time, and what the
/// last run did.
///
/// A WorkManager periodic task is deferrable: while the phone sleeps, Doze
/// only runs it in rare maintenance windows, and App Standby rations it by
/// how often the app is opened. The battery-optimisation exemption lifts
/// both, which is why Settings asks for it. The platform half lives in
/// `MainActivity.kt` (channel [powerChannel]); the pure formatting here is
/// unit-tested.
library;

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:intl/intl.dart';

const powerChannel = MethodChannel('mailclient/background_power');

/// A run that started this long ago without finishing was stopped by
/// Android rather than still running (WorkManager's own limit is 10 min).
const staleRunAfter = Duration(minutes: 10);

class PowerStatus {
  const PowerStatus({required this.unrestricted, required this.standbyBucket});

  /// Exempt from battery optimisation: Doze and App Standby do not defer
  /// the worker.
  final bool unrestricted;

  /// `UsageStatsManager` bucket (10 active … 45 restricted), 0 if unknown.
  final int standbyBucket;

  static PowerStatus? fromMap(Object? raw) {
    if (raw is! Map) return null;
    final unrestricted = raw['unrestricted'];
    final bucket = raw['standbyBucket'];
    if (unrestricted is! bool) return null;
    return PowerStatus(
      unrestricted: unrestricted,
      standbyBucket: bucket is int ? bucket : 0,
    );
  }
}

/// Current status, or null off Android or when the channel is missing
/// (tests, the headless worker engine).
Future<PowerStatus?> backgroundPowerStatus() async {
  if (!Platform.isAndroid) return null;
  try {
    return PowerStatus.fromMap(await powerChannel.invokeMethod('status'));
  } on PlatformException {
    return null;
  } on MissingPluginException {
    return null;
  }
}

/// Open Android's "stop optimising battery usage?" prompt. Returns once the
/// prompt is up, not when the user answered — re-read the status on resume.
Future<bool> requestUnrestrictedBackground() async {
  if (!Platform.isAndroid) return false;
  try {
    return await powerChannel.invokeMethod<bool>('requestUnrestricted') ??
        false;
  } on PlatformException {
    return false;
  } on MissingPluginException {
    return false;
  }
}

/// Whether the system lets the app schedule exact alarms (Android 12+,
/// denied by default since 14). Without it the alarm scheduler still fires
/// via AllowWhileIdle, just not at the exact minute.
Future<bool> exactAlarmPermitted() async {
  if (!Platform.isAndroid) return true;
  try {
    return await powerChannel.invokeMethod<bool>('exactAlarmStatus') ?? true;
  } on PlatformException {
    return true;
  } on MissingPluginException {
    return true;
  }
}

/// Open the system "Alarms & reminders" screen so the user can allow exact
/// alarms. Returns once the screen is up — re-read the status on resume.
Future<bool> requestExactAlarm() async {
  if (!Platform.isAndroid) return false;
  try {
    return await powerChannel.invokeMethod<bool>('requestExactAlarm') ?? false;
  } on PlatformException {
    return false;
  } on MissingPluginException {
    return false;
  }
}

/// Human name of a standby bucket, or null for buckets that do not limit
/// the worker (exempt, active, working set) and unknown values.
String? limitingBucketLabel(int bucket) => switch (bucket) {
  30 => 'frequent',
  40 => 'rare',
  45 => 'restricted',
  50 => 'never used',
  _ => null,
};

/// One line describing [run] (the `backgroundLastRun` map) as of [now].
String describeLastRun(Map<String, dynamic>? run, DateTime now) {
  final started = DateTime.tryParse('${run?['started_at'] ?? ''}');
  if (run == null || started == null) {
    return 'No background check has run yet.';
  }
  final when = _when(started.toLocal(), now);
  final finished = DateTime.tryParse('${run['finished_at'] ?? ''}');
  if (finished == null) {
    return now.difference(started) < staleRunAfter
        ? 'Background check running since $when.'
        : 'Last background check started $when but did not finish '
              '— Android stopped it.';
  }
  if (run['skipped'] == true) {
    return 'Last background check $when: skipped, another sync was running.';
  }
  final errors = (run['errors'] as List?)?.whereType<String>().toList() ?? [];
  if (errors.isNotEmpty) {
    return 'Last background check $when failed: ${errors.first}';
  }
  final n = run['new'] is int ? run['new'] as int : 0;
  final found = switch (n) {
    0 => 'no new mail',
    1 => '1 new message',
    _ => '$n new messages',
  };
  return 'Last background check $when: $found.';
}

String _when(DateTime at, DateTime now) {
  final sameDay =
      at.year == now.year && at.month == now.month && at.day == now.day;
  final clock = sameDay
      ? DateFormat.Hm().format(at)
      : DateFormat('d MMM, HH:mm').format(at);
  return '$clock (${_ago(now.difference(at))})';
}

String _ago(Duration d) {
  if (d.inMinutes < 1) return 'just now';
  if (d.inMinutes < 60) return '${d.inMinutes} min ago';
  if (d.inHours < 48) return '${d.inHours} h ago';
  return '${d.inDays} days ago';
}
