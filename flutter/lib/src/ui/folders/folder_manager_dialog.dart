import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// The IMAP folder manager: create folders, hide them from the sidebar,
/// refresh the server-side list, and jump to one.
///
/// Hiding is display-only — a hidden folder keeps its cache and still
/// quick-syncs, so its unread count stays honest.
class FolderManagerDialog extends StatefulWidget {
  const FolderManagerDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await MailDialog.show(context, builder: (_) => const FolderManagerDialog());
  }

  @override
  State<FolderManagerDialog> createState() => _FolderManagerDialogState();
}

class _FolderManagerDialogState extends State<FolderManagerDialog> {
  final _newFolder = TextEditingController();
  bool _canCreate = false;

  @override
  void initState() {
    super.initState();
    _newFolder.addListener(_onNameChanged);
  }

  void _onNameChanged() {
    final can = _newFolder.text.trim().isNotEmpty;
    if (can != _canCreate) setState(() => _canCreate = can);
  }

  @override
  void dispose() {
    _newFolder.removeListener(_onNameChanged);
    _newFolder.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final narrow = MailDialog.isNarrow(context);
    return Dialog(
      insetPadding: MailDialog.insets(context, wideH: 16),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MailDialog.maxWidth(context, 520),
          maxHeight: MailDialog.maxHeight(context, 560),
        ),
        child: Padding(
          padding: EdgeInsets.all(narrow ? 12 : 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                'IMAP folders',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _newFolder,
                      textInputAction: TextInputAction.done,
                      onSubmitted: (_) => _create(state),
                      decoration: const InputDecoration(
                        labelText: 'New folder name (/ for subfolders)',
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: _canCreate ? () => _create(state) : null,
                    child: const Text('Create'),
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Text(
                'Uncheck to hide a folder from the sidebar.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 8),
              Expanded(
                child: state.allFolders.isEmpty
                    ? const Center(
                        child: Text(
                          'No folders yet — press Refresh from server.',
                        ),
                      )
                    : ListView.separated(
                        // Keep rows above the keyboard while creating.
                        padding: EdgeInsets.only(
                          bottom: MediaQuery.viewInsetsOf(context).bottom,
                        ),
                        itemCount: state.allFolders.length,
                        separatorBuilder: (_, _) => const Divider(height: 1),
                        itemBuilder: (context, i) =>
                            _row(context, state, state.allFolders[i]),
                      ),
              ),
              const SizedBox(height: 8),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  OutlinedButton.icon(
                    icon: state.isBusy
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.sync, size: 16),
                    label: const Text('Refresh from server'),
                    onPressed: state.isBusy
                        ? null
                        : () => state.refreshFolders(),
                  ),
                  const SizedBox(width: 8),
                  TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: const Text('Close'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _row(BuildContext context, MailState state, Folder f) {
    final current = f.id == state.folderId;
    return ListTile(
      selected: current,
      contentPadding: EdgeInsets.zero,
      dense: true,
      leading: Checkbox(
        value: f.subscribed,
        onChanged: (v) => state.setFolderSubscribed(f.id, v ?? true),
      ),
      title: Row(
        children: [
          Icon(_iconFor(f.role), size: 20),
          const SizedBox(width: 8),
          Expanded(child: Text(f.path, overflow: TextOverflow.ellipsis)),
        ],
      ),
      subtitle: Text('${f.total} · ${f.unread} unread'),
      // Qt parity: chevron jumps to the folder (and closes the manager).
      trailing: IconButton(
        tooltip: current ? 'Currently open' : 'Open folder',
        icon: const Icon(Icons.chevron_right, size: 20),
        onPressed: current
            ? null
            : () {
                Navigator.of(context).pop();
                state.selectFolder(f.id);
              },
      ),
      onTap: current
          ? null
          : () {
              Navigator.of(context).pop();
              state.selectFolder(f.id);
            },
    );
  }

  Future<void> _create(MailState state) async {
    final path = _newFolder.text.trim();
    if (path.isEmpty) return;
    // `/` separates levels; the core maps it onto the account delimiter.
    await state.createFolder(path.replaceAll('\\', '/'));
    if (!mounted) return;
    setState(_newFolder.clear);
  }

  static IconData _iconFor(FolderRole role) => switch (role) {
    FolderRole.inbox => Icons.inbox_outlined,
    FolderRole.sent => Icons.send_outlined,
    FolderRole.drafts => Icons.edit_note_outlined,
    FolderRole.trash => Icons.delete_outline,
    FolderRole.junk => Icons.report_gmailerrorred_outlined,
    FolderRole.archive => Icons.archive_outlined,
    FolderRole.custom => Icons.folder_outlined,
  };
}
