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
/// - The keyboard is handled by Material's `Dialog` itself: it pads by
///   `viewInsets` and removes them for its child. Never pad by `viewInsets`
///   again on top — the keyboard then costs twice its height and a dialog on
///   a tablet or a landscape phone collapses to nothing.
/// - Widths are `min(desired, screen - 16)` — never a fixed `SizedBox`.
/// - Form flows on narrow *or* short screens become fullscreen pages
///   ([showForm]), because a Scaffold resizes for the keyboard natively.
abstract final class MailDialog {
  /// Narrow = phone / small window. Dialogs go near-fullscreen here.
  static bool isNarrow(BuildContext context) =>
      MediaQuery.sizeOf(context).width < 600;

  /// A form belongs on a fullscreen page: narrow, or too short to keep a
  /// usable dialog once the keyboard takes roughly half the height (landscape
  /// phones, small tablets in landscape).
  static bool prefersPage(BuildContext context) =>
      isNarrow(context) || MediaQuery.sizeOf(context).height < 560;

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

  /// Clamp a desired max height to the room above the keyboard.
  ///
  /// This is only an upper bound: `Dialog` already shrinks its own box by
  /// the keyboard, so the tighter of the two wins and nothing is subtracted
  /// twice.
  static double maxHeight(BuildContext context, double desired) {
    final mq = MediaQuery.of(context);
    final cap = mq.size.height - mq.viewInsets.bottom - 24;
    return desired > cap ? (cap > 0 ? cap : 0) : desired;
  }

  /// The standard `showDialog` wrapper: safe area, non-dismissible option
  /// for forms where a tap-outside would lose input. The keyboard is left to
  /// `Dialog` (see the class docs).
  static Future<T?> show<T>(
    BuildContext context, {
    required WidgetBuilder builder,
    bool barrierDismissible = true,
  }) => showDialog<T>(
    context: context,
    useSafeArea: true,
    barrierDismissible: barrierDismissible,
    builder: builder,
  );

  /// Danger-styled filled button for destructive confirms (permanent delete,
  /// draft destroy, account remove): red in both themes, like Qt's danger
  /// intent. Trash moves stay plain `FilledButton`.
  static ButtonStyle dangerStyle(BuildContext context) =>
      FilledButton.styleFrom(
        backgroundColor: Theme.of(context).colorScheme.error,
        foregroundColor: Theme.of(context).colorScheme.onError,
      );

  /// Show a form flow: fullscreen page on narrow or short screens, dialog
  /// otherwise.
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
    if (prefersPage(context)) {
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
                // A page is full-width on a desktop window too; cap the form
                // so lines stay readable instead of spanning the screen.
                child: Center(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 900),
                    child: SizedBox(width: double.infinity, child: body),
                  ),
                ),
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

/// Avatar letters for a sender: the first letter of the name (or of the
/// address when there is none) plus the first letter of the address's
/// domain, so the many senders sharing one initial still tell apart.
/// Like Qt `Initials.of()`. Non-letters fall back to `?`.
String senderInitials(String name, String address) {
  final at = address.lastIndexOf('@');
  final first = _firstAlnum(name) ?? _firstAlnum(address) ?? '?';
  final second = at < 0
      ? null
      : _firstAlnum(_domainLabel(address.substring(at + 1)));
  return second == null ? first : '$first$second';
}

String? _firstAlnum(String s) =>
    RegExp(r'[a-zA-Z0-9]').firstMatch(s)?.group(0)!.toUpperCase();

/// The name-bearing label of a domain: `mail.example.co.uk` -> `example`.
String _domainLabel(String domain) {
  final labels = domain
      .replaceAll(RegExp(r'[>\s]'), '')
      .toLowerCase()
      .split('.')
      .where((l) => l.isNotEmpty)
      .toList();
  if (labels.length > 1) labels.removeLast();
  if (labels.length > 1 && _secondLevel.contains(labels.last)) {
    labels.removeLast();
  }
  return labels.isEmpty ? '' : labels.last;
}

const _secondLevel = {'co', 'com', 'net', 'org', 'ac', 'gov', 'edu'};

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
