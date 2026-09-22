import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';

/// All preferences, Roundcube-style: sections on the left, the form on the
/// right. Everything edits a local copy; Save writes it through, Cancel
/// reverts by writing nothing at all.
class SettingsDialog extends StatefulWidget {
  const SettingsDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await showDialog(
      context: context,
      builder: (_) => const SettingsDialog(),
    );
  }

  @override
  State<SettingsDialog> createState() => _SettingsDialogState();
}

enum _Section { interface, mailbox, reading, composing, sync, about }

class _SettingsDialogState extends State<SettingsDialog> {
  _Section _section = _Section.interface;
  late AppSettings _draft;
  late final TextEditingController _signature;
  bool _saving = false;
  String? _error;
  int? _capsAccountId;

  @override
  void initState() {
    super.initState();
    _draft = context.read<MailState>().settings;
    _signature = TextEditingController(text: _draft.signatureText);
  }

  @override
  void dispose() {
    _signature.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final width = MediaQuery.sizeOf(context).width;
    final narrow = width < 720;
    return Dialog(
      insetPadding:
          EdgeInsets.symmetric(horizontal: narrow ? 8 : 40, vertical: 24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 800, maxHeight: 640),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Settings',
                  style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 12),
              if (narrow)
                DropdownButton<_Section>(
                  value: _section,
                  isExpanded: true,
                  items: [
                    for (final s in _Section.values)
                      DropdownMenuItem(value: s, child: Text(_label(s))),
                  ],
                  onChanged: (s) =>
                      setState(() => _section = s ?? _section),
                ),
              Expanded(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (!narrow)
                      NavigationRail(
                        selectedIndex: _Section.values.indexOf(_section),
                        onDestinationSelected: (i) => setState(
                            () => _section = _Section.values[i]),
                        labelType: NavigationRailLabelType.all,
                        destinations: [
                          for (final s in _Section.values)
                            NavigationRailDestination(
                              icon: Icon(_icon(s)),
                              label: Text(_label(s)),
                            ),
                        ],
                      ),
                    const VerticalDivider(width: 1),
                    Expanded(
                      child: SingleChildScrollView(
                        padding:
                            const EdgeInsets.symmetric(horizontal: 16),
                        child: _body(),
                      ),
                    ),
                  ],
                ),
              ),
              if (_error != null)
                Text(_error!,
                    style: TextStyle(
                        color: Theme.of(context).colorScheme.error)),
              const SizedBox(height: 8),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: _saving
                        ? null
                        : () => Navigator.of(context).pop(),
                    child: const Text('Cancel'),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: const Text('Save'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _body() => switch (_section) {
        _Section.interface => _interface(),
        _Section.mailbox => _mailbox(),
        _Section.reading => _reading(),
        _Section.composing => _composing(),
        _Section.sync => _sync(),
        _Section.about => _about(),
      };

  Widget _interface() => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _choice<double>(
            'Interface scale',
            _draft.uiScale,
            const [1.0, 1.1, 1.25, 1.5],
            (v) => '${(v * 100).round()}%',
            (v) => setState(() => _draft = _draft.copyWith(uiScale: v)),
          ),
          _choice<String>(
            'Mail text size',
            _draft.readerFontSize,
            const ['small', 'normal', 'large'],
            (v) => v[0].toUpperCase() + v.substring(1),
            (v) =>
                setState(() => _draft = _draft.copyWith(readerFontSize: v)),
            help: 'Plain-text messages only.',
          ),
        ],
      );

  Widget _mailbox() => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _choice<String>(
            'Sort messages by',
            _draft.sortField,
            const ['date', 'from', 'subject'],
            (v) => {'date': 'Date', 'from': 'Sender', 'subject': 'Subject'}[v]!,
            (v) => setState(() => _draft = _draft.copyWith(sortField: v)),
          ),
          _choice<bool>(
            'Order',
            _draft.sortDescending,
            const [true, false],
            (v) => v ? 'Newest first' : 'Oldest first',
            (v) =>
                setState(() => _draft = _draft.copyWith(sortDescending: v)),
          ),
          _choice<String>(
            'Density',
            _draft.density,
            const ['comfortable', 'compact'],
            (v) => v[0].toUpperCase() + v.substring(1),
            (v) => setState(() => _draft = _draft.copyWith(density: v)),
            help: 'Compact hides the preview line.',
          ),
          _switch(
            'Confirm before moving mail to Trash',
            _draft.confirmDelete,
            (v) => setState(() => _draft = _draft.copyWith(confirmDelete: v)),
            help:
                'Single mails, selections and the Delete key ask first. Permanent deletes always ask.',
          ),
        ],
      );

  Widget _reading() => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _switch(
            'Automatically mark messages as read',
            _draft.autoMarkRead,
            (v) => setState(() => _draft = _draft.copyWith(autoMarkRead: v)),
          ),
          _choice<int>(
            'Mark as read',
            _draft.markReadDelaySecs,
            const [0, 3, 5, 10, 30],
            (v) => v == 0 ? 'Immediately' : 'After ${v}s',
            (v) => setState(
                () => _draft = _draft.copyWith(markReadDelaySecs: v)),
            enabled: _draft.autoMarkRead,
            help:
                'With a delay, closing the message early keeps it unread.',
          ),
          _switch(
            'Load remote images (not recommended)',
            _draft.loadRemoteImages,
            (v) => setState(
                () => _draft = _draft.copyWith(loadRemoteImages: v)),
            help:
                'Remote images tell the sender you opened the message. Off means the Show-once banner.',
          ),
        ],
      );

  Widget _composing() => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _choice<String>(
            'Send mail as',
            _draft.sendFormat,
            const ['auto', 'plain', 'multipart', 'html'],
            (v) => {
              'auto': 'Automatic (recommended)',
              'plain': 'Plain text (safest)',
              'multipart': 'Multipart plain+HTML',
              'html': 'HTML only',
            }[v]!,
            (v) => setState(() => _draft = _draft.copyWith(sendFormat: v)),
            help:
                'Automatic sends plain text unless the body carries real formatting.',
          ),
          _switch(
            'Always include a plain-text version alongside HTML',
            _draft.includePlain,
            (v) => setState(
                () => _draft = _draft.copyWith(includePlain: v)),
          ),
          _choice<bool>(
            'Replies start',
            _draft.replyBelowQuote,
            const [false, true],
            (v) => v ? 'Below quote' : 'Above quote',
            (v) => setState(
                () => _draft = _draft.copyWith(replyBelowQuote: v)),
          ),
          _switch(
            'Use signature',
            _draft.signatureEnabled,
            (v) => setState(
                () => _draft = _draft.copyWith(signatureEnabled: v)),
          ),
          TextField(
            controller: _signature,
            maxLines: 3,
            onChanged: (v) =>
                _draft = _draft.copyWith(signatureText: v),
            decoration: const InputDecoration(
              labelText: 'Signature',
              helperText: 'Added after “-- ” to new mail, replies and forwards.',
            ),
          ),
          _switch(
            'Request read receipt',
            _draft.requestMdn,
            (v) =>
                setState(() => _draft = _draft.copyWith(requestMdn: v)),
            help:
                'Adds Disposition-Notification-To. Recipients may ignore it.',
          ),
        ],
      );

  Widget _sync() => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _switch(
            'Save a copy of sent mail in Sent',
            _draft.sentCopy,
            (v) =>
                setState(() => _draft = _draft.copyWith(sentCopy: v)),
          ),
          _switch(
            'Suggest recipients from sent mail',
            _draft.collectContacts,
            (v) => setState(
                () => _draft = _draft.copyWith(collectContacts: v)),
            help: 'Addresses from mail you sent power the composer.',
          ),
          _choice<int>(
            'Check for new mail',
            _draft.syncIntervalMinutes,
            const [0, 5, 10, 15, 30, 60],
            (v) => v == 0 ? 'Manually' : 'Every ${v}m',
            (v) => setState(
                () => _draft = _draft.copyWith(syncIntervalMinutes: v)),
          ),
        ],
      );

  Widget _about() {
    final state = context.watch<MailState>();
    final info = MailCore.instance.info;
    _capsAccountId ??= state.accountId;
    final caps = _capsAccountId == null
        ? null
        : state.capabilitiesFor(_capsAccountId!);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SelectableText('Mailclient ${info.version}'),
        SelectableText('License: ${info.license}'),
        SelectableText('Database: ${info.dbPath}'),
        const SizedBox(height: 16),
        Text('Server capabilities',
            style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        Row(
          children: [
            Expanded(
              child: DropdownButton<int>(
                value: state.accounts.any((a) => a.id == _capsAccountId)
                    ? _capsAccountId
                    : null,
                hint: const Text('Add an account first'),
                isExpanded: true,
                items: [
                  for (final a in state.accounts)
                    DropdownMenuItem(
                        value: a.id, child: Text(a.email)),
                ],
                onChanged: (id) =>
                    setState(() => _capsAccountId = id),
              ),
            ),
            const SizedBox(width: 8),
            OutlinedButton(
              onPressed: _capsAccountId == null || _capsAccountId! < 0
                  ? null
                  : () => state
                      .refreshCapabilities(_capsAccountId!),
              child: const Text('Refresh'),
            ),
          ],
        ),
        const SizedBox(height: 8),
        if (caps == null)
          const Text('No capabilities loaded yet — press Refresh.')
        else ...[
          SelectableText(
              '${caps['email'] ?? ''} · ${caps['imap_host'] ?? ''}'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final c in ((caps['capabilities'] as List?) ??
                  const []))
                Chip(
                  label: Text('$c',
                      style: const TextStyle(fontFamily: 'monospace')),
                  visualDensity: VisualDensity.compact,
                ),
            ],
          ),
        ],
      ],
    );
  }

  Future<void> _save() async {
    final state = context.read<MailState>();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final d = _draft;
      final writes = <String, String>{
        SettingKeys.sentCopy: _yn(d.sentCopy),
        SettingKeys.loadRemoteImages: _yn(d.loadRemoteImages),
        SettingKeys.sendFormat: d.sendFormat,
        SettingKeys.includePlain: _yn(d.includePlain),
        SettingKeys.autoMarkRead: _yn(d.autoMarkRead),
        SettingKeys.markReadDelay: '${d.markReadDelaySecs}',
        SettingKeys.collectContacts: _yn(d.collectContacts),
        SettingKeys.confirmDelete: _yn(d.confirmDelete),
        SettingKeys.listDensity: d.density,
        SettingKeys.readerFontSize: d.readerFontSize,
        SettingKeys.syncInterval: '${d.syncIntervalMinutes}',
        SettingKeys.signatureEnabled: _yn(d.signatureEnabled),
        SettingKeys.signatureText: d.signatureText,
        SettingKeys.replyBelowQuote: _yn(d.replyBelowQuote),
        SettingKeys.requestMdn: _yn(d.requestMdn),
        SettingKeys.uiScale: '${d.uiScale}',
      };
      for (final e in writes.entries) {
        await state.setSetting(e.key, e.value);
      }
      // Sort is its own call: the two keys only make sense together.
      await state.setSort(d.sortField, d.sortDescending);
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _saving = false;
        _error = e is Exception
            ? e.toString().replaceFirst('Exception: ', '')
            : '$e';
      });
    }
  }

  static String _yn(bool v) => v ? '1' : '0';

  static String _label(_Section s) => switch (s) {
        _Section.interface => 'Interface',
        _Section.mailbox => 'Mailbox',
        _Section.reading => 'Reading',
        _Section.composing => 'Composing',
        _Section.sync => 'Accounts & sync',
        _Section.about => 'About',
      };

  static IconData _icon(_Section s) => switch (s) {
        _Section.interface => Icons.tune_outlined,
        _Section.mailbox => Icons.inbox_outlined,
        _Section.reading => Icons.mark_email_read_outlined,
        _Section.composing => Icons.edit_outlined,
        _Section.sync => Icons.sync_outlined,
        _Section.about => Icons.info_outline,
      };

  Widget _switch(String title, bool value, ValueChanged<bool> onChanged,
      {String? help}) {
    return SwitchListTile(
      title: Text(title),
      subtitle: help == null ? null : Text(help),
      value: value,
      onChanged: onChanged,
      contentPadding: EdgeInsets.zero,
    );
  }

  Widget _choice<T>(
    String title,
    T value,
    List<T> options,
    String Function(T) label,
    ValueChanged<T> onChanged, {
    String? help,
    bool enabled = true,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title),
                if (help != null)
                  Text(help,
                      style: Theme.of(context).textTheme.bodySmall),
              ],
            ),
          ),
          DropdownButton<T>(
            value: options.contains(value) ? value : options.first,
            items: [
              for (final o in options)
                DropdownMenuItem(value: o, child: Text(label(o))),
            ],
            onChanged: enabled ? (v) => v != null ? onChanged(v) : null : null,
          ),
        ],
      ),
    );
  }
}
