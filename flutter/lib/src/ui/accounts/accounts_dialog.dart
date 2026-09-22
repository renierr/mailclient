import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../accounts/account_setup_dialog.dart';

/// List, switch, edit and remove accounts.
///
/// Removing deletes the cached folders/messages locally and drops the keyring
/// secret; mail on the server is untouched, which the confirm dialog says out
/// loud.
class AccountsDialog extends StatelessWidget {
  const AccountsDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await showDialog(
      context: context,
      builder: (_) => const AccountsDialog(),
    );
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    return Dialog(
      insetPadding:
          const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520, maxHeight: 520),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Accounts',
                  style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 12),
              Expanded(
                child: state.accounts.isEmpty
                    ? const Center(child: Text('No accounts yet.'))
                    : ListView.separated(
                        itemCount: state.accounts.length,
                        separatorBuilder: (_, _) =>
                            const Divider(height: 1),
                        itemBuilder: (context, i) =>
                            _row(context, state, state.accounts[i]),
                      ),
              ),
              const SizedBox(height: 8),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  OutlinedButton.icon(
                    icon: const Icon(Icons.add, size: 16),
                    label: const Text('Add account…'),
                    onPressed: () =>
                        AccountSetupDialog.show(context),
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

  Widget _row(BuildContext context, MailState state, Account a) {
    final current = a.id == state.accountId;
    return ListTile(
      leading: const Icon(Icons.account_circle_outlined),
      title: Row(
        children: [
          Expanded(
              child: Text(a.email,
                  overflow: TextOverflow.ellipsis)),
          if (current)
            const Chip(
              label: Text('active'),
              visualDensity: VisualDensity.compact,
            ),
        ],
      ),
      subtitle: Text(
          '${a.displayName} · ${a.imapHost}',
          overflow: TextOverflow.ellipsis),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (!current)
            IconButton(
              tooltip: 'Use this account',
              icon: const Icon(Icons.check, size: 18),
              onPressed: () {
                Navigator.of(context).pop();
                state.selectAccount(a.id);
              },
            ),
          IconButton(
            tooltip: 'Edit',
            icon: const Icon(Icons.edit_outlined, size: 18),
            onPressed: () =>
                AccountSetupDialog.show(context, accountId: a.id),
          ),
          IconButton(
            tooltip: 'Remove',
            icon: const Icon(Icons.delete_outline, size: 18),
            onPressed: () => _confirmRemove(context, state, a),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmRemove(
      BuildContext context, MailState state, Account a) async {
    final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('Remove account?'),
            content: Text(
                'Remove ${a.email}? Its cached folders and messages are deleted locally and the password is removed from the OS keyring. Mail on the server is untouched.'),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(true),
                child: const Text('Remove'),
              ),
            ],
          ),
        ) ??
        false;
    if (!confirmed || !context.mounted) return;
    try {
      await state.removeAccount(a.id);
    } catch (e) {
      if (!context.mounted) return;
      state.showStatus('$e', isError: true);
    }
  }
}
