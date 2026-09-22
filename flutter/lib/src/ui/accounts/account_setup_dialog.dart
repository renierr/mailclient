import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../state/mail_state.dart';

/// Add or edit an account.
///
/// The password field is always blank, including when editing: secrets live in
/// the OS keyring and cannot be read back, only overwritten. Leaving it empty
/// on an edit therefore keeps the stored one — the core relies on that, so the
/// hint says it out loud rather than making the user guess.
class AccountSetupDialog extends StatefulWidget {
  const AccountSetupDialog({super.key, this.accountId});

  /// Null for a new account.
  final int? accountId;

  static Future<bool> show(BuildContext context, {int? accountId}) async =>
      await showDialog<bool>(
        context: context,
        builder: (_) => AccountSetupDialog(accountId: accountId),
      ) ??
      false;

  @override
  State<AccountSetupDialog> createState() => _AccountSetupDialogState();
}

class _AccountSetupDialogState extends State<AccountSetupDialog> {
  final _form = GlobalKey<FormState>();
  final _fields = <String, TextEditingController>{
    for (final k in const [
      'name',
      'email',
      'from_name',
      'imap_host',
      'imap_port',
      'imap_user',
      'password',
      'smtp_host',
      'smtp_port',
      'smtp_user',
    ])
      k: TextEditingController(),
  };
  String _imapSec = 'ssl';
  String _smtpSec = 'ssl';
  bool _saving = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _fields['imap_port']!.text = '993';
    _fields['smtp_port']!.text = '465';
    if (widget.accountId != null) _loadExisting(widget.accountId!);
  }

  Future<void> _loadExisting(int id) async {
    final form = await MailCore.instance.accountForm(id);
    if (!mounted) return;
    setState(() {
      for (final entry in _fields.entries) {
        final value = form[entry.key];
        if (value is String) entry.value.text = value;
      }
      _imapSec = (form['imap_sec'] as String?) ?? _imapSec;
      _smtpSec = (form['smtp_sec'] as String?) ?? _smtpSec;
    });
  }

  @override
  void dispose() {
    for (final c in _fields.values) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.accountId != null;
    return AlertDialog(
      title: Text(editing ? 'Edit account' : 'Add account'),
      content: SizedBox(
        width: 460,
        child: Form(
          key: _form,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _text('email', 'Email address', required: true),
                _text('name', 'Account name (optional)'),
                _text('from_name', 'Sender display name (optional)'),
                const SizedBox(height: 12),
                _section('Incoming (IMAP)'),
                _text('imap_host', 'Host', required: true),
                Row(children: [
                  Expanded(child: _text('imap_port', 'Port')),
                  const SizedBox(width: 12),
                  Expanded(
                    child: _security(
                        _imapSec, (v) => setState(() => _imapSec = v)),
                  ),
                ]),
                _text('imap_user', 'Username'),
                _text(
                  'password',
                  editing ? 'Password (blank keeps the stored one)' : 'Password',
                  obscure: true,
                  required: !editing,
                ),
                const SizedBox(height: 12),
                _section('Outgoing (SMTP)'),
                _text('smtp_host', 'Host', required: true),
                Row(children: [
                  Expanded(child: _text('smtp_port', 'Port')),
                  const SizedBox(width: 12),
                  Expanded(
                    child: _security(
                        _smtpSec, (v) => setState(() => _smtpSec = v)),
                  ),
                ]),
                _text('smtp_user', 'Username (blank = same as IMAP)'),
                if (_error != null) ...[
                  const SizedBox(height: 12),
                  Text(_error!,
                      style:
                          TextStyle(color: Theme.of(context).colorScheme.error)),
                ],
              ],
            ),
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _saving ? null : () => Navigator.of(context).pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: _saving ? null : _save,
          child: Text(_saving ? 'Saving…' : 'Save'),
        ),
      ],
    );
  }

  Widget _section(String title) => Align(
        alignment: Alignment.centerLeft,
        child: Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: Text(title, style: Theme.of(context).textTheme.labelLarge),
        ),
      );

  Widget _text(
    String key,
    String label, {
    bool obscure = false,
    bool required = false,
  }) =>
      Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: TextFormField(
          controller: _fields[key],
          obscureText: obscure,
          decoration: InputDecoration(labelText: label, isDense: true),
          validator: required
              ? (v) => (v == null || v.trim().isEmpty) ? 'Required' : null
              : null,
        ),
      );

  Widget _security(String value, ValueChanged<String> onChanged) =>
      DropdownButtonFormField<String>(
        initialValue: value,
        isDense: true,
        decoration: const InputDecoration(labelText: 'Encryption'),
        items: const [
          DropdownMenuItem(value: 'ssl', child: Text('SSL/TLS')),
          DropdownMenuItem(value: 'starttls', child: Text('STARTTLS')),
          DropdownMenuItem(value: 'none', child: Text('None')),
        ],
        onChanged: (v) => onChanged(v ?? value),
      );

  Future<void> _save() async {
    if (!(_form.currentState?.validate() ?? false)) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final id = await MailCore.instance.saveAccount({
        for (final e in _fields.entries) e.key: e.value.text.trim(),
        'imap_sec': _imapSec,
        'smtp_sec': _smtpSec,
      });
      if (!mounted) return;
      await context.read<MailState>().accountsChanged(select: id);
      if (mounted) Navigator.of(context).pop(true);
    } catch (e) {
      // Everything the core rejects here is something the user can fix in this
      // dialog (a missing host, a keyring that will not open), so it belongs
      // next to the fields rather than in a snackbar that replaces the form.
      if (mounted) {
        setState(() {
          _saving = false;
          _error = e.toString().replaceFirst('Exception: ', '');
        });
      }
    }
  }
}
