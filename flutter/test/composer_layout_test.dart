import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/composer/composer_dialog.dart';
import 'package:mailclient/src/ui/composer/composer_header_row.dart';
import 'package:provider/provider.dart';

/// Just enough core for a composer that is only looked at, never sent.
class _NoCore implements MailCore {
  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<void> _pumpComposer(
  WidgetTester tester, {
  required Size size,
  double textScale = 1.0,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ChangeNotifierProvider(
      create: (_) => MailState(_NoCore()),
      child: MaterialApp(
        home: MediaQuery(
          data: MediaQueryData(
            size: size,
            textScaler: TextScaler.linear(textScale),
          ),
          child: Scaffold(
            body: ComposerDialog(
              initial: const ComposerInitial(
                mode: ComposeMode.blank,
                to: 'someone@example.com',
                subject: 'Hello',
                body: 'Some **text** and ![logo.png](inline:1)',
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pump();
}

void main() {
  // RenderFlex overflow is a bug here (see AGENTS.md): the composer has to
  // hold at phone width, short heights and large text.
  for (final (name, size, scale) in [
    ('desktop', const Size(1000, 800), 1.0),
    ('phone', const Size(360, 640), 1.0),
    ('phone at 150%', const Size(360, 640), 1.5),
    ('short window', const Size(800, 400), 1.0),
  ]) {
    testWidgets('composer lays out without overflow ($name)', (tester) async {
      await _pumpComposer(tester, size: size, textScale: scale);
      expect(tester.takeException(), isNull);
      expect(find.text('From'), findsOneWidget);
      expect(find.text('Subject'), findsOneWidget);
    });
  }

  testWidgets('tapping a header label focuses its field', (tester) async {
    await _pumpComposer(tester, size: const Size(1000, 800));
    await tester.tap(find.text('Subject'));
    await tester.pump();
    final focused = FocusManager.instance.primaryFocus;
    final subject = tester.widget<EditableText>(
      find.descendant(
        of: find.ancestor(
          of: find.text('Subject'),
          matching: find.byType(ComposerHeaderRow),
        ),
        matching: find.byType(EditableText),
      ),
    );
    expect(focused, same(subject.focusNode));
  });

  testWidgets('Cc toggles its row on and off', (tester) async {
    await _pumpComposer(tester, size: const Size(1000, 800));
    Finder ccRow() => find.widgetWithText(ComposerHeaderRow, 'Cc');
    // The toggle itself is not a header row, so the row count is the test.
    expect(ccRow(), findsOneWidget); // the To row, which holds the toggle
    await tester.tap(find.widgetWithText(TextButton, 'Cc'));
    await tester.pump();
    expect(ccRow(), findsNWidgets(2));
    await tester.tap(find.widgetWithText(TextButton, 'Cc'));
    await tester.pump();
    expect(ccRow(), findsOneWidget);
  });
}
