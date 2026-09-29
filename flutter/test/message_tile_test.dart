import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/ui/message_list/message_list_widgets.dart';

MessageSummary row({String fromName = '', String from = 'a@example.com'}) =>
    MessageSummary(
      uid: 7,
      subject: 'Hello',
      from: from,
      fromName: fromName,
      date: '12:30',
      snippet: 'snip',
      unread: true,
      starred: false,
      hasAttachments: false,
    );

Future<void> pumpTile(WidgetTester tester, MessageSummary m) =>
    tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MessageTile(
            message: m,
            selected: false,
            checked: false,
            selectionMode: false,
            compact: false,
            onTap: () {},
            onToggle: () {},
          ),
        ),
      ),
    );

void main() {
  testWidgets('the row shows the sent name and the date top right', (
    tester,
  ) async {
    await pumpTile(
      tester,
      row(fromName: 'Jürgen Müller', from: 'juergen@example.com'),
    );

    expect(find.text('Jürgen Müller'), findsOneWidget);
    expect(find.text('12:30'), findsOneWidget);
    expect(find.text('juergen@example.com'), findsNothing);
    expect(find.text('Hello'), findsOneWidget);
    expect(find.byTooltip('Message actions'), findsOneWidget);
  });

  testWidgets('without a name the row falls back to the address', (
    tester,
  ) async {
    await pumpTile(tester, row());

    expect(find.text('a@example.com'), findsOneWidget);
  });
}
