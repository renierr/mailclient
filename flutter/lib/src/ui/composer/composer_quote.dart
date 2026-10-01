import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

/// The quoted original of a reply or forward, carried beside the Markdown
/// text box rather than inside it: the core quotes an HTML mail as HTML
/// (`mailcore::compose::answer`), which plain text cannot hold, and the
/// composer appends it to `body_html` on send. Collapsed by default; it can
/// be previewed or left out, not edited.
class ComposerQuote extends StatefulWidget {
  const ComposerQuote({
    super.key,
    required this.html,
    required this.onRemove,
    this.forward = false,
  });

  final String html;
  final VoidCallback onRemove;

  /// A forwarded message rather than a quoted reply (only the label).
  final bool forward;

  @override
  State<ComposerQuote> createState() => _ComposerQuoteState();
}

class _ComposerQuoteState extends State<ComposerQuote> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Container(
      decoration: BoxDecoration(
        border: Border.all(color: scheme.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: InkWell(
                  onTap: () => setState(() => _open = !_open),
                  borderRadius: BorderRadius.circular(8),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 12,
                    ),
                    child: Row(
                      children: [
                        Icon(
                          _open ? Icons.expand_more : Icons.chevron_right,
                          size: 20,
                        ),
                        const SizedBox(width: 4),
                        Flexible(
                          child: Text(
                            widget.forward
                                ? 'Forwarded message'
                                : 'Quoted original',
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodyMedium,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              IconButton(
                tooltip: 'Leave out',
                icon: const Icon(Icons.close),
                onPressed: widget.onRemove,
              ),
            ],
          ),
          if (_open)
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 320),
              child: SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                child: HtmlWidget(
                  widget.html,
                  textStyle: theme.textTheme.bodySmall,
                ),
              ),
            ),
        ],
      ),
    );
  }
}
