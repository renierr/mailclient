import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/reader/attachment_card.dart';

/// A core whose attachment never has bytes and whose downloads fail, or
/// never finish when [finishes] is false.
class _DownloadCore implements MailCore {
  _DownloadCore({this.finishes = true});

  final bool finishes;
  final _events = StreamController<JobEvent>.broadcast();
  int downloads = 0;

  @override
  Stream<JobEvent> jobEvents() => _events.stream;

  @override
  Future<List<int>?> attachmentBytes(int attachmentId) async => null;

  @override
  Future<void> downloadAttachments(int accountId, int folderId, int uid) async {
    downloads++;
    if (!finishes) return;
    scheduleMicrotask(
      () => _events.add(
        JobEvent(
          kind: 'Attachments',
          phase: JobPhase.finished,
          status: 'connection refused',
          ok: false,
          accountId: -1,
          folderId: -1,
        ),
      ),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  final message = MessageBody.fromJson({'uid': 7, 'subject': 'Files'});

  test('a failed download is reported once, not retried', () async {
    final core = _DownloadCore();
    MailCore.debugInstance = core;
    final state = MailState(core);
    await expectLater(
      attachmentBytes(state, message, 1),
      throwsA(
        isA<AttachmentDownloadFailed>().having(
          (e) => e.message,
          'message',
          'connection refused',
        ),
      ),
    );
    expect(core.downloads, 1);
  });

  test('a download that never finishes gives up after the wait', () async {
    final core = _DownloadCore(finishes: false);
    MailCore.debugInstance = core;
    final state = MailState(core);
    final bytes = await attachmentBytes(
      state,
      message,
      1,
      wait: const Duration(milliseconds: 10),
    );
    expect(bytes, isNull);
    expect(core.downloads, 1);
  });
}
