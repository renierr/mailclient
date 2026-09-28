import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/reader/image_baseline.dart';

// A 1x1 transparent PNG.
final _png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
);

/// An image in running text inside a parent that asks for intrinsic sizes,
/// as mail layout tables do.
Widget _inlineImage(Widget image) => MaterialApp(
  home: Scaffold(
    body: IntrinsicWidth(
      child: Text.rich(
        TextSpan(
          children: [
            const TextSpan(text: 'before '),
            WidgetSpan(
              alignment: PlaceholderAlignment.baseline,
              baseline: TextBaseline.alphabetic,
              child: image,
            ),
          ],
        ),
      ),
    ),
  ),
);

void main() {
  testWidgets('a bare image in text fails the intrinsic pass', (tester) async {
    // Guards the premise: once Flutter gives RenderImage a dry baseline,
    // this fails and ImageBaseline can go.
    await tester.pumpWidget(_inlineImage(Image.memory(_png)));
    expect(tester.takeException(), isNotNull);
  });

  testWidgets('ImageBaseline lays the same image out', (tester) async {
    await tester.pumpWidget(
      _inlineImage(
        ImageBaseline(child: Image.memory(_png, width: 20, height: 10)),
      ),
    );
    expect(tester.takeException(), isNull);
    final box = tester.renderObject<RenderImageBaseline>(
      find.byType(ImageBaseline),
    );
    expect(
      box.getDryBaseline(const BoxConstraints(), TextBaseline.alphabetic),
      10,
    );
  });
}
