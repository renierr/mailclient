import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// Unsent mail for the open account: queued, sending or failed sends.
///
/// A sync retries everything still deliverable; a failed send the user
/// already saw keeps no bytes and must be resent by hand — dismissing it
/// here only forgets the dead row. No text field, so a plain dialog on every
/// width (§3: only flows with a `TextField` become pages).
class OutboxDialog extends StatefulWidget {
  const OutboxDialog({super.key});

  static Future<void> show(BuildContext context) =>
      MailDialog.show<void>(context, builder: (_) => const OutboxDialog());

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

  late final MailState _state;

  /// The pill's counts when the rows were last read: a send or sync moving
  /// rows changes them, and the open dialog re-reads (Qt reloads on the same
  /// job finishes).
  String? _seen;

  @override
  void initState() {
    super.initState();
    _state = context.read<MailState>()..addListener(_onState);
    _load();
  }

  @override
  void dispose() {
    _state.removeListener(_onState);
    super.dispose();
  }

  String _countsKey() {
    final s = _state.outboxStatus;
    return '${s?.queued}/${s?.sending}/${s?.failed}/${s?.retryable}';
  }

  void _onState() {
    if (_loaded && _countsKey() != _seen) _load();
  }

  Future<void> _load() async {
    _seen = _countsKey();
    final entries = await _state.outboxEntries();
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
    final to = e.envelopeTo.join(', ');
    return ListTile(
      contentPadding: EdgeInsets.zero,
      dense: true,
      title: Text(
        e.subject,
        overflow: TextOverflow.ellipsis,
        style: Theme.of(context).textTheme.titleSmall
            ?.copyWith(fontWeight: FontWeight.bold),
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
        onPressed: e.dismissable ? () => _dismiss(state, e.id) : null,
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
      itemCount: _entries.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) => _row(context, state, _entries[i]),
    );
  }

  Widget _emptyHint() => const Center(child: Text('Outbox is empty'));

  /// Sync-now + Close actions, shared by dialog and page. Wrap, not Row:
  /// the buttons stack instead of overflowing on a very narrow dialog.
  Widget _actions(MailState state) {
    final retryable = context.select<MailState, int>(
      (s) => s.outboxStatus?.retryable ?? 0,
    );
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
          onPressed: _syncing || retryable == 0 ? null : () => _syncNow(state),
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
              Text('Outbox', style: Theme.of(context).textTheme.titleLarge),
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
}
