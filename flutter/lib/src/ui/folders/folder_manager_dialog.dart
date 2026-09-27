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
  const FolderManagerDialog({super.key, this.fullscreen = false});

  /// Fullscreen page instead of a floating dialog — used on phones, where a
  /// dialog plus the on-screen keyboard leaves no usable room.
  final bool fullscreen;

  static Future<void> show(BuildContext context) async {
    await MailDialog.showForm(
      context,
      dialog: (_) => const FolderManagerDialog(),
      page: (_) => const FolderManagerDialog(fullscreen: true),
    );
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
    if (widget.fullscreen) return _page();
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
              _createRow(),
              const SizedBox(height: 4),
              Text(
                'Uncheck to hide a folder from the sidebar.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 8),
              Expanded(child: _listBody()),
              const SizedBox(height: 8),
              // Wrap, not Row: Refresh + Close stack instead of overflowing
              // on a very narrow dialog.
              Wrap(
                alignment: WrapAlignment.end,
                spacing: 8,
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

  /// Create row: field beside the button on wide screens, stacked full-width
  /// on narrow/zoomed layouts where the Row squeezes the field to zero.
  Widget _createRow() {
    final state = context.read<MailState>();
    final field = TextField(
      controller: _newFolder,
      textInputAction: TextInputAction.done,
      onSubmitted: (_) => _create(state),
      decoration: const InputDecoration(
        labelText: 'New folder name (/ for subfolders)',
      ),
    );
    final button = FilledButton(
      onPressed: _canCreate ? () => _create(state) : null,
      child: const Text('Create'),
    );
    if (MailDialog.isNarrow(context)) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          field,
          const SizedBox(height: 8),
          Align(alignment: Alignment.centerRight, child: button),
        ],
      );
    }
    return Row(
      children: [
        Expanded(child: field),
        const SizedBox(width: 8),
        button,
      ],
    );
  }

  /// Folder list, shared by the dialog and the fullscreen page.
  Widget _listBody() {
    final state = context.watch<MailState>();
    if (state.allFolders.isEmpty) {
      return const Center(
        child: Text('No folders yet — press Refresh from server.'),
      );
    }
    return ListView.separated(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      itemCount: state.allFolders.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) => _row(context, state, state.allFolders[i]),
    );
  }

  /// Fullscreen folder manager for phones (see [FolderManagerDialog.fullscreen]):
  /// the list stays lazy inside Expanded while the Scaffold shrinks for the
  /// keyboard natively.
  Widget _page() {
    final state = context.watch<MailState>();
    return Scaffold(
      appBar: AppBar(
        title: const Text('IMAP folders'),
        actions: [
          OutlinedButton.icon(
            icon: state.isBusy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.sync, size: 16),
            label: const Text('Refresh'),
            onPressed: state.isBusy ? null : () => state.refreshFolders(),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
              child: _createRow(),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 0),
              child: Text(
                'Uncheck to hide a folder from the sidebar.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
            const SizedBox(height: 8),
            Expanded(child: _listBody()),
          ],
        ),
      ),
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
