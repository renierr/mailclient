/// Per-account overrides of the sync settings, and the background schedule
/// they add up to. Mirrors `mailcore::store::account_settings` and
/// `mailcore::sync::background::schedule`.
library;

import 'settings.dart';

/// Keys an account can override, mirroring `account_settings::KEYS`. All but
/// [pushEnabled] and the quiet hours are app-wide [SettingKeys] too.
abstract final class AccountSettingKeys {
  static const pushEnabled = 'push_enabled';

  /// Quiet hours: no background checks between [quietStart] and [quietEnd]
  /// (`HH:MM`, device local time). Off unless `'1'`.
  static const quietEnabled = 'quiet_hours_enabled';
  static const quietStart = 'quiet_hours_start';
  static const quietEnd = 'quiet_hours_end';
  static const defaultQuietStart = '00:00';
  static const defaultQuietEnd = '07:00';

  static const all = [
    SettingKeys.syncInterval,
    pushEnabled,
    SettingKeys.sentCopy,
    SettingKeys.collectContacts,
    SettingKeys.notificationsEnabled,
    quietEnabled,
    quietStart,
    quietEnd,
  ];
}

/// A quiet-hours end as stored: `HH:MM`, 24-hour, device local time.
/// Mirrors `account_settings::parse_time` / `format_time`.
abstract final class QuietTime {
  /// `"7:05"` / `"07:05"` as hour and minute; null for anything else.
  static ({int hour, int minute})? parse(String value) {
    final parts = value.trim().split(':');
    if (parts.length != 2) return null;
    final (h, m) = (parts[0], parts[1]);
    if (h.isEmpty || h.length > 2 || m.length != 2) return null;
    final hour = int.tryParse(h);
    final minute = int.tryParse(m);
    if (hour == null || minute == null) return null;
    if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return null;
    return (hour: hour, minute: minute);
  }

  static String format(int hour, int minute) =>
      '${hour.toString().padLeft(2, '0')}:${minute.toString().padLeft(2, '0')}';
}

/// One account's settings: what it sets itself ([overrides]) and what
/// applies to it ([effective]). Values are the stored strings: `"1"`/`"0"`
/// for switches, minutes for the interval.
class AccountSettings {
  const AccountSettings({
    required this.overrides,
    required this.effective,
    this.frequentHeartbeatSecs,
    this.quietNow = false,
  });

  static const empty = AccountSettings(overrides: {}, effective: {});

  final Map<String, String> overrides;
  final Map<String, String> effective;

  /// Seconds between the server's IDLE heartbeats (`* OK Still here`) when
  /// they come often enough to cost battery in push mode; null otherwise.
  final int? frequentHeartbeatSecs;

  /// Inside its quiet hours right now: the foreground timer leaves the
  /// account alone while nobody looks at the app.
  final bool quietNow;

  factory AccountSettings.fromJson(Map<String, dynamic> j) => AccountSettings(
    overrides: _strings(j['overrides']),
    effective: _strings(j['effective']),
    frequentHeartbeatSecs: (j['frequent_heartbeat_secs'] as num?)?.toInt(),
    quietNow: j['quiet_now'] == true,
  );

  /// Automatic check interval in minutes (0 = manually).
  int get syncIntervalMinutes =>
      int.tryParse(effective[SettingKeys.syncInterval] ?? '') ?? 0;

  /// "every 2 minutes" / "every 45 seconds" for a heartbeat gap.
  static String describeGap(int secs) {
    if (secs < 90) return 'every $secs seconds';
    return 'every ${(secs / 60).round()} minutes';
  }

  static Map<String, String> _strings(Object? raw) => raw is Map
      ? {for (final e in raw.entries) '${e.key}': '${e.value}'}
      : const {};
}

/// What Android runs in the background: the push service when any account
/// uses push, and one poller ticking every [pollMinutes] for the rest.
class BackgroundPlan {
  const BackgroundPlan({
    required this.push,
    required this.pollMinutes,
    required this.pollScheduler,
    this.quietAccounts = 0,
    this.replanAt,
  });

  final bool push;
  final int pollMinutes;

  /// `workmanager` or `alarm`.
  final String pollScheduler;

  /// Accounts left out of [push] and [pollMinutes] because they are inside
  /// their quiet hours right now.
  final int quietAccounts;

  /// When some account's quiet hours next start or end; null without any.
  final DateTime? replanAt;

  /// Whether anything checks for mail while the app is closed, now or once
  /// the quiet hours end.
  bool get any => push || pollMinutes > 0 || quietAccounts > 0;

  factory BackgroundPlan.fromJson(Map<String, dynamic> j) => BackgroundPlan(
    push: j['push'] == true,
    pollMinutes: (j['poll_minutes'] as num?)?.toInt() ?? 0,
    pollScheduler: '${j['poll_scheduler'] ?? 'workmanager'}',
    quietAccounts: (j['quiet_accounts'] as num?)?.toInt() ?? 0,
    replanAt: switch (j['replan_at']) {
      final num secs => DateTime.fromMillisecondsSinceEpoch(
        secs.toInt() * 1000,
      ),
      _ => null,
    },
  );
}
