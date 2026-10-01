import 'package:flutter/material.dart';

import '../dialogs/mail_dialog.dart';

/// One settings row: a title (and optional hint) with a dropdown of
/// [options]. Label above the control on narrow layouts, beside it on wide
/// ones.
class SettingChoice<T> extends StatelessWidget {
  const SettingChoice({
    super.key,
    required this.title,
    required this.value,
    required this.options,
    required this.label,
    required this.onChanged,
    this.help,
    this.enabled = true,
  });

  final String title;
  final T value;
  final List<T> options;
  final String Function(T) label;
  final ValueChanged<T> onChanged;
  final String? help;
  final bool enabled;

  /// Secondary hint under a setting: small and muted, so the labels carry
  /// the page and the hints stay out of the way.
  static Widget hint(BuildContext context, String text) => Text(
    text,
    style: Theme.of(context).textTheme.bodySmall
        ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant),
  );

  @override
  Widget build(BuildContext context) {
    final control = DropdownButton<T>(
      value: options.contains(value) ? value : options.first,
      // Expanded: a long option label ellipsizes instead of overflowing the
      // dropdown at 360px / large text.
      isExpanded: true,
      items: [
        for (final o in options)
          DropdownMenuItem(
            value: o,
            child: Text(label(o), overflow: TextOverflow.ellipsis),
          ),
      ],
      onChanged: enabled ? (v) => v != null ? onChanged(v) : null : null,
    );
    final labels = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [Text(title), if (help != null) hint(context, help!)],
    );
    // Label above the control on narrow/zoomed layouts: label-beside-control
    // rows squeeze the dropdown (or the label) to zero there. Wide screens
    // keep the compact side-by-side form.
    if (MailDialog.isNarrow(context)) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [labels, const SizedBox(height: 2), control],
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Expanded(child: labels),
          const SizedBox(width: 8),
          // Flexible: the button caps at the remaining width and ellipsizes
          // instead of overflowing the row on narrow dialogs.
          Flexible(child: control),
        ],
      ),
    );
  }
}
