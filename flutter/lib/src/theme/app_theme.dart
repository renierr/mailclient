import 'package:flutter/material.dart';

/// The app's light and dark themes.
///
/// Both are generated from one seed colour so the two stay in step: adding a
/// surface here means it exists in both, which is the failure mode a
/// hand-written dark palette always eventually hits.
abstract final class AppTheme {
  static const _seed = Color(0xFF2F6FED);

  static ThemeData light() => _build(Brightness.light);
  static ThemeData dark() => _build(Brightness.dark);

  static ThemeData _build(Brightness brightness) {
    final scheme = ColorScheme.fromSeed(
      seedColor: _seed,
      brightness: brightness,
    );
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      // A mail list is dense by nature: the default desktop density leaves
      // barely a dozen rows on screen.
      visualDensity: VisualDensity.compact,
      listTileTheme: const ListTileThemeData(
        dense: true,
        minVerticalPadding: 6,
      ),
      dividerTheme: DividerThemeData(
        space: 1,
        thickness: 1,
        color: scheme.outlineVariant,
      ),
    );
  }
}

/// Where the layout switches between one, two and three panes.
///
/// Phone-sized windows exist on the desktop too — a narrow window is a narrow
/// window — so this is keyed on width, never on the platform.
abstract final class Breakpoints {
  /// Below this, one pane at a time with navigation between them.
  static const compact = 700.0;

  /// Below this, sidebar plus list; the reader opens over them.
  static const medium = 1100.0;
}
