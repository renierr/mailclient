import 'package:flutter_rust_bridge/flutter_rust_bridge.dart'
    show AnyhowException;
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/state/mail_state.dart';

/// A core whose one message can vanish, as when a sync learns it was moved
/// on another device.
class _VanishingCore implements MailCore {
  bool gone = false;

  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  Future<MessageBody> message(int folderId, int uid) async {
    if (gone) throw AnyhowException('message uid $uid not found');
    return MessageBody.fromJson({'uid': uid, 'subject': 'Hello'});
  }

  @override
  Future<MoveResult> deleteMessages(
    int accountId,
    int folderId,
    List<int> uids,
  ) async => throw AnyhowException(
    'message is no longer available\n\nStack backtrace:\n   0: <unknown>',
  );

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('trashing a message that is already gone closes the reader', () async {
    final core = _VanishingCore();
    final state = MailState(core);
    await state.openMessage(7);
    expect(state.openUid, 7);

    core.gone = true;
    await state.deleteMessages([7]);

    expect(state.openUid, -1);
    expect(state.status, 'The message was moved or deleted elsewhere');
  });

  test('a message missing from the cache does not open', () async {
    final state = MailState(_VanishingCore()..gone = true);
    await state.openMessage(7);
    expect(state.openUid, -1);
  });

  test('core errors lose the Rust backtrace', () {
    expect(
      coreErrorText(
        AnyhowException('message is no longer available\n\nStack backtrace:'),
      ),
      'message is no longer available',
    );
    expect(coreErrorText(Exception('offline')), 'offline');
  });
}
