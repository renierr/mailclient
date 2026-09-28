import 'package:flutter/material.dart';

import '../../ffi/mail_core.dart';
import '../../sync/background_alarm.dart';
import '../../sync/background_power.dart';

/// Android-only Settings block: whether the background worker may run on
/// time (battery-optimisation exemption, standby bucket) and what the last
/// run did. Re-reads on resume, so returning from the system prompt shows
/// the new state without reopening Settings.
class BackgroundCheckStatus extends StatefulWidget {
  const BackgroundCheckStatus({super.key});

  @override
  State<BackgroundCheckStatus> createState() => _BackgroundCheckStatusState();
}

class _BackgroundCheckStatusState extends State<BackgroundCheckStatus>
    with WidgetsBindingObserver {
  PowerStatus? _power;
  Map<String, dynamic>? _lastRun;
  String _scheduler = schedulerWorkmanager;
  bool _exactAlarm = true;
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _load();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _load();
  }

  Future<void> _load() async {
    final power = await backgroundPowerStatus();
    Map<String, dynamic>? lastRun;
    var scheduler = schedulerWorkmanager;
    var exactAlarm = true;
    try {
      lastRun = await MailCore.instance.backgroundLastRun();
      scheduler = (await MailCore.instance.settings()).backgroundScheduler;
      exactAlarm = await exactAlarmPermitted();
    } catch (_) {
      lastRun = null;
    }
    if (!mounted) return;
    setState(() {
      _power = power;
      _lastRun = lastRun;
      _scheduler = scheduler;
      _exactAlarm = exactAlarm;
      _loaded = true;
    });
  }

  @override
  Widget build(BuildContext context) {
    if (!_loaded) return const SizedBox.shrink();
    final theme = Theme.of(context);
    final power = _power;
    final bucket = power == null
        ? null
        : limitingBucketLabel(power.standbyBucket);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Background checks', style: theme.textTheme.titleSmall),
        const SizedBox(height: 8),
        _line(
          context,
          _scheduler == schedulerAlarm
              ? Icons.alarm_outlined
              : Icons.battery_saver_outlined,
          _scheduler == schedulerAlarm
              ? 'On-time alarm: checks fire in standby too.'
              : 'Battery-saving worker: standby may delay checks until '
                    'the phone is unlocked.',
        ),
        if (_scheduler == schedulerAlarm && !_exactAlarm)
          _line(
            context,
            Icons.notification_important_outlined,
            'Exact alarms are not allowed: the alarm still fires in '
            'standby, just not at the exact minute.',
            error: true,
          ),
        if (_scheduler == schedulerAlarm && !_exactAlarm)
          Padding(
            padding: const EdgeInsets.only(top: 4, bottom: 4),
            child: FilledButton.tonalIcon(
              onPressed: requestExactAlarm,
              icon: const Icon(Icons.alarm_add_outlined),
              label: const Text('Allow exact alarms'),
            ),
          ),
        if (power != null)
          _line(
            context,
            power.unrestricted
                ? Icons.battery_full_outlined
                : Icons.battery_alert_outlined,
            power.unrestricted
                ? 'Battery use is unrestricted, so checks keep running '
                      'while the phone sleeps.'
                : 'Battery optimisation is on: while the phone sleeps, '
                      'Android may postpone checks by hours.',
            error: !power.unrestricted,
          ),
        if (power != null && !power.unrestricted && bucket != null)
          _line(
            context,
            Icons.hourglass_bottom_outlined,
            'Android rates the app as "$bucket", which limits checks '
            'further.',
            error: true,
          ),
        if (power != null && !power.unrestricted)
          Padding(
            padding: const EdgeInsets.only(top: 4, bottom: 4),
            child: FilledButton.tonalIcon(
              onPressed: requestUnrestrictedBackground,
              icon: const Icon(Icons.battery_charging_full_outlined),
              label: const Text('Allow background use'),
            ),
          ),
        _line(
          context,
          Icons.history_outlined,
          describeLastRun(_lastRun, DateTime.now()),
        ),
      ],
    );
  }

  Widget _line(
    BuildContext context,
    IconData icon,
    String text, {
    bool error = false,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final color = error ? scheme.error : scheme.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 18, color: color),
          const SizedBox(width: 8),
          Expanded(child: Text(text)),
        ],
      ),
    );
  }
}
