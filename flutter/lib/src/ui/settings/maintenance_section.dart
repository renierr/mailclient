import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// Storage stats plus one-shot local actions: database export, temp cleanup,
/// downloaded-file eviction and cache trimming.
///
/// Like Qt's Maintenance pane, the stats are a live read (not part of the
/// settings Save draft) and the actions apply immediately. Everything here
/// is local-only and never touches the mail server.
class MaintenanceSection extends StatefulWidget {
  const MaintenanceSection({super.key});

  @override
  State<MaintenanceSection> createState() => _MaintenanceSectionState();
}

/// The viewer-copy folder, same one `writeAttachmentCopy` stages into.
Future<String> _tempDir() async {
  final base = (await getTemporaryDirectory()).path;
  return '$base${Platform.pathSeparator}mailclient-attachments';
}

class _MaintenanceSectionState extends State<MaintenanceSection> {
  Map<String, dynamic>? _stats;
  bool _loading = true;
  bool _acting = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final stats = await context.read<MailState>().storageStats(
        await _tempDir(),
      );
      if (!mounted) return;
      setState(() {
        _stats = stats;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _loading = false);
      context.read<MailState>().showStatus(coreErrorText(e), isError: true);
    }
  }

  /// Deleting cached data is recoverable (sync / re-download brings it
  /// back), but it still asks first and says exactly what will happen.
  Future<bool> _confirm(String title, String body, String action) async {
    final ok =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: Text(title),
            content: Text(body),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                style: MailDialog.dangerStyle(context),
                onPressed: () => Navigator.of(context).pop(true),
                child: Text(action),
              ),
            ],
          ),
        ) ??
        false;
    return ok;
  }

  /// Run an action, announce the status line it set, and re-read the stats.
  Future<void> _guard(Future<void> Function() run) async {
    setState(() => _acting = true);
    try {
      await run();
    } finally {
      if (mounted) setState(() => _acting = false);
    }
    if (!mounted) return;
    final state = context.read<MailState>();
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(state.status)));
    _load();
  }

  Future<void> _export() => _guard(() async {
    final dir = await FilePicker.getDirectoryPath(
      dialogTitle: 'Export database',
    );
    if (dir == null || !mounted) return;
    // The state reports success or failure; a failure still reloads the
    // stats below, which is harmless.
    await context.read<MailState>().exportDatabase(dir);
  });

  Future<void> _cleanTemp() async {
    if (!await _confirm(
      'Clean temporary files?',
      'Delete staged viewer copies? They are re-created the next time '
          'an attachment is opened. Mail on the server is untouched.',
      'Clean',
    )) {
      return;
    }
    if (!mounted) return;
    await _guard(() async {
      await context.read<MailState>().cleanupTempFiles(await _tempDir());
    });
  }

  Future<void> _evict() async {
    if (!await _confirm(
      'Remove downloaded files?',
      'Delete downloaded attachment files from this device? Names and '
          'sizes stay, and files download again when opened. Mail on the '
          'server is untouched.',
      'Remove',
    )) {
      return;
    }
    if (!mounted) return;
    await _guard(() => context.read<MailState>().evictCachedAttachments());
  }

  Future<void> _trim() async {
    final keep = (_stats?['keep_per_folder'] as num?)?.toInt() ?? 200;
    if (!await _confirm(
      'Trim old messages?',
      'Delete cached messages past the newest $keep per folder from this '
          'device? Drafts and unsent mail are kept, and trimmed mail '
          'returns with the next sync. Mail on the server is untouched.',
      'Trim',
    )) {
      return;
    }
    if (!mounted) return;
    await _guard(() => context.read<MailState>().trimLocalCache());
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Row(
        children: [
          SizedBox(
            width: 14,
            height: 14,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          SizedBox(width: 8),
          Expanded(child: Text('Loading storage…')),
        ],
      );
    }
    final stats = _stats;
    final busy = _acting;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Storage', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        if (stats != null) ...[
          SelectableText('Database: ${stats['db_display'] ?? ''}'),
          SelectableText(
            '${(stats['message_count'] as num? ?? 0).toInt()} messages cached',
          ),
          SelectableText('Downloaded files: ${stats['cached_display'] ?? ''}'),
          SelectableText('Temporary files: ${stats['temp_display'] ?? ''}'),
          SelectableText('Database: ${stats['db_path'] ?? ''}'),
          const SizedBox(height: 8),
        ],
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton(
              onPressed: busy ? null : _load,
              child: const Text('Refresh'),
            ),
          ],
        ),
        const SizedBox(height: 16),
        Text('Database backup', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        const Text(
          'Saves a consistent copy of the local mail database. '
          'Your mail stays where it is.',
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton.icon(
              onPressed: busy ? null : _export,
              icon: const Icon(Icons.save_outlined),
              label: const Text('Export database…'),
            ),
          ],
        ),
        const SizedBox(height: 16),
        Text('Cache', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        const Text(
          'Frees local space only — nothing here touches the mail server. '
          'Trimmed messages return with the next sync; removed files '
          'download again when opened.',
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton(
              onPressed: busy ? null : _cleanTemp,
              child: const Text('Clean temporary files'),
            ),
            OutlinedButton(
              onPressed: busy ? null : _evict,
              child: const Text('Remove downloaded files'),
            ),
            OutlinedButton(
              onPressed: busy ? null : _trim,
              child: const Text('Trim old messages'),
            ),
          ],
        ),
      ],
    );
  }
}
