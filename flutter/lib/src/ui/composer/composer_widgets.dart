import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';

/// What the dirty-composer guard asked the user to do.
enum DiscardChoice { cancel, discard, save }

/// A file picked for sending. Only the path travels to the core, which reads
/// the bytes at send time — nothing binary lives in Dart state.
class PickedFile {
  const PickedFile({required this.path, required this.name});

  final String path;
  final String name;
}

/// The outgoing tray: picked files as removable chips plus an Add button.
///
/// The picker is the platform file dialog (`file_picker`); on a minimal Linux
/// without zenity/kdialog it cannot open one, and says so instead of failing
/// silently.
class AttachmentPicker extends StatelessWidget {
  const AttachmentPicker({
    super.key,
    required this.picked,
    required this.onChanged,
  });

  final List<PickedFile> picked;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) {
    final state = context.read<MailState>();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (picked.isNotEmpty)
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (var i = 0; i < picked.length; i++)
                Chip(
                  avatar: const Icon(Icons.attach_file, size: 16),
                  label: Text(picked[i].name, overflow: TextOverflow.ellipsis),
                  deleteIcon: const Icon(Icons.close, size: 16),
                  onDeleted: () {
                    picked.removeAt(i);
                    onChanged();
                  },
                ),
            ],
          ),
        Wrap(
          crossAxisAlignment: WrapCrossAlignment.center,
          spacing: 4,
          children: [
            const Icon(Icons.attach_file, size: 16),
            Text(
              picked.isEmpty
                  ? 'No files attached.'
                  : '${picked.length} file(s) will be sent.',
            ),
            TextButton.icon(
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add files…'),
              onPressed: () => _pick(context, state),
            ),
          ],
        ),
      ],
    );
  }

  Future<void> _pick(BuildContext context, MailState state) async {
    List<PlatformFile> files;
    try {
      files = await FilePicker.pickFiles();
    } catch (e) {
      state.showStatus(
        'No file picker available ($e). On Linux this needs zenity, kdialog or qarma installed.',
        isError: true,
      );
      return;
    }
    if (!context.mounted) return;
    var added = 0;
    for (final f in files) {
      final path = f.path;
      if (path == null || path.isEmpty) continue;
      if (picked.any((p) => p.path == path)) continue;
      picked.add(PickedFile(path: path, name: f.name));
      added++;
    }
    if (added == 0 && files.isNotEmpty && files.every((f) => f.path == null)) {
      state.showStatus(
        'The picked files have no usable path on this system.',
        isError: true,
      );
    }
    onChanged();
  }
}

/// A tinted callout: danger for a reply-to mismatch, neutral otherwise.
class ComposerNotice extends StatelessWidget {
  const ComposerNotice({super.key, required this.text, this.danger = false});

  final String text;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: danger ? scheme.errorContainer : scheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Text(
        text,
        style: Theme.of(context).textTheme.bodySmall
            ?.copyWith(color: danger ? scheme.onErrorContainer : null),
      ),
    );
  }
}

/// One-tap Markdown formatting, replacing the Qt WYSIWYG toolbar in scope:
/// bold / italic / quote / bullet act on the body selection; image inserts
/// an inline image at the cursor.
class FormatToolbar extends StatelessWidget {
  const FormatToolbar({
    super.key,
    required this.onBold,
    required this.onItalic,
    required this.onQuote,
    required this.onBullet,
    required this.onImage,
  });

  final VoidCallback onBold;
  final VoidCallback onItalic;
  final VoidCallback onQuote;
  final VoidCallback onBullet;
  final VoidCallback onImage;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Wrap(
        spacing: 0,
        children: [
          IconButton(
            tooltip: 'Bold (**text**)',
            icon: const Text(
              'B',
              style: TextStyle(fontWeight: FontWeight.bold),
            ),
            onPressed: onBold,
          ),
          IconButton(
            tooltip: 'Italic (*text*)',
            icon: const Text(
              'I',
              style: TextStyle(fontStyle: FontStyle.italic),
            ),
            onPressed: onItalic,
          ),
          IconButton(
            tooltip: 'Quote selection',
            icon: const Icon(Icons.format_quote_outlined, size: 20),
            onPressed: onQuote,
          ),
          IconButton(
            tooltip: 'Bullet at cursor',
            icon: const Icon(Icons.format_list_bulleted, size: 20),
            onPressed: onBullet,
          ),
          IconButton(
            tooltip: 'Insert image inline',
            icon: const Icon(Icons.image_outlined, size: 20),
            onPressed: onImage,
          ),
        ],
      ),
    );
  }
}

/// A recipient line with contact suggestions from sent mail.
///
/// Only the segment being typed is completed; picking a suggestion replaces
/// just that segment, so a half-typed list is never clobbered.
class RecipientField extends StatelessWidget {
  const RecipientField({
    super.key,
    required this.label,
    required this.controller,
    this.onToggleCc,
    this.onToggleBcc,
    this.onToggleReplyTo,
  });

  final String label;
  final TextEditingController controller;
  final VoidCallback? onToggleCc;
  final VoidCallback? onToggleBcc;
  final VoidCallback? onToggleReplyTo;

  @override
  Widget build(BuildContext context) {
    final collect = context.select<MailState, bool>(
      (s) => s.settings.collectContacts,
    );
    final hasToggles =
        onToggleCc != null || onToggleBcc != null || onToggleReplyTo != null;
    Widget field() => collect
        ? Autocomplete<Contact>(
            fieldViewBuilder: (context, fieldController, focusNode, onSubmit) {
              // Keep the outer controller authoritative: the inner one
              // mirrors it, and edits flow back through it.
              if (fieldController.text != controller.text) {
                fieldController.text = controller.text;
              }
              return TextField(
                controller: fieldController,
                focusNode: focusNode,
                keyboardType: TextInputType.emailAddress,
                textInputAction: TextInputAction.next,
                onChanged: (v) {
                  if (v != controller.text) controller.text = v;
                },
                decoration: InputDecoration(labelText: label),
              );
            },
            optionsBuilder: (value) async {
              final query = currentSegment(value.text);
              if (query.isEmpty) return const Iterable<Contact>.empty();
              try {
                return await MailCore.instance.contacts(prefix: query);
              } catch (_) {
                return const Iterable<Contact>.empty();
              }
            },
            displayStringForOption: (c) => c.address,
            onSelected: (c) {
              controller.text = replaceSegment(controller.text, c.address);
            },
          )
        : TextField(
            controller: controller,
            keyboardType: TextInputType.emailAddress,
            textInputAction: TextInputAction.next,
            decoration: InputDecoration(labelText: label),
          );
    // Field above, Cc/Bcc/Reply-To toggles wrapped below: a single Row of
    // field + three buttons overflows narrow dialogs (RenderFlex).
    if (!hasToggles) return field();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        field(),
        Wrap(
          children: [
            if (onToggleCc != null)
              TextButton(onPressed: onToggleCc, child: const Text('Cc')),
            if (onToggleBcc != null)
              TextButton(onPressed: onToggleBcc, child: const Text('Bcc')),
            if (onToggleReplyTo != null)
              TextButton(
                onPressed: onToggleReplyTo,
                child: const Text('Reply-To'),
              ),
          ],
        ),
      ],
    );
  }

  static String currentSegment(String text) {
    final parts = text.split(RegExp(r'[,;]'));
    return parts.isEmpty ? '' : parts.last.trim();
  }

  static String replaceSegment(String text, String address) {
    final idx = text.lastIndexOf(RegExp(r'[,;]'));
    final head = idx < 0 ? '' : '${text.substring(0, idx + 1)} ';
    return '$head$address';
  }
}
