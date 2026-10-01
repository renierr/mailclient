import 'package:flutter/material.dart';

import '../../models/account_settings.dart';

/// Hint under an account's push setting when its server sends IDLE
/// heartbeats often: each one wakes the phone, so polling may save battery.
class HeartbeatWarning extends StatelessWidget {
  const HeartbeatWarning({super.key, required this.seconds});

  /// Observed gap between the server's heartbeats.
  final int seconds;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.battery_alert_outlined, size: 18, color: scheme.tertiary),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              'This server sends "still here" '
              '${AccountSettings.describeGap(seconds)} while push waits, '
              'waking the phone each time. If battery matters, set Push to '
              '"Check at the interval".',
              style: Theme.of(context).textTheme.bodySmall
                  ?.copyWith(color: scheme.onSurfaceVariant),
            ),
          ),
        ],
      ),
    );
  }
}
