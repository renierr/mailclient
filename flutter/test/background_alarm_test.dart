import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_alarm.dart';

void main() {
  group('normalizeScheduler', () {
    test('alarm and push pass through, case-insensitively', () {
      expect(normalizeScheduler('alarm'), schedulerAlarm);
      expect(normalizeScheduler(' ALARM '), schedulerAlarm);
      expect(normalizeScheduler('Push'), schedulerPush);
    });

    test('anything else falls back to WorkManager', () {
      expect(normalizeScheduler('workmanager'), schedulerWorkmanager);
      expect(normalizeScheduler(null), schedulerWorkmanager);
      expect(normalizeScheduler(''), schedulerWorkmanager);
      expect(normalizeScheduler('nonsense'), schedulerWorkmanager);
    });
  });
}
