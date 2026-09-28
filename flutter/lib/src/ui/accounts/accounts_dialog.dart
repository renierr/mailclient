import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../accounts/account_setup_dialog.dart';
import '../dialogs/mail_dialog.dart';

/// List, switch, edit and remove accounts.
///
/// Removing deletes the cached folders/messages locally and drops the keyring
/// secret; mail on the server is untouched, which the confirm dialog says out
/// loud.
class AccountsDialog extends StatelessWidget {
  const AccountsDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await MailDialog.show(context, builder: (_) => const AccountsDialog());
  }

  @override
  Widget build(BuildContext context) {
    final accounts = context.select<MailState, List<Account>>(
      (s) => s.accounts,
    );
    // Subscribed too, so the "active" marker follows an account switch.
    final activeId = context.select<MailState, int>((s) => s.accountId);
    return MailDialogShell(
      title: 'Accounts',
      maxWidth: 520,
      maxHeight: 520,
      scrollBody: false,
      body: accounts.isEmpty
          ? const Center(child: Text('No accounts yet.'))
          : ListView.separated(
              itemCount: accounts.length,
              separatorBuilder: (_, _) => const Divider(height: 1),
              itemBuilder: (context, i) => _row(
                context,
                context.read<MailState>(),
                accounts[i],
                activeId,
              ),
            ),
      actions: [
        OutlinedButton.icon(
          icon: const Icon(Icons.add, size: 16),
          label: const Text('Add account…'),
          onPressed: () => AccountSetupDialog.show(context),
        ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }

  Widget _accountButton({
    required String tooltip,
    required IconData icon,
    required VoidCallback onPressed,
  }) {
    // Compact 32px targets: three full-size IconButtons plus an "active" chip
    // overflow the trailing slot on narrow dialogs (RenderFlex).
    return IconButton(
      tooltip: tooltip,
      iconSize: 18,
      padding: const EdgeInsets.all(4),
      constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
      icon: Icon(icon),
      onPressed: onPressed,
    );
  }

  Widget _row(BuildContext context, MailState state, Account a, int activeId) {
    final current = a.id == activeId;
    final avatarBg = avatarColor(context, a.email);
    return ListTile(
      dense: true,
      leading: CircleAvatar(
        backgroundColor: avatarBg,
        foregroundColor: Theme.of(context).colorScheme.onPrimary,
        child: Text(
          a.email.isEmpty ? '?' : a.email[0].toUpperCase(),
          style: const TextStyle(fontWeight: FontWeight.w600),
        ),
      ),
      // Plain title: the "active" chip moved to the subtitle, where it costs
      // no horizontal width next to up to three trailing buttons.
      title: Text(a.email, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        '${a.displayName} · ${a.imapHost}${current ? ' · active' : ''}',
        overflow: TextOverflow.ellipsis,
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (!current)
            _accountButton(
              tooltip: 'Use this account',
              icon: Icons.check,
              onPressed: () {
                Navigator.of(context).pop();
                state.selectAccount(a.id);
              },
            ),
          _accountButton(
            tooltip: 'Edit',
            icon: Icons.edit_outlined,
            onPressed: () => AccountSetupDialog.show(context, accountId: a.id),
          ),
          _accountButton(
            tooltip: 'Remove',
            icon: Icons.delete_outline,
            onPressed: () => _confirmRemove(context, state, a),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmRemove(
    BuildContext context,
    MailState state,
    Account a,
  ) async {
    final confirmed =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Remove account?'),
            content: Text(
              'Remove ${a.email}? Its cached folders and messages are deleted locally and the password is removed from the OS keyring. Mail on the server is untouched.',
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                style: MailDialog.dangerStyle(context),
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
