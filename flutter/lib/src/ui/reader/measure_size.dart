import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// Reports its child's laid-out size whenever it changes, after layout.
///
/// The Android reader needs the header's height to reserve the same room
/// at the top of the mail document the header overlays.
class MeasureSize extends SingleChildRenderObjectWidget {
  const MeasureSize({super.key, required this.onChange, super.child});

  final ValueChanged<Size> onChange;

  @override
  RenderObject createRenderObject(BuildContext context) =>
      MeasureSizeRender(onChange);

  @override
  void updateRenderObject(
    BuildContext context,
    MeasureSizeRender renderObject,
  ) {
    renderObject.onChange = onChange;
  }
}

class MeasureSizeRender extends RenderProxyBox {
  MeasureSizeRender(this.onChange);

  ValueChanged<Size> onChange;
  Size? _last;

  @override
  void performLayout() {
    super.performLayout();
    if (size == _last) return;
    _last = size;
    // Not during layout: the callback may well call setState.
    WidgetsBinding.instance.addPostFrameCallback((_) => onChange(size));
  }
}
