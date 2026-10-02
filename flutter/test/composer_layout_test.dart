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
  ({String local, String domain}) senderParts(String address) {
    final at = address.lastIndexOf('@');
    return at < 0
        ? (local: address, domain: '')
        : (local: address.substring(0, at), domain: address.substring(at));
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

const _blank = ComposerInitial(
  mode: ComposeMode.blank,
  to: 'someone@example.com',
  subject: 'Hello',
  body: 'Some **text** and ![logo.png](inline:1)',
);

const _reply = ComposerInitial(
  mode: ComposeMode.reply,
  to: 'someone@example.com',
  subject: 'Re: Hello',
  body: 'Thanks',
  quoteHtml:
      '<p>On 2026-09-12 13:50, someone@example.com wrote:</p>'
      '<blockquote><p>A rather long original line that has to wrap at '
      'phone width instead of pushing the card off screen.</p></blockquote>',
);

Future<void> _pumpComposer(
  WidgetTester tester, {
  required Size size,
  double textScale = 1.0,
  ComposerInitial initial = _blank,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  final core = _NoCore();
  MailCore.debugInstance = core;
  await tester.pumpWidget(
    ChangeNotifierProvider(
      create: (_) => MailState(core),
      child: MaterialApp(
        home: MediaQuery(
          data: MediaQueryData(
            size: size,
            textScaler: TextScaler.linear(textScale),
          ),
          child: Scaffold(body: ComposerDialog(initial: initial)),
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

  for (final (name, size, scale) in [
    ('phone', const Size(360, 640), 1.0),
    ('phone at 150%', const Size(360, 640), 1.5),
  ]) {
    testWidgets('a reply quote opens without overflow ($name)', (tester) async {
      await _pumpComposer(
        tester,
        size: size,
        textScale: scale,
        initial: _reply,
      );
      await tester.ensureVisible(find.text('Quoted original'));
      await tester.tap(find.text('Quoted original'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.textContaining('wrote:', findRichText: true), findsOneWidget);
    });
  }

  testWidgets('a quote can be left out', (tester) async {
    await _pumpComposer(tester, size: const Size(1000, 800), initial: _reply);
    expect(find.text('Quoted original'), findsOneWidget);
    await tester.ensureVisible(find.byTooltip('Leave out'));
    await tester.tap(find.byTooltip('Leave out'));
    await tester.pump();
    expect(find.text('Quoted original'), findsNothing);
  });

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
