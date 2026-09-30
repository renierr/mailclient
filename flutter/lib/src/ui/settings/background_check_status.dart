import 'package:flutter/material.dart';

import '../../ffi/mail_core.dart';
import '../../sync/background_alarm.dart';
import '../../sync/background_power.dart';

/// Android-only Settings block: whether the background worker may run on
/// time (battery-optimisation exemption, standby bucket), what the last
/// run did, and the recent runs for diagnosing missed notifications. Re-reads on resume, so returning from the system prompt shows
/// the new state without reopening Settings.
class BackgroundCheckStatus extends StatefulWidget {
  const BackgroundCheckStatus({super.key});

  @override
  State<BackgroundCheckStatus> createState() => _BackgroundCheckStatusState();
}

class _BackgroundCheckStatusState extends State<BackgroundCheckStatus>
    with WidgetsBindingObserver {
  PowerStatus? _power;
  List<Map<String, dynamic>> _runs = const [];
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
    var runs = const <Map<String, dynamic>>[];
    var scheduler = schedulerWorkmanager;
    var exactAlarm = true;
    try {
      runs = await MailCore.instance.backgroundRunHistory();
      scheduler = (await MailCore.instance.settings()).backgroundScheduler;
      exactAlarm = await exactAlarmPermitted();
    } catch (_) {
      runs = const [];
    }
    if (!mounted) return;
    setState(() {
      _power = power;
      _runs = runs;
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
          switch (_scheduler) {
            schedulerAlarm => Icons.alarm_outlined,
            schedulerPush => Icons.bolt_outlined,
            _ => Icons.battery_saver_outlined,
          },
          switch (_scheduler) {
            schedulerAlarm => 'On-time alarm: checks fire in standby too.',
            schedulerPush =>
              'Push: the server announces new mail as it arrives. The '
                  '"Mail monitor" notification Android requires for it can '
                  'be turned off in the system notification settings.',
            _ =>
              'Battery-saving worker: standby may delay checks until '
                  'the phone is unlocked.',
          },
        ),
        if (_scheduler != schedulerWorkmanager && !_exactAlarm)
          _line(
            context,
            Icons.notification_important_outlined,
            _scheduler == schedulerPush
                ? 'Exact alarms are not allowed: in standby the push '
                      'keep-alive may run late and connections drop.'
                : 'Exact alarms are not allowed: the alarm still fires in '
                      'standby, just not at the exact minute.',
            error: true,
          ),
        if (_scheduler != schedulerWorkmanager && !_exactAlarm)
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
          describeLastRun(_runs.firstOrNull, DateTime.now()),
        ),
        if (_runs.length > 1) _history(context),
      ],
    );
  }

  /// The recent runs, collapsed: when a notification went missing, this
  /// shows whether a check ran at all, which scheduler ran it, and what it
  /// did with the result.
  Widget _history(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.textTheme.bodySmall?.copyWith(
      color: theme.colorScheme.onSurfaceVariant,
    );
    final now = DateTime.now();
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      childrenPadding: const EdgeInsets.only(left: 26, bottom: 8),
      expandedCrossAxisAlignment: CrossAxisAlignment.start,
      title: Text('Recent checks', style: theme.textTheme.bodyMedium),
      children: [
        for (final run in _runs)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Text(describeRun(run, now), style: muted),
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
