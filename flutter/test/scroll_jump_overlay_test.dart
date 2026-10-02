import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/message_list/scroll_jump_overlay.dart';

Widget _list(int rows) => MaterialApp(
  home: Scaffold(
    body: ScrollJumpOverlay(
      builder: (controller) => ListView.builder(
        controller: controller,
        itemCount: rows,
        itemBuilder: (_, i) => SizedBox(height: 50, child: Text('row $i')),
      ),
    ),
  ),
);

void main() {
  testWidgets('a short list shows no jump buttons', (tester) async {
    await tester.pumpWidget(_list(5));
    await tester.pumpAndSettle();
    expect(find.byTooltip('Jump to top'), findsNothing);
    expect(find.byTooltip('Jump to bottom'), findsNothing);
  });

  testWidgets('jumps to the bottom and back to the top', (tester) async {
    await tester.pumpWidget(_list(500));
    await tester.pumpAndSettle();
    // At the top only the way down is offered.
    expect(find.byTooltip('Jump to top'), findsNothing);
    await tester.tap(find.byTooltip('Jump to bottom'));
    await tester.pumpAndSettle();
    expect(find.text('row 499'), findsOneWidget);
    expect(find.byTooltip('Jump to bottom'), findsNothing);

    await tester.tap(find.byTooltip('Jump to top'));
    await tester.pumpAndSettle();
    expect(find.text('row 0'), findsOneWidget);
    expect(find.byTooltip('Jump to top'), findsNothing);
  });
}
