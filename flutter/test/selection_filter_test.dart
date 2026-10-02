import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/state/mail_state.dart';

/// A core with one folder of three rows, newest first: 3 unread with an
/// attachment, 2 read, 1 unread.
class _FolderCore implements MailCore {
  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  Future<List<MessageSummary>> messages(
    int folderId, {
    int limit = 200,
    int offset = 0,
  }) async => [
    for (final (uid, unread, attach) in [
      (3, true, true),
      (2, false, false),
      (1, true, false),
    ])
      MessageSummary.fromJson({
        'uid': uid,
        'subject': 'Message $uid',
        'from': 'a@example.com',
        'unread': unread,
        'has_attachments': attach,
      }),
  ];

  @override
  Future<void> syncFolder(int accountId, int folderId) async {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<MailState> _folder() async {
  final state = MailState(_FolderCore());
  await state.selectFolder(5);
  return state;
}

void main() {
  test('select all takes only the rows a filter leaves visible', () async {
    final state = await _folder();
    state.setFilterAttachments(true);
    state.selectAllVisible();
    expect(state.selectedUids, {3});
  });

  test('invert and range stay inside the visible rows', () async {
    final state = await _folder();
    state.setFilterUnread(true);
    state.invertSelection();
    expect(state.selectedUids, {1, 3});

    state.exitSelectionMode();
    // The hidden read row 2 sits between 3 and 1 and stays out.
    state.selectRange(3, 1);
    expect(state.selectedUids, {1, 3});
  });

  test('turning a filter on drops selected rows it hides', () async {
    final state = await _folder();
    state.selectAllVisible();
    expect(state.selectedUids, {1, 2, 3});
    state.setFilterUnread(true);
    expect(state.selectedUids, {1, 3});
    state.setFilterAttachments(true);
    expect(state.selectedUids, {3});
    expect(state.selectionMode, isTrue);
  });
}
