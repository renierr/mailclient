import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/sync/background_power.dart';

void main() {
  // Local times, so the "same day" clock format is deterministic.
  final now = DateTime(2026, 3, 10, 12, 0);
  String at(DateTime t) => t.toUtc().toIso8601String();

  group('describeLastRun', () {
    test('no record yet', () {
      expect(describeLastRun(null, now), 'No background check has run yet.');
      expect(
        describeLastRun({'started_at': 'garbage'}, now),
        'No background check has run yet.',
      );
    });

    test('a fresh unfinished run is still running', () {
      final run = {'started_at': at(now.subtract(const Duration(minutes: 2)))};
      expect(
        describeLastRun(run, now),
        'Background check running since 11:58 (2 min ago).',
      );
    });

    test('an old unfinished run was stopped by Android', () {
      final run = {'started_at': at(now.subtract(const Duration(hours: 3)))};
      expect(describeLastRun(run, now), contains('did not finish'));
      expect(describeLastRun(run, now), contains('09:00 (3 h ago)'));
    });

    Map<String, dynamic> done(Map<String, dynamic> extra) => {
      'started_at': at(now.subtract(const Duration(minutes: 20))),
      'finished_at': at(now.subtract(const Duration(minutes: 19))),
      'skipped': false,
      'new': 0,
      'errors': <String>[],
      ...extra,
    };

    test('finished runs report their outcome', () {
      expect(
        describeLastRun(done({}), now),
        'Last background check 11:40 (20 min ago): no new mail.',
      );
      expect(
        describeLastRun(done({'new': 1}), now),
        endsWith('1 new message.'),
      );
      expect(
        describeLastRun(done({'new': 4}), now),
        endsWith('4 new messages.'),
      );
      expect(
        describeLastRun(done({'new': 1, 'outcome': 'notified (1)'}), now),
        endsWith('1 new message, notified (1).'),
      );
      expect(
        describeLastRun(done({'skipped': true}), now),
        endsWith('skipped, another sync was running.'),
      );
      expect(
        describeLastRun(
          done({
            'errors': ['connect: timed out', 'second'],
          }),
          now,
        ),
        endsWith('failed: connect: timed out'),
      );
    });

    test('an older day shows the date', () {
      final run = done({
        'started_at': at(DateTime(2026, 3, 7, 8, 5)),
        'finished_at': at(DateTime(2026, 3, 7, 8, 6)),
      });
      expect(describeLastRun(run, now), contains('7 Mar, 08:05 (3 days ago)'));
    });
  });

  group('describeRun', () {
    Map<String, dynamic> run(Map<String, dynamic> extra) => {
      'started_at': at(now.subtract(const Duration(minutes: 5))),
      'finished_at': at(now.subtract(const Duration(minutes: 4))),
      'trigger': 'alarm',
      'skipped': false,
      'new': 0,
      'errors': <String>[],
      ...extra,
    };

    test('names time, scheduler, result and outcome', () {
      expect(
        describeRun(run({'new': 2, 'outcome': 'notified (2)'}), now),
        '11:55 (5 min ago) · alarm · 2 new messages · notified (2)',
      );
      expect(
        describeRun(run({'skipped': true, 'trigger': ''}), now),
        '11:55 (5 min ago) · skipped, another sync was running',
      );
      expect(
        describeRun(
          run({
            'errors': ['dns'],
          }),
          now,
        ),
        endsWith('no new mail · failed: dns'),
      );
    });

    test('an unfinished old run was stopped', () {
      final r = run({
        'started_at': at(now.subtract(const Duration(hours: 1))),
        'finished_at': null,
      });
      expect(describeRun(r, now), endsWith('alarm · stopped by Android'));
    });
  });

  test('only limiting standby buckets get a label', () {
    for (final free in [0, 5, 10, 20]) {
      expect(limitingBucketLabel(free), isNull);
    }
    expect(limitingBucketLabel(40), 'rare');
    expect(limitingBucketLabel(45), 'restricted');
  });

  test('PowerStatus.fromMap tolerates partial and foreign maps', () {
    final full = PowerStatus.fromMap({
      'unrestricted': true,
      'standbyBucket': 10,
    });
    expect(full?.unrestricted, isTrue);
    expect(full?.standbyBucket, 10);
    expect(PowerStatus.fromMap({'unrestricted': false})?.standbyBucket, 0);
    expect(PowerStatus.fromMap({'standbyBucket': 10}), isNull);
    expect(PowerStatus.fromMap(null), isNull);
  });
}
