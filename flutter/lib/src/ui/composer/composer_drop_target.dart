import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';

/// Files dragged onto the composer from the desktop (or, on Android, from
/// another app in split screen). Hands the dropped paths up and shows a
/// highlight while something hovers; what each file becomes is the
/// composer's decision.
class ComposerDropTarget extends StatefulWidget {
  const ComposerDropTarget({
    super.key,
    required this.child,
    required this.onFiles,
  });

  final Widget child;

  /// Local paths of the dropped files. Items without a path (some virtual
  /// sources) are left out.
  final void Function(List<({String path, String name})> files) onFiles;

  @override
  State<ComposerDropTarget> createState() => _ComposerDropTargetState();
}

class _ComposerDropTargetState extends State<ComposerDropTarget> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return DropTarget(
      onDragEntered: (_) => setState(() => _hovering = true),
      onDragExited: (_) => setState(() => _hovering = false),
      onDragDone: (details) {
        setState(() => _hovering = false);
        final files = [
          for (final f in details.files)
            if (f.path.isNotEmpty) (path: f.path, name: f.name),
        ];
        if (files.isNotEmpty) widget.onFiles(files);
      },
      child: DecoratedBox(
        position: DecorationPosition.foreground,
        decoration: BoxDecoration(
          border: _hovering
              ? Border.all(color: scheme.primary, width: 2)
              : null,
          borderRadius: BorderRadius.circular(8),
          color: _hovering ? scheme.primary.withValues(alpha: 0.06) : null,
        ),
        child: widget.child,
      ),
    );
  }
}

/// Where dropped images should go.
enum ImagePlacement { inline, attach }

/// Ask whether [count] dropped images go inside the text or attach.
/// `null` when the user cancels.
Future<ImagePlacement?> askImagePlacement(BuildContext context, int count) {
  return showDialog<ImagePlacement>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(count == 1 ? 'Add image' : 'Add $count images'),
      content: const Text(
        'Inline images show inside the message text. Attached ones arrive as separate files.',
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        OutlinedButton(
          onPressed: () => Navigator.pop(context, ImagePlacement.attach),
          child: const Text('Attach'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, ImagePlacement.inline),
          child: const Text('Insert inline'),
        ),
      ],
    ),
  );
}
