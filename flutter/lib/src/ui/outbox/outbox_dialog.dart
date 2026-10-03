import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// Unsent mail for the open account: queued, sending or failed sends.
///
/// A sync retries everything still deliverable; a failed send the user
/// already saw keeps no bytes and must be resent by hand — dismissing it
/// here only forgets the dead row.
class OutboxDialog extends StatefulWidget {
  const OutboxDialog({super.key, this.fullscreen = false});

  /// Fullscreen page instead of a floating dialog — used on phones, where a
  /// dialog plus the on-screen keyboard leaves no usable room.
  final bool fullscreen;

  static Future<void> show(BuildContext context) async {
    await MailDialog.showForm(
      context,
      dialog: (_) => const OutboxDialog(),
      page: (_) => const OutboxDialog(fullscreen: true),
    );
  }

  @override
  State<OutboxDialog> createState() => _OutboxDialogState();
}

class _OutboxDialogState extends State<OutboxDialog> {
  List<OutboxEntry> _entries = const [];
  bool _loaded = false;
  bool _syncing = false;

  /// Why the last sync-now failed, shown under the hint — the status bar is
  /// behind this dialog (and a whole page away on phones).
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final entries = await context.read<MailState>().outboxEntries();
    if (!mounted) return;
    setState(() {
      _entries = entries;
      _loaded = true;
    });
  }

  Future<void> _dismiss(MailState state, int id) async {
    await state.dismissOutbox(id);
    if (!mounted) return;
    await _load();
  }

  Future<void> _syncNow(MailState state) async {
    if (_syncing) return;
    setState(() {
      _syncing = true;
      _error = null;
    });
    final before = state.status;
    final finished = state.nextFinished('Sync');
    await state.syncAccount();
    if (!mounted) return;
    // A refused queue reports straight to the status line and never finishes.
    if (state.statusIsError && state.status != before) {
      setState(() {
        _syncing = false;
        _error = state.status;
      });
      return;
    }
    await finished;
    if (!mounted) return;
    setState(() => _syncing = false);
    await _load();
  }

  Widget _row(BuildContext context, MailState state, OutboxEntry e) {
    final scheme = Theme.of(context).colorScheme;
    final failed = e.status == 'failed';
    final to = e.envelopeTo.join(', ');
    return ListTile(
      contentPadding: EdgeInsets.zero,
      dense: true,
      title: Text(
        e.subject.isNotEmpty ? e.subject : '(no subject)',
        overflow: TextOverflow.ellipsis,
        style: Theme.of(
          context,
        ).textTheme.titleSmall?.copyWith(fontWeight: FontWeight.bold),
      ),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (to.isNotEmpty)
            Text(to, maxLines: 1, overflow: TextOverflow.ellipsis),
          if (e.state.isNotEmpty) Text(e.state),
          if (e.lastError.isNotEmpty)
            Text(e.lastError, style: TextStyle(color: scheme.error)),
        ],
      ),
      // Qt parity: dismissing forgets the dead row, never resends.
      trailing: IconButton(
        tooltip: 'Forget this entry',
        icon: const Icon(Icons.close, size: 20),
        onPressed: failed || e.status == 'queued'
            ? () => _dismiss(state, e.id)
            : null,
      ),
    );
  }

  Widget _hint() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Mail waiting to send, or sends that failed. '
          'Syncing retries everything still deliverable.',
          style: Theme.of(context).textTheme.bodySmall,
        ),
        if (_error case final String error) ...[
          const SizedBox(height: 4),
          Text(
            error,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ],
    );
  }

  Widget _listBody(MailState state) {
    if (!_loaded) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_entries.isEmpty) return _emptyHint();
    return ListView.separated(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      itemCount: _entries.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) =>
          _row(context, state, _entries[i]),
    );
  }

  Widget _emptyHint() => const Center(child: Text('Outbox is empty'));

  /// Sync-now + Close actions, shared by dialog and page. Wrap, not Row:
  /// the buttons stack instead of overflowing on a very narrow dialog.
  Widget _actions(MailState state) {
    final retryable =
        context.select<MailState, int>((s) => s.outboxStatus?.retryable ?? 0);
    return Wrap(
      alignment: WrapAlignment.end,
      spacing: 8,
      children: [
        OutlinedButton.icon(
          icon: _syncing
              ? const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.sync, size: 16),
          label: const Text('Sync now'),
          onPressed: _syncing || retryable == 0
              ? null
              : () => _syncNow(state),
        ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    if (widget.fullscreen) return _page();
    final state = context.read<MailState>();
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
                'Outbox',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 12),
              _hint(),
              const SizedBox(height: 8),
              Expanded(child: _listBody(state)),
              const SizedBox(height: 8),
              _actions(state),
            ],
          ),
        ),
      ),
    );
  }

  /// Fullscreen outbox for phones (see [OutboxDialog.fullscreen]).
  /// Header and rows scroll as one lazy sliver list: a fixed header over an
  /// Expanded list overflows a landscape phone once the keyboard is up.
  Widget _page() {
    final state = context.read<MailState>();
    return Scaffold(
      appBar: AppBar(
        title: const Text('Outbox'),
        actions: [
          OutlinedButton.icon(
            icon: _syncing
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.sync, size: 16),
            label: const Text('Sync'),
            onPressed: _syncing ? null : () => _syncNow(state),
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
              sliver: SliverToBoxAdapter(child: _hint()),
            ),
            if (!_loaded)
              const SliverFillRemaining(
                hasScrollBody: false,
                child: Center(child: CircularProgressIndicator()),
              )
            else if (_entries.isEmpty)
              SliverFillRemaining(hasScrollBody: false, child: _emptyHint())
            else
              SliverList.separated(
                itemCount: _entries.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, i) =>
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: _row(context, state, _entries[i]),
                    ),
              ),
          ],
        ),
      ),
    );
  }
}
