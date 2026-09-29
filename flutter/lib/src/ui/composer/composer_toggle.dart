import 'package:flutter/material.dart';

/// A show/hide toggle for an optional composer row (Cc, Bcc, Reply-To),
/// like the Qt composer's toggle buttons: tinted while its row is shown,
/// pressing again hides it.
class ComposerToggle extends StatelessWidget {
  const ComposerToggle({
    super.key,
    required this.label,
    required this.tooltip,
    required this.active,
    required this.onPressed,
    this.icon,
  });

  final String label;
  final String tooltip;
  final bool active;
  final VoidCallback onPressed;

  /// Shown instead of the label text, which then only names it for
  /// accessibility (the From line's Reply-To arrow).
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final style = TextButton.styleFrom(
      visualDensity: VisualDensity.compact,
      minimumSize: const Size(40, 36),
      padding: const EdgeInsets.symmetric(horizontal: 10),
      foregroundColor: active ? scheme.onSecondaryContainer : scheme.primary,
      backgroundColor: active ? scheme.secondaryContainer : null,
    );
    return Padding(
      padding: const EdgeInsets.only(left: 4),
      child: Tooltip(
        message: tooltip,
        child: Semantics(
          toggled: active,
          label: label,
          child: TextButton(
            style: style,
            onPressed: onPressed,
            child: icon == null ? Text(label) : Icon(icon, size: 18),
          ),
        ),
      ),
    );
  }
}
