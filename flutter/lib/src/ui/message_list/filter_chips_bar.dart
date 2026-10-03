import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';

/// Quick filter chips (Unread, Starred, Attachments) above the message list.
class FilterChipsBar extends StatelessWidget {
  const FilterChipsBar({super.key});

  @override
  Widget build(BuildContext context) {
    final filterUnread = context.select<MailState, bool>((s) => s.filterUnread);
    final filterStarred = context.select<MailState, bool>(
      (s) => s.filterStarred,
    );
    final filterAttachments = context.select<MailState, bool>(
      (s) => s.filterAttachments,
    );
    final hasFilter = filterUnread || filterStarred || filterAttachments;

    final theme = Theme.of(context);
    final colorScheme = theme.colorScheme;

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: Wrap(
        spacing: 6,
        runSpacing: 4,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
            FilterChip(
              showCheckmark: false,
              avatar: Icon(
                Icons.mark_email_unread_outlined,
                size: 16,
                color: filterUnread
                    ? colorScheme.primary
                    : colorScheme.onSurfaceVariant,
              ),
              label: const Text('Unread'),
              labelStyle: TextStyle(
                fontSize: 12,
                fontWeight: filterUnread ? FontWeight.bold : FontWeight.normal,
              ),
              selected: filterUnread,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              onSelected: (v) => context.read<MailState>().setFilterUnread(v),
            ),
            const SizedBox(width: 6),
            FilterChip(
              showCheckmark: false,
              avatar: Icon(
                Icons.star_border,
                size: 16,
                color: filterStarred
                    ? colorScheme.primary
                    : colorScheme.onSurfaceVariant,
              ),
              label: const Text('Starred'),
              labelStyle: TextStyle(
                fontSize: 12,
                fontWeight: filterStarred ? FontWeight.bold : FontWeight.normal,
              ),
              selected: filterStarred,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              onSelected: (v) => context.read<MailState>().setFilterStarred(v),
            ),
            const SizedBox(width: 6),
            FilterChip(
              showCheckmark: false,
              avatar: Icon(
                Icons.attach_file,
                size: 16,
                color: filterAttachments
                    ? colorScheme.primary
                    : colorScheme.onSurfaceVariant,
              ),
              label: const Text('Attachments'),
              labelStyle: TextStyle(
                fontSize: 12,
                fontWeight: filterAttachments
                    ? FontWeight.bold
                    : FontWeight.normal,
              ),
              selected: filterAttachments,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              onSelected: (v) =>
                  context.read<MailState>().setFilterAttachments(v),
            ),
            if (hasFilter)
              TextButton(
                style: TextButton.styleFrom(
                  visualDensity: VisualDensity.compact,
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                ),
                onPressed: () => context.read<MailState>().clearListFilters(),
                child: const Text('Clear', style: TextStyle(fontSize: 12)),
              ),
          ],
        ),
      );
    }
  }
