import 'package:flutter/material.dart';

import '../../models/settings.dart';
import 'quiet_hours_times.dart';
import 'setting_choice.dart';

/// One account's quiet hours: the app-wide window ("Default"), a window of
/// its own ("On", with its own times) or none ("Off").
///
/// Edits the same draft as `AccountSyncSettings`: [overrides] holds the
/// stored strings (`'1'`/`'0'`, `HH:MM`), and [onChanged] reports `''` to
/// inherit again. Own times only count with "On", so leaving it drops them.
class QuietHoursSetting extends StatelessWidget {
  const QuietHoursSetting({
    super.key,
    required this.defaults,
    required this.overrides,
    required this.onChanged,
  });

  final AppSettings defaults;
  final Map<String, String> overrides;
  final void Function(String key, String value) onChanged;

  @override
  Widget build(BuildContext context) {
    final choice = overrides[SettingKeys.quietEnabled] ?? '';
    final inherited = defaults.quietEnabled
        ? '${defaults.quietStart}–${defaults.quietEnd}'
        : 'Off';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SettingChoice<String>(
          title: 'Quiet hours',
          value: choice,
          options: const ['', '1', '0'],
          label: (v) => switch (v) {
            '1' => 'On',
            '0' => 'Off',
            _ => 'Default ($inherited)',
          },
          onChanged: (v) {
            onChanged(SettingKeys.quietEnabled, v);
            if (v != '1') {
              onChanged(SettingKeys.quietStart, '');
              onChanged(SettingKeys.quietEnd, '');
            }
          },
          help:
              'No background checks or push between these times. Opening '
              'the app or syncing by hand still checks.',
        ),
        if (choice == '1')
          QuietHoursTimes(
            start: overrides[SettingKeys.quietStart] ?? defaults.quietStart,
            end: overrides[SettingKeys.quietEnd] ?? defaults.quietEnd,
            onStart: (v) => onChanged(SettingKeys.quietStart, v),
            onEnd: (v) => onChanged(SettingKeys.quietEnd, v),
          ),
      ],
    );
  }
}
