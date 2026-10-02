import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/models/settings.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/reader/mail_html_view.dart';
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

  // The reader document is the core's; these stand-ins only keep the
  // widgets building.
  @override
  ReaderPaint readerPaint(bool colored, bool dark, bool keepOriginal) =>
      !colored
      ? ReaderPaint.theme
      : dark && !keepOriginal
      ? ReaderPaint.darkened
      : ReaderPaint.original;

  @override
  ReaderPalette readerPalette(ReaderPaint paint, ReaderPalette theme) => theme;

  @override
  String readerBody(String body, ReaderPaint paint, {bool fit = false}) => body;

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

  // One scrolling page: header and attachments move with the body, and
  // nothing overflows at phone width.
  testWidgets('header and attachments scroll with the body on a phone', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 640);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final core = _OneMessageCore({
      'uid': 8,
      'subject': 'A subject long enough to wrap onto a second line here',
      'from': 'someone@example.com',
      'body_text': List.filled(200, 'line of body text').join('\n'),
      'is_html': false,
      'attachments': [
        {'id': 1, 'filename': 'a-rather-long-file-name.pdf', 'size': 2048},
        {'id': 2, 'filename': 'b.txt', 'size': 4},
      ],
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: const MaterialApp(home: Scaffold(body: ReaderPane())),
      ),
    );
    await state.openMessage(8);
    await tester.pump();

    expect(tester.takeException(), isNull);
    expect(find.text('2 attachments'), findsOneWidget);
    final subject = find.textContaining('A subject long enough');
    final before = tester.getTopLeft(subject).dy;
    await tester.drag(find.byType(CustomScrollView), const Offset(0, -300));
    await tester.pump();
    expect(tester.getTopLeft(subject).dy, lessThan(before - 200));
  });

  // Original colours show the mail as sent: its own palette and its
  // original fixed-width layout, sideways scroll included. The toggle
  // flips MailHtmlView.fitWidths with it.
  testWidgets('original colours also restore the original width layout', (
    tester,
  ) async {
    final core = _OneMessageCore({
      'uid': 9,
      'subject': 'Newsletter',
      'from': 'news@example.com',
      'body_text': 'fallback',
      'is_html': true,
      'html_colored': true,
      'body_html': '<table width="600"><tr><td>hi</td></tr></table>',
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: MaterialApp(
          theme: ThemeData.dark(),
          home: const Scaffold(body: ReaderPane()),
        ),
      ),
    );
    await state.openMessage(9);
    await tester.pump();

    expect(tester.takeException(), isNull);
    MailHtmlView view() =>
        tester.widget<MailHtmlView>(find.byType(MailHtmlView));
    expect(view().fitWidths, isTrue);

    await tester.tap(find.byTooltip('Show original colours'));
    await tester.pump();
    expect(view().fitWidths, isFalse);

    await tester.tap(find.byTooltip('Darken to match the theme'));
    await tester.pump();
    expect(view().fitWidths, isTrue);
  });
}
