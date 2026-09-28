import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/shell/undo_snack_bar_host.dart';
import 'package:provider/provider.dart';

/// A core whose deletes all land in Trash with an Undo handle.
class _TrashCore implements MailCore {
  int undone = 0;

  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  int undoGraceSecs() => 5;

  @override
  Future<MoveResult> deleteMessages(
    int accountId,
    int folderId,
    List<int> uids,
  ) async =>
      const MoveResult(batch: 'b1', label: 'Moved 1 to Trash', purging: false);

  @override
  Future<String> undoMove(String batch) async {
    undone++;
    return 'Restored 1';
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<MailState> _pump(WidgetTester tester, _TrashCore core) async {
  final state = MailState(core);
  await tester.pumpWidget(
    ChangeNotifierProvider.value(
      value: state,
      child: const MaterialApp(
        home: UndoSnackBarHost(child: Scaffold(body: SizedBox.expand())),
      ),
    ),
  );
  return state;
}

void main() {
  // Flutter keeps a SnackBar with an action up until it is dismissed; the
  // Undo bar has to leave with its grace period.
  testWidgets('the undo bar goes away when the grace period ends', (
    tester,
  ) async {
    final state = await _pump(tester, _TrashCore());
    await state.deleteMessages([1]);
    await tester.pumpAndSettle();
    expect(find.text('Undo'), findsOneWidget);

    await tester.pump(const Duration(seconds: 6));
    await tester.pumpAndSettle();
    expect(find.text('Undo'), findsNothing);
  });

  testWidgets('undoing from elsewhere takes the bar down', (tester) async {
    final core = _TrashCore();
    final state = await _pump(tester, core);
    await state.deleteMessages([1]);
    await tester.pumpAndSettle();
    expect(find.text('Undo'), findsOneWidget);

    await state.undoLast();
    await tester.pumpAndSettle();
    expect(core.undone, 1);
    expect(find.text('Undo'), findsNothing);
  });
}
