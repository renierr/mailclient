import 'package:flutter/material.dart';

import '../../models/models.dart';
import 'attachment_card.dart' show openAttachment, saveAttachment;

/// Preview card for an iCalendar event invitation above the message body.
class EventCard extends StatelessWidget {
  const EventCard({
    super.key,
    required this.message,
    required this.event,
    this.passThrough = false,
  });

  final MessageBody message;
  final CalendarEventInfo event;
  final bool passThrough;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final small = theme.textTheme.bodySmall;
    final compact = TextButton.styleFrom(
      visualDensity: VisualDensity.compact,
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
    );

    Widget flow(Widget child) =>
        passThrough ? IgnorePointer(child: child) : child;

    final isCancelled = event.isCancelled;
    final iconColor = isCancelled ? scheme.error : scheme.primary;
    final iconBg = isCancelled
        ? scheme.errorContainer.withValues(alpha: 0.5)
        : scheme.primaryContainer.withValues(alpha: 0.5);

    AttachmentInfo? findAttachment() {
      if (event.attachmentId == null) return null;
      // An inline invitation part is not in the attachment list; use the
      // core's safe name, which always comes with attachmentId.
      final name = event.saveName ?? '';
      return message.attachments
              .where((a) => a.id == event.attachmentId)
              .firstOrNull ??
          AttachmentInfo(
            id: event.attachmentId!,
            filename: name,
            fileName: name,
            mimeType: 'text/calendar',
            size: 0,
            sizeText: '',
            isInline: false,
          );
    }

    final att = findAttachment();

    final content = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: iconBg,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Icon(Icons.event, color: iconColor, size: 22),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: flow(
                          Text(
                            event.summary,
                            style: theme.textTheme.titleSmall?.copyWith(
                              fontWeight: FontWeight.bold,
                            ),
                            // A long subject line must wrap to a bounded
                            // height: unbounded growth resizes the WebView
                            // spacer on every frame and overlaps the card
                            // below it.
                            maxLines: 3,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      ),
                      if (isCancelled) ...[
                        const SizedBox(width: 8),
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 6,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: scheme.error,
                            borderRadius: BorderRadius.circular(4),
                          ),
                          child: Text(
                            'Cancelled',
                            style: theme.textTheme.labelSmall?.copyWith(
                              color: scheme.onError,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ),
                      ],
                    ],
                  ),
                  const SizedBox(height: 4),
                  Row(
                    children: [
                      Icon(
                        Icons.schedule,
                        size: 14,
                        color: scheme.onSurfaceVariant,
                      ),
                      const SizedBox(width: 4),
                      Expanded(
                        child: flow(
                          Text(
                            event.formattedTime,
                            style: small?.copyWith(
                              fontWeight: FontWeight.w600,
                              color: scheme.onSurface,
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
                  if (event.location != null &&
                      event.location!.trim().isNotEmpty) ...[
                    const SizedBox(height: 2),
                    Row(
                      children: [
                        Icon(
                          Icons.place_outlined,
                          size: 14,
                          color: scheme.onSurfaceVariant,
                        ),
                        const SizedBox(width: 4),
                        Expanded(
                          child: flow(
                            Text(
                              event.location!,
                              style: small?.copyWith(
                                color: scheme.onSurfaceVariant,
                              ),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                  if (event.organizer != null &&
                      event.organizer!.trim().isNotEmpty) ...[
                    const SizedBox(height: 2),
                    Row(
                      children: [
                        Icon(
                          Icons.person_outline,
                          size: 14,
                          color: scheme.onSurfaceVariant,
                        ),
                        const SizedBox(width: 4),
                        Expanded(
                          child: flow(
                            Text(
                              'Organizer: ${event.organizer!}',
                              style: small?.copyWith(
                                color: scheme.onSurfaceVariant,
                              ),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
        if (att != null) ...[
          const SizedBox(height: 8),
          Wrap(
            alignment: WrapAlignment.end,
            spacing: 8,
            children: [
              TextButton.icon(
                style: compact,
                onPressed: () => openAttachment(context, message, att),
                icon: const Icon(Icons.open_in_new, size: 16),
                label: const Text('Open in Calendar'),
              ),
              TextButton(
                style: compact,
                onPressed: () => saveAttachment(context, message, att),
                child: const Text('Save .ics'),
              ),
            ],
          ),
        ],
      ],
    );

    const margin = EdgeInsets.fromLTRB(12, 8, 12, 4);
    const padding = EdgeInsets.all(12);
    final decoration = BoxDecoration(
      color: scheme.surfaceContainerLow,
      border: Border.all(
        color: isCancelled ? scheme.error : scheme.outlineVariant,
      ),
      borderRadius: BorderRadius.circular(8),
    );

    if (!passThrough) {
      return Container(
        margin: margin,
        padding: padding,
        decoration: decoration,
        child: content,
      );
    }

    return Padding(
      padding: margin,
      child: Stack(
        children: [
          Positioned.fill(
            child: IgnorePointer(child: DecoratedBox(decoration: decoration)),
          ),
          Padding(padding: padding, child: content),
        ],
      ),
    );
  }
}
