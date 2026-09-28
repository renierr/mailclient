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
  bool _creating = false;

  /// Why the last create failed, shown under the field — the status bar is
  /// behind this dialog (and a whole page away on phones).
  String? _error;

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
    final busy = context.select<MailState, bool>((s) => s.isBusy);
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
              _header(),
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
                    icon: busy
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.sync, size: 16),
                    label: const Text('Refresh from server'),
                    onPressed: busy
                        ? null
                        : () => context.read<MailState>().refreshFolders(),
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

  Widget _row(BuildContext context, MailState state, Folder f, int openId) {
    final current = f.id == openId;
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
          Icon(folderIcon(f.role), size: 20),
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
      onPressed: _canCreate && !_creating ? () => _create(state) : null,
      child: _creating
          ? const SizedBox(
              width: 16,
              height: 16,
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          : const Text('Create'),
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

  /// Create row, create error and the hint line, shared by dialog and page.
  Widget _header() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _createRow(),
        if (_error case final String error) ...[
          const SizedBox(height: 4),
          Text(
            error,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
        const SizedBox(height: 4),
        Text(
          'Uncheck to hide a folder from the sidebar.',
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ],
    );
  }

  /// Folder list for the dialog, whose header is short enough to stay fixed.
  Widget _listBody() {
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.allFolders,
    );
    // Subscribed, so the "current" marker follows a folder switch.
    final openId = context.select<MailState, int>((s) => s.folderId);
    if (folders.isEmpty) return _emptyHint();
    return ListView.separated(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      itemCount: folders.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) =>
          _row(context, context.read<MailState>(), folders[i], openId),
    );
  }

  Widget _emptyHint() =>
      const Center(child: Text('No folders yet — press Refresh from server.'));

  /// Fullscreen folder manager for phones (see [FolderManagerDialog.fullscreen]).
  /// Header and rows scroll as one lazy sliver list: a fixed header over an
  /// Expanded list overflows a landscape phone once the keyboard is up.
  Widget _page() {
    final busy = context.select<MailState, bool>((s) => s.isBusy);
    final folders = context.select<MailState, List<Folder>>(
      (s) => s.allFolders,
    );
    final openId = context.select<MailState, int>((s) => s.folderId);
    return Scaffold(
      appBar: AppBar(
        title: const Text('IMAP folders'),
        actions: [
          OutlinedButton.icon(
            icon: busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.sync, size: 16),
            label: const Text('Refresh'),
            onPressed: busy
                ? null
                : () => context.read<MailState>().refreshFolders(),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: SafeArea(
        child: CustomScrollView(
          keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
          slivers: [
            SliverPadding(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
              sliver: SliverToBoxAdapter(child: _header()),
            ),
            if (folders.isEmpty)
              SliverFillRemaining(hasScrollBody: false, child: _emptyHint())
            else
              SliverList.separated(
                itemCount: folders.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, i) => _row(
                  context,
                  context.read<MailState>(),
                  folders[i],
                  openId,
                ),
              ),
          ],
        ),
      ),
    );
  }

  Future<void> _create(MailState state) async {
    final path = _newFolder.text.trim();
    if (path.isEmpty || _creating) return;
    setState(() {
      _creating = true;
      _error = null;
    });
    final before = state.status;
    final finished = state.nextFinished('Folders');
    // `/` separates levels; the core maps it onto the account delimiter.
    await state.createFolder(path.replaceAll('\\', '/'));
    if (!mounted) return;
    // A refused queue reports straight to the status line and never finishes.
    if (state.statusIsError && state.status != before) {
      setState(() {
        _creating = false;
        _error = state.status;
      });
      return;
    }
    final e = await finished;
    if (!mounted) return;
    setState(() {
      _creating = false;
      // The name stays on failure, so a typo can be fixed and retried.
      if (e.ok) {
        _newFolder.clear();
      } else {
        _error = e.status.isEmpty ? 'Could not create the folder' : e.status;
      }
    });
  }
}
