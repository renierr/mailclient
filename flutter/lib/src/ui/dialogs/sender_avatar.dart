import 'package:flutter/material.dart';

import '../../models/models.dart';

/// Round sender avatar from a `mailcore` badge, like the Qt `Avatar`:
/// letters and colour are computed once in the core for both frontends.
class SenderAvatar extends StatelessWidget {
  const SenderAvatar({
    super.key,
    required this.badge,
    this.radius = 18,
    this.fontSize = 14,
  });

  final SenderBadge badge;
  final double radius;
  final double fontSize;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final dark = theme.brightness == Brightness.dark;
    return CircleAvatar(
      radius: radius,
      backgroundColor:
          _hex(dark ? badge.dark : badge.light) ??
          theme.colorScheme.onSurfaceVariant,
      foregroundColor: Colors.white,
      child: Text(
        badge.initials,
        style: TextStyle(fontSize: fontSize, fontWeight: FontWeight.w600),
      ),
    );
  }

  static Color? _hex(String s) {
    if (s.length != 7 || !s.startsWith('#')) return null;
    final rgb = int.tryParse(s.substring(1), radix: 16);
    return rgb == null ? null : Color(0xff000000 | rgb);
  }
}
