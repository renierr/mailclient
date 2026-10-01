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
  List<String> _securityChoices = const ['tls', 'starttls', 'none'];

  /// Per-field errors from the core's check, set by [_save] right before
  /// the form validates.
  Map<String, String> _fieldErrors = const {};

  /// Per-field warnings (an unencrypted connection), kept current.
  Map<String, String> _warnings = const {};
  bool _saving = false;
  String? _error;
  final _revealed = <String>{};
  bool _guessed = false;

  /// Anything typed or changed since open (or since the stored account
  /// loaded) — back and Cancel then ask before dropping it.
  bool _dirty = false;

  void _edited() {
    if (!_dirty) setState(() => _dirty = true);
  }

  @override
  void initState() {
    super.initState();
    // Starting values and choices come from the core, shared with Qt.
    final d = MailCore.instance.accountFormDefaults();
    _fields['imap_port']!.text = d['imap_port'] as String? ?? '';
    _fields['smtp_port']!.text = d['smtp_port'] as String? ?? '';
    _imapSec = d['imap_sec'] as String? ?? _imapSec;
    _smtpSec = d['smtp_sec'] as String? ?? _smtpSec;
    _securityChoices =
        (d['security_choices'] as List?)?.cast<String>() ?? _securityChoices;
    for (final c in _fields.values) {
      c.addListener(_edited);
    }
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
      // The core already normalized older values ('ssl', 'plain').
      _imapSec = form['imap_sec'] as String? ?? _imapSec;
      _smtpSec = form['smtp_sec'] as String? ?? _smtpSec;
      _guessed = true;
      // Filling in the stored values is not an edit.
      _dirty = false;
    });
    _refreshWarnings();
  }

  /// Qt parity: typing the address once fills the core's host/user guesses,
  /// so a standard provider needs only the password. Never overwrites an
  /// edited field.
  void _maybeGuess() {
    if (_guessed || widget.accountId != null) return;
    final g = MailCore.instance.accountGuess(_fields['email']!.text);
    if (g.isEmpty) return;
    _guessed = true;
    for (final key in const ['imap_host', 'smtp_host', 'imap_user']) {
      if (_fields[key]!.text.isEmpty) _fields[key]!.text = g[key] as String;
    }
  }

  /// Standard ports follow the encryption (the core decides; a port the
  /// user chose stays).
  String _portFor(String protocol, String oldSec, String newSec) {
    final field = _fields['${protocol}_port']!;
    return MailCore.instance.accountPortForSecurity(
      protocol,
      oldSec,
      newSec,
      field.text,
    );
  }

  void _onImapSec(String v) {
    setState(() {
      _fields['imap_port']!.text = _portFor('imap', _imapSec, v);
      _imapSec = v;
      _dirty = true;
    });
    _refreshWarnings();
  }

  void _onSmtpSec(String v) {
    setState(() {
      _fields['smtp_port']!.text = _portFor('smtp', _smtpSec, v);
      _smtpSec = v;
      _dirty = true;
    });
    _refreshWarnings();
  }

  static bool _isSecret(String key) =>
      key == 'password' || key == 'smtp_password';

  Map<String, dynamic> _payload() => {
    // Passwords go verbatim: a space at either end can be part of one.
    for (final e in _fields.entries)
      e.key: _isSecret(e.key) ? e.value.text : e.value.text.trim(),
    'imap_sec': _imapSec,
    'smtp_sec': _smtpSec,
    // Without the id, changing the address creates a second account
    // instead of renaming this one.
    if (widget.accountId != null) 'id': widget.accountId,
  };

  void _refreshWarnings() {
    final w = MailCore.instance
        .accountFormCheck(_payload(), editing: widget.accountId != null)
        .warnings;
    setState(() => _warnings = w);
  }

  @override
  void dispose() {
    _fields['email']!.removeListener(_maybeGuess);
    for (final c in _fields.values) {
      c.dispose();
    }
    super.dispose();
  }

  /// Same guard as the composer: system back, the page's close button and
  /// Cancel all come through here, so a half-typed account is never dropped
  /// without asking.
  Widget _guarded(Widget child) => PopScope(
    canPop: !_saving && !_dirty,
    onPopInvokedWithResult: (didPop, _) {
      if (!didPop && !_saving) _maybeClose();
    },
    child: child,
  );

  Future<void> _maybeClose() async {
    if (!_dirty) {
      Navigator.of(context).pop(false);
      return;
    }
    final discard =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Discard changes?'),
            content: const Text('The account form has unsaved changes.'),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Keep editing'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(true),
                child: const Text('Discard'),
              ),
            ],
          ),
        ) ??
        false;
    if (discard && mounted) Navigator.of(context).pop(false);
  }

  Widget _errorLine() => Padding(
    padding: const EdgeInsets.only(bottom: 8),
    child: Text(
      _error!,
      style: TextStyle(color: Theme.of(context).colorScheme.error),
    ),
  );

  @override
  Widget build(BuildContext context) =>
      _guarded(widget.fullscreen ? _page() : _dialog());

  Widget _dialog() {
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
              // Outside the scrolling fields, so a rejected save is visible
              // however far down the form is scrolled.
              if (_error != null) _errorLine(),
              // Wrap, not Row: the buttons stack instead of overflowing at
              // large text scales.
              Wrap(
                alignment: WrapAlignment.end,
                spacing: 8,
                children: [
                  TextButton(
                    onPressed: _saving ? null : _maybeClose,
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
            onPressed: _saving ? null : _maybeClose,
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
              // Save sits in the AppBar, so the error goes at the top rather
              // than below a long form.
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [if (_error != null) _errorLine(), _fieldsColumn()],
              ),
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
        _text('email', 'Email address', keyboard: TextInputType.emailAddress),
        _text('name', 'Account name (optional)', keyboard: TextInputType.text),
        _text(
          'from_name',
          'Sender display name (optional)',
          keyboard: TextInputType.text,
        ),
        const SizedBox(height: 12),
        _section('Incoming (IMAP)'),
        _text('imap_host', 'Host'),
        _portSecurity('imap_port', _imapSec, _onImapSec),
        _text('imap_user', 'Username'),
        _text(
          'password',
          editing ? 'Password (blank keeps the stored one)' : 'Password',
          obscure: true,
        ),
        const SizedBox(height: 12),
        _section('Outgoing (SMTP)'),
        _text('smtp_host', 'Host'),
        _portSecurity('smtp_port', _smtpSec, _onSmtpSec),
        _text('smtp_user', 'Username (blank = same as IMAP)'),
        _text(
          'smtp_password',
          editing
              ? 'SMTP password (blank keeps the stored one)'
              : 'SMTP password (blank = same as IMAP)',
          obscure: true,
        ),
      ],
    );
  }

  /// Port beside encryption on wide screens, stacked full-width on
  /// narrow/zoomed layouts where the Row squeezes both unreadably thin.
  Widget _portSecurity(String portKey, String sec, ValueChanged<String> onSec) {
    final port = _text(portKey, 'Port', keyboard: TextInputType.number);
    final protocol = portKey == 'imap_port' ? 'imap' : 'smtp';
    final security = _security(sec, onSec, _warnings['${protocol}_sec']);
    if (MailDialog.isNarrow(context)) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [port, security],
      );
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
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
    TextInputType? keyboard,
  }) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: TextFormField(
      controller: _fields[key],
      obscureText: obscure && !_revealed.contains(key),
      // A revealed password is plain text to the keyboard: keep it from
      // learning or suggesting it, and let password managers fill it.
      autocorrect: !obscure,
      enableSuggestions: !obscure,
      autofillHints: obscure ? const [AutofillHints.password] : null,
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
      // The core's check, run by [_save] just before validating.
      validator: (_) => _fieldErrors[key],
    ),
  );

  static String _securityLabel(String value) => switch (value) {
    'starttls' => 'STARTTLS',
    'none' => 'None (unencrypted)',
    _ => 'SSL/TLS',
  };

  Widget _security(
    String value,
    ValueChanged<String> onChanged,
    String? warning,
  ) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      DropdownButtonFormField<String>(
        initialValue: value,
        isDense: true,
        isExpanded: true,
        decoration: const InputDecoration(labelText: 'Encryption'),
        items: [
          for (final v in _securityChoices)
            DropdownMenuItem(
              value: v,
              child: Text(_securityLabel(v), overflow: TextOverflow.ellipsis),
            ),
        ],
        onChanged: (v) => onChanged(v ?? value),
      ),
      if (warning != null)
        Padding(
          padding: const EdgeInsets.only(top: 4),
          child: Text(
            warning,
            style: TextStyle(
              color: Theme.of(context).brightness == Brightness.dark
                  ? Colors.amber.shade300
                  : Colors.amber.shade900,
              fontSize: 12,
            ),
          ),
        ),
    ],
  );

  Future<void> _save() async {
    _fieldErrors = MailCore.instance
        .accountFormCheck(_payload(), editing: widget.accountId != null)
        .errors;
    if (!(_form.currentState?.validate() ?? false)) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final id = await MailCore.instance.saveAccount(_payload());
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
