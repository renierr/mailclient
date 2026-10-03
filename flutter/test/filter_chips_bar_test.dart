import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/message_list/filter_chips_bar.dart';
import 'package:provider/provider.dart';

class _FakeCore implements MailCore {
  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  testWidgets('FilterChipsBar toggles quick filters and shows clear action', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 640);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final core = _FakeCore();
    MailCore.debugInstance = core;
    final state = MailState(core);

    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: const MaterialApp(
          home: Scaffold(body: FilterChipsBar()),
        ),
      ),
    );

    // Initial state: all chips unselected, Clear not visible
    expect(find.text('Unread'), findsOneWidget);
    expect(find.text('Starred'), findsOneWidget);
    expect(find.text('Attachments'), findsOneWidget);
    expect(find.text('Clear'), findsNothing);
    expect(tester.takeException(), isNull);

    // Tap Unread chip
    await tester.tap(find.text('Unread'));
    await tester.pump();
    expect(state.filterUnread, isTrue);
    expect(find.text('Clear'), findsOneWidget);

    // Tap Starred chip
    await tester.tap(find.text('Starred'));
    await tester.pump();
    expect(state.filterStarred, isTrue);

    // Tap Clear
    await tester.tap(find.text('Clear'));
    await tester.pump();
    expect(state.filterUnread, isFalse);
    expect(state.filterStarred, isFalse);
    expect(state.filterAttachments, isFalse);
    expect(find.text('Clear'), findsNothing);
  });
}
