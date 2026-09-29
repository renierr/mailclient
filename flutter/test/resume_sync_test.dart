import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/state/mail_state.dart';

void main() {
  final now = DateTime(2026, 3, 10, 12, 0);

  test('resuming syncs when no sync ran yet or the last one is old', () {
    expect(shouldSyncOnResume(null, now), isTrue);
    expect(
      shouldSyncOnResume(now.subtract(const Duration(minutes: 5)), now),
      isTrue,
    );
  });

  test('a quick app switch right after a sync does not sync again', () {
    expect(
      shouldSyncOnResume(now.subtract(const Duration(seconds: 20)), now),
      isFalse,
    );
  });
}
