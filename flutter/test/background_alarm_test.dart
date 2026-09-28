import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_alarm.dart';

void main() {
  group('normalizeScheduler', () {
    test('alarm passes through, case-insensitively', () {
      expect(normalizeScheduler('alarm'), schedulerAlarm);
      expect(normalizeScheduler(' ALARM '), schedulerAlarm);
    });

    test('anything else falls back to WorkManager', () {
      expect(normalizeScheduler('workmanager'), schedulerWorkmanager);
      expect(normalizeScheduler(null), schedulerWorkmanager);
      expect(normalizeScheduler(''), schedulerWorkmanager);
      expect(normalizeScheduler('nonsense'), schedulerWorkmanager);
    });
  });

  group('effectiveAlarmMinutes', () {
    test('zero and negative disable the alarm', () {
      expect(effectiveAlarmMinutes(0), 0);
      expect(effectiveAlarmMinutes(-5), 0);
    });

    test('short intervals pass through — no 15-minute floor', () {
      expect(effectiveAlarmMinutes(5), 5);
      expect(effectiveAlarmMinutes(10), 10);
      expect(effectiveAlarmMinutes(15), 15);
      expect(effectiveAlarmMinutes(60), 60);
    });
  });
}
