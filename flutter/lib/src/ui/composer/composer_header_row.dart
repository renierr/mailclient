import 'package:flutter/material.dart';

import '../dialogs/mail_dialog.dart';

/// One line of the composer header — From, To, Subject — laid out like a
/// mail header: a muted label, a borderless field, a divider below.
///
/// Wide layouts put the labels in a column of their own so the fields line
/// up; narrow ones put the label above the field, where a side label would
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

  /// Small actions at the end of the line (the To line's Cc/Bcc).
  final Widget? trailing;

  /// Borderless decoration for a field inside a header row.
  static InputDecoration field({String? hint, Widget? suffix}) =>
      InputDecoration(
        hintText: hint,
        suffix: suffix,
        border: InputBorder.none,
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(vertical: 12),
      );

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final labelText = Text(
      label,
      style: theme.textTheme.bodyMedium?.copyWith(
        color: theme.colorScheme.onSurfaceVariant,
      ),
    );
    final line = Row(
      children: [
        Expanded(child: child),
        ?trailing,
      ],
    );
    final narrow = MailDialog.isNarrow(context);
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: theme.dividerColor)),
      ),
      child: narrow
          ? Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [labelText, line],
              ),
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
