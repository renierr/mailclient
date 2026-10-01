import 'package:flutter/material.dart';

import '../../models/account_settings.dart';
import 'setting_choice.dart';

/// One account's quiet hours: a switch and the two ends of the window.
///
/// Edits the same draft as `AccountSyncSettings`: [overrides] holds the
/// stored strings (`'1'`, `HH:MM`), and [onChanged] reports `''` to drop a
/// value again (quiet hours off).
class QuietHoursSetting extends StatelessWidget {
  const QuietHoursSetting({
    super.key,
    required this.overrides,
    required this.onChanged,
  });

  final Map<String, String> overrides;
  final void Function(String key, String value) onChanged;

  String _time(String key, String fallback) {
    final v = overrides[key] ?? '';
    return QuietTime.parse(v) == null ? fallback : v;
  }

  @override
  Widget build(BuildContext context) {
    final enabled = overrides[AccountSettingKeys.quietEnabled] == '1';
    final start = _time(
      AccountSettingKeys.quietStart,
      AccountSettingKeys.defaultQuietStart,
    );
    final end = _time(
      AccountSettingKeys.quietEnd,
      AccountSettingKeys.defaultQuietEnd,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          title: const Text('Quiet hours'),
          subtitle: SettingChoice.hint(
            context,
            'No background checks or push between these times. Opening '
            'the app or syncing by hand still checks.',
          ),
          value: enabled,
          onChanged: (v) =>
              onChanged(AccountSettingKeys.quietEnabled, v ? '1' : ''),
        ),
        if (enabled)
          Wrap(
            spacing: 8,
            runSpacing: 4,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              _timeButton(
                context,
                'From',
                AccountSettingKeys.quietStart,
                start,
              ),
              _timeButton(context, 'to', AccountSettingKeys.quietEnd, end),
            ],
          ),
      ],
    );
  }

  Widget _timeButton(
    BuildContext context,
    String label,
    String key,
    String value,
  ) {
    final time = QuietTime.parse(value)!;
    final shown = TimeOfDay(hour: time.hour, minute: time.minute);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label),
        const SizedBox(width: 6),
        OutlinedButton(
          onPressed: () async {
            final picked = await showTimePicker(
              context: context,
              initialTime: shown,
            );
            if (picked != null) {
              onChanged(key, QuietTime.format(picked.hour, picked.minute));
            }
          },
          child: Text(shown.format(context)),
        ),
      ],
    );
  }
}
