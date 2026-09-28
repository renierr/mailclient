import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';

/// Add or edit an account.
///
/// The password field is always blank, including when editing: secrets live in
/// the OS keyring and cannot be read back, only overwritten. Leaving it empty
/// on an edit therefore keeps the stored one — the core relies on that, so the
/// hint says it out loud rather than making the user guess.
class AccountSetupDialog extends StatefulWidget {
  const AccountSetupDialog({
    super.key,
    this.accountId,
    this.fullscreen = false,
  });

  /// Null for a new account.
  final int? accountId;

  /// Fullscreen Scaffold page instead of a floating dialog. Used on phones,
  /// where a dialog plus the on-screen keyboard leaves no usable room.
  final bool fullscreen;

  static Future<bool> show(BuildContext context, {int? accountId}) async {
    return await MailDialog.showForm<bool>(
          context,
          // A tap outside must never drop a half-typed account form.
          barrierDismissible: false,
          dialog: (_) => AccountSetupDialog(accountId: accountId),
          page: (_) =>
              AccountSetupDialog(accountId: accountId, fullscreen: true),
        ) ??
        false;
  }

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
      'smtp_password',
    ])
      k: TextEditingController(),
  };
  String _imapSec = 'tls';
  String _smtpSec = 'tls';
  bool _saving = false;
  String? _error;
  final _revealed = <String>{};
  bool _guessed = false;

  @override
  void initState() {
    super.initState();
    _fields['imap_port']!.text = '993';
    _fields['smtp_port']!.text = '465';
    _fields['email']!.addListener(_maybeGuess);
    if (widget.accountId != null) _loadExisting(widget.accountId!);
  }

  Future<void> _loadExisting(int id) async {
    final form = await MailCore.instance.accountForm(id);
    if (!mounted) return;
    _fields['email']!.removeListener(_maybeGuess);
    setState(() {
      for (final entry in _fields.entries) {
        if (entry.key == 'password' || entry.key == 'smtp_password') continue;
        final value = form[entry.key];
        if (value is String) entry.value.text = value;
      }
      // Older builds stored 'ssl'; the core canonical value is 'tls'.
      _imapSec = _normSec(form['imap_sec'] as String?) ?? _imapSec;
      _smtpSec = _normSec(form['smtp_sec'] as String?) ?? _smtpSec;
      _guessed = true;
    });
  }

  static String? _normSec(String? v) {
    final s = (v ?? '').trim().toLowerCase();
    if (s.isEmpty) return null;
    if (s == 'ssl') return 'tls';
    if (s == 'tls' || s == 'starttls' || s == 'none') return s;
    return s;
  }

  /// Qt parity: typing the address once fills host/user guesses, so a standard
  /// provider needs only the password. Never overwrites an edited field.
  void _maybeGuess() {
    if (_guessed || widget.accountId != null) return;
    final email = _fields['email']!.text.trim();
    final at = email.indexOf('@');
    if (at <= 0 || !email.substring(at).contains('.')) return;
    final domain = email.substring(at + 1);
    _guessed = true;
    if (_fields['imap_host']!.text.isEmpty) {
      _fields['imap_host']!.text = 'mail.$domain';
    }
    if (_fields['smtp_host']!.text.isEmpty) {
      _fields['smtp_host']!.text = 'mail.$domain';
    }
    if (_fields['imap_user']!.text.isEmpty) {
      _fields['imap_user']!.text = email;
    }
  }

  void _onImapSec(String v) => setState(() {
    _imapSec = v;
    // Qt port auto-swap: standard ports follow the encryption.
    if (v == 'tls' && _fields['imap_port']!.text == '143') {
      _fields['imap_port']!.text = '993';
    } else if (v != 'tls' && _fields['imap_port']!.text == '993') {
      _fields['imap_port']!.text = '143';
    }
  });

  void _onSmtpSec(String v) => setState(() {
    _smtpSec = v;
    if (v == 'tls' && _fields['smtp_port']!.text == '587') {
      _fields['smtp_port']!.text = '465';
    } else if (v != 'tls' && _fields['smtp_port']!.text == '465') {
      _fields['smtp_port']!.text = '587';
    }
  });

  @override
  void dispose() {
    _fields['email']!.removeListener(_maybeGuess);
    for (final c in _fields.values) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (widget.fullscreen) return _page();
    final maxW = MailDialog.maxWidth(context, 480);
    final maxH = MailDialog.maxHeight(context, 600);
    return Dialog(
      insetPadding: MailDialog.insets(context),
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: maxW, maxHeight: maxH),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                widget.accountId != null ? 'Edit account' : 'Add account',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 8),
              Flexible(
                child: Form(
                  key: _form,
                  // No viewInsets padding here: Dialog already pads for the
                  // keyboard, and a second padding collapses the form.
                  child: SingleChildScrollView(child: _fieldsColumn()),
                ),
              ),
              const SizedBox(height: 8),
              // Wrap, not Row: the buttons stack instead of overflowing at
              // large text scales.
              Wrap(
                alignment: WrapAlignment.end,
                spacing: 8,
                children: [
                  TextButton(
                    onPressed: _saving
                        ? null
                        : () => Navigator.of(context).pop(false),
                    child: const Text('Cancel'),
                  ),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: Text(_saving ? 'Saving…' : 'Save'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Fullscreen form page for phones: the Scaffold shrinks for the keyboard
  /// natively, so every field stays reachable while typing.
  Widget _page() {
    final editing = widget.accountId != null;
    return Scaffold(
      appBar: AppBar(
        title: Text(editing ? 'Edit account' : 'Add account'),
        actions: [
          TextButton(
            onPressed: _saving ? null : () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: _saving ? null : _save,
            child: Text(_saving ? 'Saving…' : 'Save'),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: SafeArea(
        // Dismiss the keyboard on drag so the Save button is always one tap
        // away, even mid-form on a short screen.
        child: GestureDetector(
          onTap: () => FocusScope.of(context).unfocus(),
          child: Form(
            key: _form,
            child: SingleChildScrollView(
              keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
              padding: const EdgeInsets.all(16),
              child: _fieldsColumn(),
            ),
          ),
        ),
      ),
    );
  }

  /// The fields, shared by the dialog and the fullscreen page.
  Widget _fieldsColumn() {
    final editing = widget.accountId != null;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        _text(
          'email',
          'Email address',
          required: true,
          email: true,
          keyboard: TextInputType.emailAddress,
        ),
        _text('name', 'Account name (optional)', keyboard: TextInputType.text),
        _text(
          'from_name',
          'Sender display name (optional)',
          keyboard: TextInputType.text,
        ),
        const SizedBox(height: 12),
        _section('Incoming (IMAP)'),
        _text('imap_host', 'Host', required: true),
        _portSecurity('imap_port', _imapSec, _onImapSec),
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
        _portSecurity('smtp_port', _smtpSec, _onSmtpSec),
        _text('smtp_user', 'Username (blank = same as IMAP)'),
        _text(
          'smtp_password',
          editing
              ? 'SMTP password (blank keeps the stored one)'
              : 'SMTP password (blank = same as IMAP)',
          obscure: true,
        ),
        if (_error != null) ...[
          const SizedBox(height: 12),
          Text(
            _error!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ],
    );
  }

  /// Port beside encryption on wide screens, stacked full-width on
  /// narrow/zoomed layouts where the Row squeezes both unreadably thin.
  Widget _portSecurity(String portKey, String sec, ValueChanged<String> onSec) {
    final port = _text(
      portKey,
      'Port',
      port: true,
      keyboard: TextInputType.number,
    );
    final security = _security(sec, onSec);
    if (MailDialog.isNarrow(context)) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [port, security],
      );
    }
    return Row(
      children: [
        Expanded(child: port),
        const SizedBox(width: 12),
        Expanded(child: security),
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
    bool email = false,
    bool port = false,
    TextInputType? keyboard,
  }) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: TextFormField(
      controller: _fields[key],
      obscureText: obscure && !_revealed.contains(key),
      keyboardType: keyboard,
      textInputAction: TextInputAction.next,
      decoration: InputDecoration(
        labelText: label,
        isDense: true,
        suffixIcon: obscure
            ? IconButton(
                tooltip: _revealed.contains(key) ? 'Hide' : 'Show',
                icon: Icon(
                  _revealed.contains(key)
                      ? Icons.visibility_off_outlined
                      : Icons.visibility_outlined,
                  size: 20,
                ),
                onPressed: () => setState(() {
                  _revealed.contains(key)
                      ? _revealed.remove(key)
                      : _revealed.add(key);
                }),
              )
            : null,
      ),
      validator: (v) {
        final t = (v ?? '').trim();
        if (required && t.isEmpty) return 'Required';
        if (email && t.isNotEmpty && !t.contains('@')) {
          return 'Enter a full email address';
        }
        if (port && t.isNotEmpty && int.tryParse(t) == null) {
          return 'Port must be a number';
        }
        return null;
      },
    ),
  );

  Widget _security(String value, ValueChanged<String> onChanged) =>
      DropdownButtonFormField<String>(
        initialValue: value,
        isDense: true,
        decoration: const InputDecoration(labelText: 'Encryption'),
        items: const [
          DropdownMenuItem(value: 'tls', child: Text('SSL/TLS')),
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
        // Without the id, changing the address creates a second account
        // instead of renaming this one.
        if (widget.accountId != null) 'id': widget.accountId,
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
