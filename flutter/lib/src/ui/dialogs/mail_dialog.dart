import 'package:flutter/material.dart';

import '../../models/models.dart';

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

  /// Clamp a desired max height to what fits *above the keyboard*.
  ///
  /// The [keyboardSafe] wrapper pads the dialog by the keyboard height, so a
  /// maxHeight computed from the full screen would overflow the remaining box
  /// and push the dialog off-screen (on a phone: dialog gone the moment the
  /// keyboard opens). Subtract the keyboard here so the two stay consistent.
  static double maxHeight(BuildContext context, double desired) {
    final mq = MediaQuery.of(context);
    final cap = mq.size.height - mq.viewInsets.bottom - 24;
    // A keyboard taller than the screen minus chrome leaves nothing usable;
    // still return a sane minimum instead of a negative constraint.
    final sane = cap < 200 ? 200.0 : cap;
    return desired > sane ? sane : desired;
  }

  /// Wrap dialog content so the keyboard never covers it: SafeArea for the
  /// system bars + animated bottom padding for the keyboard.
  ///
  /// This is the SINGLE keyboard handler for dialogs. Do not add extra
  /// `viewInsets.bottom` padding inside dialog bodies on top of it — doubled
  /// padding squeezes the content to zero exactly when the keyboard opens.
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

  /// Show a form flow: fullscreen page on phones, dialog on wide screens.
  ///
  /// This is the general answer to dialogs breaking under the on-screen
  /// keyboard — a floating dialog plus keyboard leaves no usable room on a
  /// short screen, while a Scaffold page resizes natively. Any flow with a
  /// TextField routes through here; pure-choice dialogs (move-to, confirms,
  /// viewers) stay plain dialogs because the keyboard never opens in them.
  static Future<T?> showForm<T>(
    BuildContext context, {
    required WidgetBuilder dialog,
    required WidgetBuilder page,
    bool barrierDismissible = true,
  }) {
    if (MailDialog.isNarrow(context)) {
      return Navigator.of(context).push<T>(
        MaterialPageRoute(fullscreenDialog: true, builder: (ctx) => page(ctx)),
      );
    }
    return MailDialog.show<T>(
      context,
      builder: dialog,
      barrierDismissible: barrierDismissible,
    );
  }
}

/// Fullscreen form page for phones: title + actions in the AppBar, scrollable
/// SafeArea body that the Scaffold shrinks for the keyboard natively.
///
/// Shared chrome so the five form flows (account setup, composer, settings,
/// contacts, folders) don't each reimplement it as a file-private widget.
class MailFormPage extends StatelessWidget {
  const MailFormPage({
    super.key,
    required this.title,
    required this.body,
    this.actions = const [],
    this.bottomBar,
  });

  final String title;
  final Widget body;
  final List<Widget> actions;

  /// Optional fixed bar above the keyboard area (e.g. Save). Prefer AppBar
  /// actions; use this only when the action must stay visible while typing.
  final Widget? bottomBar;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(title), actions: [...actions]),
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: SingleChildScrollView(
                keyboardDismissBehavior:
                    ScrollViewKeyboardDismissBehavior.onDrag,
                padding: const EdgeInsets.all(16),
                child: body,
              ),
            ),
            if (bottomBar case final Widget bar) bar,
          ],
        ),
      ),
    );
  }
}

/// Shared dialog chrome for wide screens: title, scrollable body, end-aligned
/// action row that wraps instead of overflowing.
///
/// Same reason as [MailFormPage]: one implementation instead of seven copies
/// of insetPadding/ConstrainedBox/Padding/Column-title boilerplate.
class MailDialogShell extends StatelessWidget {
  const MailDialogShell({
    super.key,
    required this.title,
    required this.body,
    this.actions = const [],
    this.maxWidth = 560,
    this.maxHeight = 560,
    this.scrollBody = true,
  });

  final String title;
  final Widget body;
  final List<Widget> actions;
  final double maxWidth;
  final double maxHeight;
  final bool scrollBody;

  @override
  Widget build(BuildContext context) {
    final narrow = MailDialog.isNarrow(context);
    final content = scrollBody
        ? Flexible(child: SingleChildScrollView(child: body))
        : Flexible(child: body);
    return Dialog(
      insetPadding: MailDialog.insets(context),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MailDialog.maxWidth(context, maxWidth),
          maxHeight: MailDialog.maxHeight(context, maxHeight),
        ),
        child: Padding(
          padding: EdgeInsets.all(narrow ? 12 : 20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 12),
              content,
              if (actions.isNotEmpty) ...[
                const SizedBox(height: 8),
                Wrap(
                  alignment: WrapAlignment.end,
                  spacing: 8,
                  children: actions,
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// IMAP SPECIAL-USE role as a folder icon. One mapping so the sidebar, the
/// move picker and the folder manager cannot drift apart.
IconData folderIcon(FolderRole role) => switch (role) {
  FolderRole.inbox => Icons.inbox_outlined,
  FolderRole.sent => Icons.send_outlined,
  FolderRole.drafts => Icons.edit_note_outlined,
  FolderRole.trash => Icons.delete_outline,
  FolderRole.junk => Icons.report_gmailerrorred_outlined,
  FolderRole.archive => Icons.archive_outlined,
  FolderRole.custom => Icons.folder_outlined,
};

/// First letter of a sender or address, for avatars. Non-letters fall back
/// to `?` rather than a digit-less empty circle.
String senderInitial(String from) {
  final m = RegExp(r'[a-zA-Z0-9]').firstMatch(from);
  return m == null ? '?' : m.group(0)!.toUpperCase();
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
