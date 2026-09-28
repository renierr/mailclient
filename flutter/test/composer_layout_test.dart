import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/composer/composer_dialog.dart';
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
  bool fullscreen = false,
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
              fullscreen: fullscreen,
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
  for (final (name, size, scale, fullscreen) in [
    ('desktop', const Size(1000, 800), 1.0, false),
    ('narrow dialog', const Size(360, 640), 1.0, false),
    ('phone page at 150%', const Size(360, 640), 1.5, true),
    ('short window', const Size(800, 400), 1.0, false),
  ]) {
    testWidgets('composer lays out without overflow ($name)', (tester) async {
      await _pumpComposer(
        tester,
        size: size,
        textScale: scale,
        fullscreen: fullscreen,
      );
      expect(tester.takeException(), isNull);
      expect(find.text('From'), findsOneWidget);
      expect(find.text('Subject'), findsOneWidget);
    });
  }
}
