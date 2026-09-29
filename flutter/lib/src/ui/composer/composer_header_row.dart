import 'package:flutter/material.dart';

import '../dialogs/mail_dialog.dart';

/// One line of the composer header — From, To, Subject — laid out like the
/// Qt composer: a muted label in a fixed column, a boxed field beside it, so
/// it is obvious where to tap. Tapping the label focuses the field.
///
/// Narrow layouts put the label above the field, where a side label would
/// squeeze the field unreadably thin.
class ComposerHeaderRow extends StatelessWidget {
  const ComposerHeaderRow({
    super.key,
    required this.label,
    required this.child,
    this.trailing,
  });

  final String label;
  final Widget child;

  /// Small actions at the end of the line (the To line's Cc/Bcc toggles).
  final Widget? trailing;

  /// Boxed decoration for a field inside a header row.
  static InputDecoration field({String? hint, Widget? suffix}) =>
      InputDecoration(
        hintText: hint,
        suffix: suffix,
        border: const OutlineInputBorder(),
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 10,
          vertical: 10,
        ),
      );

  /// Focus the first text field in this row. Walking the element tree keeps
  /// it working for fields whose focus node lives inside another widget
  /// (the recipient autocomplete owns its own).
  static void _focusField(BuildContext context) {
    EditableText? found;
    void visit(Element e) {
      if (found != null) return;
      final w = e.widget;
      if (w is EditableText) {
        found = w;
        return;
      }
      e.visitChildren(visit);
    }

    context.visitChildElements(visit);
    found?.focusNode.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final labelText = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => _focusField(context),
      child: Text(
        label,
        style: theme.textTheme.bodyMedium?.copyWith(
          color: theme.colorScheme.onSurfaceVariant,
        ),
      ),
    );
    final line = Row(
      children: [
        Expanded(child: child),
        ?trailing,
      ],
    );
    final narrow = MailDialog.isNarrow(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: narrow
          ? Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: labelText,
                ),
                line,
              ],
            )
          : Row(
              children: [
                SizedBox(
                  // Fits "Reply-To", scaled with the text.
                  width: MediaQuery.textScalerOf(context).scale(80),
                  child: labelText,
                ),
                Expanded(child: line),
              ],
            ),
    );
  }
}
