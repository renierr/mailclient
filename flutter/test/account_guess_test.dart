import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/ui/accounts/account_setup_dialog.dart';

/// The core's guess rule, reduced: `imap.`/`smtp.` plus whatever follows
/// the `@` once it has a dot.
class _GuessCore implements MailCore {
  @override
  Map<String, dynamic> accountFormDefaults() => const {
    'imap_port': '993',
    'smtp_port': '465',
  };

  @override
  Map<String, dynamic> accountGuess(String email) {
    final at = email.indexOf('@');
    if (at < 1) return const {};
    final domain = email.substring(at + 1);
    if (!domain.contains('.') || domain.endsWith('.')) return const {};
    return {
      'imap_host': 'imap.$domain',
      'smtp_host': 'smtp.$domain',
      'imap_user': email,
    };
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

/// The first field with `label` (IMAP comes before SMTP in the form).
TextEditingController _field(WidgetTester tester, String label) => tester
    .widget<TextField>(
      find
          .descendant(
            of: find.widgetWithText(TextFormField, label),
            matching: find.byType(TextField),
          )
          .first,
    )
    .controller!;

void main() {
  setUp(() => MailCore.debugInstance = _GuessCore());

  Future<void> pump(WidgetTester tester) async {
    tester.view.physicalSize = const Size(1200, 2400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      const MaterialApp(home: AccountSetupDialog(fullscreen: true)),
    );
  }

  testWidgets('the host guess follows the address while it is typed', (
    tester,
  ) async {
    await pump(tester);
    final email = _field(tester, 'Email address');
    // Typed character by character: `a@example.c` is already a valid guess.
    for (final partial in ['a@example.c', 'a@example.co', 'a@example.com']) {
      email.text = partial;
      await tester.pump();
    }
    expect(_field(tester, 'Host').text, 'imap.example.com');
    expect(_field(tester, 'Username').text, 'a@example.com');
  });

  testWidgets('a host the user typed is never overwritten', (tester) async {
    await pump(tester);
    final email = _field(tester, 'Email address');
    email.text = 'a@example.c';
    await tester.pump();
    final host = _field(tester, 'Host');
    host.text = 'mail.example.org';
    email.text = 'a@example.com';
    await tester.pump();
    expect(host.text, 'mail.example.org');
    expect(_field(tester, 'Username').text, 'a@example.com');
  });
}
