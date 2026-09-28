import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';
import 'composer_header_row.dart';

/// What the dirty-composer guard asked the user to do.
enum DiscardChoice { cancel, discard, save }

/// A file picked for sending. Only the path travels to the core, which reads
/// the bytes at send time — nothing binary lives in Dart state.
class PickedFile {
  const PickedFile({required this.path, required this.name});

  final String path;
  final String name;
}

/// The outgoing tray: picked files as removable chips. Empty, it takes no
/// room; files come in through the editor toolbar ([pick]) or a drop.
class AttachmentTray extends StatelessWidget {
  const AttachmentTray({
    super.key,
    required this.picked,
    required this.onChanged,
  });

  final List<PickedFile> picked;
  final VoidCallback onChanged;

  /// Open the platform file dialog and add the chosen files to [picked].
  /// Returns whether anything was added. On a minimal Linux without
  /// zenity/kdialog there is no dialog to open, and it says so instead of
  /// failing silently.
  static Future<bool> pick(MailState state, List<PickedFile> picked) async {
    List<PlatformFile> files;
    try {
      files = await FilePicker.pickFiles();
    } catch (e) {
      state.showStatus(
        'No file picker available ($e). On Linux this needs zenity, kdialog or qarma installed.',
        isError: true,
      );
      return false;
    }
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
    return added > 0;
  }

  @override
  Widget build(BuildContext context) {
    if (picked.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          for (var i = 0; i < picked.length; i++)
            InputChip(
              avatar: const Icon(Icons.attach_file, size: 16),
              label: Text(picked[i].name, overflow: TextOverflow.ellipsis),
              deleteIcon: const Icon(Icons.close, size: 16),
              deleteButtonTooltipMessage: 'Remove',
              onDeleted: () {
                picked.removeAt(i);
                onChanged();
              },
            ),
        ],
      ),
    );
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
/// an inline image at the cursor; the clip attaches files.
class FormatToolbar extends StatelessWidget {
  const FormatToolbar({
    super.key,
    required this.onBold,
    required this.onItalic,
    required this.onQuote,
    required this.onBullet,
    required this.onImage,
    required this.onAttach,
    this.enabled = true,
  });

  final VoidCallback onBold;
  final VoidCallback onItalic;
  final VoidCallback onQuote;
  final VoidCallback onBullet;
  final VoidCallback onImage;
  final VoidCallback onAttach;

  /// Off while previewing: the marks would land in text nobody can see.
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    VoidCallback? on(VoidCallback f) => enabled ? f : null;
    return Wrap(
      children: [
        IconButton(
          tooltip: 'Bold (**text**)',
          icon: const Text('B', style: TextStyle(fontWeight: FontWeight.bold)),
          onPressed: on(onBold),
        ),
        IconButton(
          tooltip: 'Italic (*text*)',
          icon: const Text('I', style: TextStyle(fontStyle: FontStyle.italic)),
          onPressed: on(onItalic),
        ),
        IconButton(
          tooltip: 'Quote selection',
          icon: const Icon(Icons.format_quote_outlined, size: 20),
          onPressed: on(onQuote),
        ),
        IconButton(
          tooltip: 'Bullet at cursor',
          icon: const Icon(Icons.format_list_bulleted, size: 20),
          onPressed: on(onBullet),
        ),
        IconButton(
          tooltip: 'Insert image inline',
          icon: const Icon(Icons.image_outlined, size: 20),
          onPressed: on(onImage),
        ),
        IconButton(
          tooltip: 'Attach files',
          icon: const Icon(Icons.attach_file, size: 20),
          onPressed: onAttach,
        ),
      ],
    );
  }
}

/// A recipient line with contact suggestions from sent mail.
///
/// Only the segment being typed is completed; picking a suggestion replaces
/// just that segment, so a half-typed list is never clobbered.
class RecipientField extends StatelessWidget {
  const RecipientField({super.key, required this.controller, this.hint});

  final TextEditingController controller;
  final String? hint;

  @override
  Widget build(BuildContext context) {
    final collect = context.select<MailState, bool>(
      (s) => s.settings.collectContacts,
    );
    final decoration = ComposerHeaderRow.field(hint: hint);
    return collect
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
                decoration: decoration,
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
            decoration: decoration,
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
