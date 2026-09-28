import 'package:flutter/material.dart';

import '../reader/mail_html_view.dart';
import '../reader/mail_paint.dart';
import 'composer_widgets.dart';
import 'markdown.dart';

/// The message body: formatting toolbar, the Markdown text field and a
/// preview of what the recipient will see, in one framed box.
///
/// The field is plain text, so formatting and inline images sit in it as
/// Markdown marks and `![name](inline:N)` tokens. Preview renders them the
/// way they are sent ([MarkdownMail.toHtml]), which is the only place an
/// inline image is actually visible while composing.
class ComposerEditor extends StatefulWidget {
  const ComposerEditor({
    super.key,
    required this.controller,
    required this.images,
    required this.sendFormat,
    required this.onBold,
    required this.onItalic,
    required this.onQuote,
    required this.onBullet,
    required this.onImage,
    required this.onAttach,
  });

  final TextEditingController controller;

  /// Inline image token id → `data:` URL, read when previewing.
  final Map<int, String> Function() images;

  /// The send-format setting (`auto`, `plain`, `multipart`, `html`).
  final String sendFormat;

  final VoidCallback onBold;
  final VoidCallback onItalic;
  final VoidCallback onQuote;
  final VoidCallback onBullet;
  final VoidCallback onImage;
  final VoidCallback onAttach;

  @override
  State<ComposerEditor> createState() => _ComposerEditorState();
}

class _ComposerEditorState extends State<ComposerEditor> {
  bool _preview = false;

  /// What Send will produce, in words: the setting alone does not say
  /// (automatic depends on whether the text carries formatting).
  static String formatNote(String format, bool formatted) => switch (format) {
    'plain' => 'Sends as plain text',
    'html' => 'Sends as HTML',
    'multipart' => 'Sends as plain text and HTML',
    _ => formatted ? 'Sends formatted (HTML)' : 'Sends as plain text',
  };

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final muted = theme.textTheme.bodySmall?.copyWith(
      color: scheme.onSurfaceVariant,
    );
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border.all(color: theme.dividerColor),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 4),
            child: Row(
              children: [
                Expanded(
                  child: FormatToolbar(
                    enabled: !_preview,
                    onBold: widget.onBold,
                    onItalic: widget.onItalic,
                    onQuote: widget.onQuote,
                    onBullet: widget.onBullet,
                    onImage: widget.onImage,
                    onAttach: widget.onAttach,
                  ),
                ),
                IconButton(
                  tooltip: _preview ? 'Back to editing' : 'Preview',
                  isSelected: _preview,
                  icon: const Icon(Icons.visibility_outlined, size: 20),
                  selectedIcon: const Icon(Icons.edit_outlined, size: 20),
                  onPressed: () => setState(() => _preview = !_preview),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: theme.dividerColor),
          if (_preview) _previewPane(context) else _field(),
          Divider(height: 1, color: theme.dividerColor),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            child: ValueListenableBuilder<TextEditingValue>(
              valueListenable: widget.controller,
              builder: (context, value, _) => Text(
                '${formatNote(widget.sendFormat, MarkdownMail.hasFormatting(value.text))}'
                ' · Markdown: **bold**, *italic*, > quote',
                style: muted,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _field() => Padding(
    padding: const EdgeInsets.symmetric(horizontal: 12),
    child: TextField(
      controller: widget.controller,
      maxLines: null,
      minLines: 10,
      keyboardType: TextInputType.multiline,
      textInputAction: TextInputAction.newline,
      decoration: const InputDecoration(
        hintText: 'Write your message',
        border: InputBorder.none,
        contentPadding: EdgeInsets.symmetric(vertical: 12),
      ),
    ),
  );

  Widget _previewPane(BuildContext context) {
    final text = widget.controller.text;
    return SizedBox(
      height: MediaQuery.textScalerOf(context).scale(260),
      child: text.trim().isEmpty
          ? Center(
              child: Text(
                'Nothing to preview yet',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            )
          : ClipRRect(
              borderRadius: BorderRadius.circular(2),
              child: MailHtmlView(
                paint: MailPaint.theme,
                html: MarkdownMail.toHtml(text, images: widget.images()),
              ),
            ),
    );
  }
}
