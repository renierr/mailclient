import 'package:flutter/material.dart';

import '../../models/account_settings.dart';
import '../../models/settings.dart';
import '../../sync/background_alarm.dart';
import 'heartbeat_warning.dart';
import 'quiet_hours_setting.dart';
import 'setting_choice.dart';
import 'setting_labels.dart';

/// One account's sync settings: every row offers "Default (…)", which
/// inherits the app-wide value in [defaults], or a value of its own.
///
/// Edits a draft: [overrides] maps a key to its stored string (`'1'`/`'0'`,
/// minutes), and [onChanged] reports `''` for "use the default".
class AccountSyncSettings extends StatelessWidget {
  const AccountSyncSettings({
    super.key,
    required this.defaults,
    required this.intervals,
    required this.overrides,
    required this.onChanged,
    required this.showPush,
    this.frequentHeartbeatSecs,
  });

  final AppSettings defaults;

  /// The check intervals the core offers, in minutes.
  final List<int> intervals;
  final Map<String, String> overrides;
  final void Function(String key, String value) onChanged;

  /// Push is an Android background mechanism; elsewhere the row is hidden.
  final bool showPush;

  /// The server's observed IDLE heartbeat gap when it is frequent (see
  /// [AccountSettings.frequentHeartbeatSecs]); shown as a hint while the
  /// account uses push.
  final int? frequentHeartbeatSecs;

  static String _interval(int minutes) =>
      SettingLabels.of(SettingKeys.syncInterval, minutes);

  static String _onOff(bool on) => on ? 'On' : 'Off';

  Widget _flag(String title, String key, bool fallback) =>
      SettingChoice<String>(
        title: title,
        value: overrides[key] ?? '',
        options: const ['', '1', '0'],
        label: (v) => switch (v) {
          '1' => 'On',
          '0' => 'Off',
          _ => 'Default (${_onOff(fallback)})',
        },
        onChanged: (v) => onChanged(key, v),
      );

  @override
  Widget build(BuildContext context) {
    final pushByDefault = defaults.backgroundScheduler == schedulerPush;
    final pushOverride = overrides[AccountSettingKeys.pushEnabled] ?? '';
    final usesPush = pushOverride.isEmpty ? pushByDefault : pushOverride == '1';
    final heartbeat = frequentHeartbeatSecs;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SettingChoice<String>(
          title: 'Check for new mail',
          value: overrides[SettingKeys.syncInterval] ?? '',
          options: ['', for (final m in intervals) '$m'],
          label: (v) => v.isEmpty
              ? 'Default (${_interval(defaults.syncIntervalMinutes)})'
              : _interval(int.parse(v)),
          onChanged: (v) => onChanged(SettingKeys.syncInterval, v),
        ),
        if (showPush)
          SettingChoice<String>(
            title: 'Push (IMAP IDLE)',
            value: overrides[AccountSettingKeys.pushEnabled] ?? '',
            options: const ['', '1', '0'],
            label: (v) => switch (v) {
              '1' => 'Push',
              '0' => 'Check at the interval',
              _ =>
                'Default (${pushByDefault ? 'push' : 'check at the interval'})',
            },
            onChanged: (v) => onChanged(AccountSettingKeys.pushEnabled, v),
            help:
                'Servers that send "still here" every few minutes wake the '
                'phone each time; checking at the interval saves battery.',
          ),
        if (showPush && usesPush && heartbeat != null)
          HeartbeatWarning(seconds: heartbeat),
        QuietHoursSetting(
          defaults: defaults,
          overrides: overrides,
          onChanged: onChanged,
        ),
        _flag(
          'Save a copy of sent mail in Sent',
          SettingKeys.sentCopy,
          defaults.sentCopy,
        ),
        _flag(
          'Suggest recipients from sent mail',
          SettingKeys.collectContacts,
          defaults.collectContacts,
        ),
        _flag(
          'Show notifications for new mail',
          SettingKeys.notificationsEnabled,
          defaults.notificationsEnabled,
        ),
      ],
    );
  }
}
