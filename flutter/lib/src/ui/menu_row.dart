import 'package:flutter/material.dart';

/// A popup-menu row with a leading icon, so menus read at a glance.
class MenuRow extends StatelessWidget {
  const MenuRow({super.key, required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 18, color: Theme.of(context).colorScheme.outline),
        const SizedBox(width: 12),
        Expanded(child: Text(text, overflow: TextOverflow.ellipsis)),
      ],
    );
  }
}
