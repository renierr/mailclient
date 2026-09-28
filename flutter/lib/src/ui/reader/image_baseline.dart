import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// Gives an image in running text a baseline at its bottom edge, the way a
/// browser lays out `<img>`.
///
/// The HTML renderer places inline images on the text baseline, so the
/// paragraph asks the image for one. `RenderImage` has none and, in debug
/// builds, asserts instead of returning null. That assert fires mid-layout
/// and leaves the render tree in a state where every later layout fails,
/// which blanked the whole mail. Answering here keeps the question from
/// ever reaching the image.
class ImageBaseline extends SingleChildRenderObjectWidget {
  const ImageBaseline({super.key, required Widget super.child});

  @override
  RenderObject createRenderObject(BuildContext context) =>
      RenderImageBaseline();
}

/// Render side of [ImageBaseline].
class RenderImageBaseline extends RenderProxyBox {
  @override
  double? computeDistanceToActualBaseline(TextBaseline baseline) =>
      hasSize ? size.height : null;

  @override
  double? computeDryBaseline(
    BoxConstraints constraints,
    TextBaseline baseline,
  ) => getDryLayout(constraints).height;
}
