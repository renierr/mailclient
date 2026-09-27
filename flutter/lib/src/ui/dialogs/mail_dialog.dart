import 'package:flutter/material.dart';

/// Shared mobile-safe dialog conventions.
///
/// Why this exists: every dialog used to be a fixed-width `Dialog`/`AlertDialog`
/// with no `SafeArea` and no `viewInsets` handling. On a phone with the
/// on-screen keyboard up, the keyboard covered the body and the action row,
/// fixed 460–480px widths overflowed 360px screens, and a tap outside the
/// account form silently dropped the input.
///
/// Rules enforced here:
/// - `SafeArea` around every dialog so gesture bars / notches never clip.
/// - `AnimatedPadding` with `MediaQuery.viewInsetsOf` so the keyboard pushes
///   content up instead of covering it.
/// - Widths are `min(desired, screen - 16)` — never a fixed `SizedBox`.
/// - Narrow screens (<600px) get near-fullscreen sheets: `insetPadding` ~8px
///   and `maxHeight` ~ screen height, so the body scrolls above the keyboard
///   instead of being squeezed into a floating box.
abstract final class MailDialog {
  /// Narrow = phone / small window. Dialogs go near-fullscreen here.
  static bool isNarrow(BuildContext context) =>
      MediaQuery.sizeOf(context).width < 600;

  /// Inset around the dialog: almost none on phones, comfortable on desktop.
  static EdgeInsets insets(BuildContext context, {double wideH = 40}) =>
      EdgeInsets.symmetric(
        horizontal: isNarrow(context) ? 8 : wideH,
        vertical: isNarrow(context) ? 8 : 24,
      );

  /// Clamp a desired max width to what fits on screen.
  static double maxWidth(BuildContext context, double desired) {
    final screen = MediaQuery.sizeOf(context).width;
    return desired > screen - 16 ? screen - 16 : desired;
  }

  /// Clamp a desired max height to what fits on screen (keyboard excluded —
  /// the viewInsets padding handles that separately).
  static double maxHeight(BuildContext context, double desired) {
    final screen = MediaQuery.sizeOf(context).height;
    final cap = screen - 24;
    return desired > cap ? cap : desired;
  }

  /// Wrap dialog content so the keyboard never covers it: SafeArea for the
  /// system bars + animated bottom padding for the keyboard.
  static Widget keyboardSafe({required Widget child}) => Builder(
    builder: (context) => SafeArea(
      child: AnimatedPadding(
        padding: MediaQuery.viewInsetsOf(context),
        duration: const Duration(milliseconds: 150),
        curve: Curves.easeOut,
        child: child,
      ),
    ),
  );

  /// The standard `showDialog` wrapper: safe area, non-dismissible option
  /// for forms where a tap-outside would lose input.
  static Future<T?> show<T>(
    BuildContext context, {
    required WidgetBuilder builder,
    bool barrierDismissible = true,
  }) => showDialog<T>(
    context: context,
    useSafeArea: true,
    barrierDismissible: barrierDismissible,
    builder: (ctx) => keyboardSafe(child: Builder(builder: builder)),
  );

  /// Danger-styled filled button for destructive confirms (permanent delete,
  /// draft destroy, account remove): red in both themes, like Qt's danger
  /// intent. Trash moves stay plain `FilledButton`.
  static ButtonStyle dangerStyle(BuildContext context) =>
      FilledButton.styleFrom(
        backgroundColor: Theme.of(context).colorScheme.error,
        foregroundColor: Theme.of(context).colorScheme.onError,
      );
}

/// Deterministic avatar colour from any string, like Qt `avatarColor()`.
Color avatarColor(BuildContext context, String seed) {
  final scheme = Theme.of(context).colorScheme;
  final colors = <Color>[
    scheme.primary,
    scheme.secondary,
    scheme.tertiary,
    Colors.teal,
    Colors.indigo,
    Colors.deepOrange,
    Colors.green,
    Colors.purple,
  ];
  var hash = 0;
  for (final unit in seed.runes) {
    hash = (hash * 31 + unit) & 0x7fffffff;
  }
  return colors[hash % colors.length];
}
