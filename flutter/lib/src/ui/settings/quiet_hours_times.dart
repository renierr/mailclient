import 'package:flutter/material.dart';

import '../../ffi/mail_core.dart';
import '../../models/settings.dart';

/// The two ends of a quiet-hours window, each a button that opens a time
/// picker. [start] and [end] are stored `HH:MM` strings; an unreadable one
/// shows the core's default. Used app-wide and per account.
class QuietHoursTimes extends StatelessWidget {
  const QuietHoursTimes({
    super.key,
    required this.start,
    required this.end,
    required this.onStart,
    required this.onEnd,
  });

  final String start;
  final String end;
  final ValueChanged<String> onStart;
  final ValueChanged<String> onEnd;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: 8,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        _timeButton(context, 'From', start, SettingKeys.quietStart, onStart),
        _timeButton(context, 'to', end, SettingKeys.quietEnd, onEnd),
      ],
    );
  }

  Widget _timeButton(
    BuildContext context,
    String label,
    String value,
    String key,
    ValueChanged<String> onChanged,
  ) {
    final core = MailCore.instance;
    final time =
        core.quietTime(value) ??
        core.quietTime(core.settingChoices.defaultOf<String>(key) ?? '') ??
        (hour: 0, minute: 0);
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
              onChanged(core.quietTimeAt(picked.hour, picked.minute));
            }
          },
          child: Text(shown.format(context)),
        ),
      ],
    );
  }
}
