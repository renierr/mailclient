import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/models/settings.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/reader/reader_pane.dart';
import 'package:provider/provider.dart';

/// A core that holds one message and nothing else.
class _OneMessageCore implements MailCore {
  _OneMessageCore(this.body);

  final Map<String, dynamic> body;

  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  Future<MessageBody> message(int folderId, int uid) async =>
      MessageBody.fromJson(body);

  @override
  Future<MessageHeaders> messageHeaders(int folderId, int uid) =>
      Future.error(StateError('no headers in this test'));

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  // The wide shell puts the reader in a Row, which hands it a loose height.
  // The reader has to fill it anyway: it once sized itself to its empty
  // hover bubble and showed nothing at all.
  testWidgets('the reader fills a loosely constrained pane', (tester) async {
    final core = _OneMessageCore({
      'uid': 7,
      'subject': 'Plain',
      'from': 'someone@example.com',
      'body_text': 'Hello plain world',
      'is_html': false,
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: const MaterialApp(
          home: Scaffold(
            body: Row(children: [Expanded(child: ReaderPane())]),
          ),
        ),
      ),
    );
    await state.openMessage(7);
    await tester.pump();

    expect(tester.takeException(), isNull);
    final pane = tester.getSize(find.byType(ReaderPane));
    final screen = tester.getSize(find.byType(Scaffold));
    expect(pane.height, screen.height);
    expect(find.text('Hello plain world'), findsOneWidget);
  });
}
