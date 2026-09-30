import 'dart:io';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../../sync/background_alarm.dart';
import '../../sync/background_power.dart';
import '../../sync/background_sync.dart';
import '../dialogs/mail_dialog.dart';
import 'background_check_status.dart';

/// All preferences, Roundcube-style: sections on the left, the form on the
/// right. Everything edits a local copy; Save writes it through, Cancel
/// reverts by writing nothing at all.
///
/// Always a full page, never a dialog: this frontend is built for phones.
/// (The Qt desktop client keeps its resizable settings dialog.)
class SettingsDialog extends StatefulWidget {
  const SettingsDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        fullscreenDialog: true,
        builder: (_) => const SettingsDialog(),
      ),
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
    // Qt auto-loads capabilities when About opens; do the same so the section
    // is never a stale "press Refresh" on first visit.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final state = context.read<MailState>();
      _capsAccountId ??= state.accountId;
      if (_capsAccountId != null &&
          _capsAccountId! >= 0 &&
          state.capabilitiesFor(_capsAccountId!) == null) {
        state.refreshCapabilities(_capsAccountId!);
      }
    });
  }

  @override
  void dispose() {
    _signature.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings'),
        // The route's close button is Cancel: it pops without writing.
        actions: [
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: FilledButton(
              onPressed: _saving ? null : _save,
              child: Text(_saving ? 'Saving…' : 'Save'),
            ),
          ),
        ],
      ),
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) {
            // The rail needs width beside the form and height for six
            // labelled destinations; otherwise a selector sits above it.
            final railMode =
                constraints.maxWidth >= 600 && constraints.maxHeight >= 480;
            final form = SingleChildScrollView(
              keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
              padding: const EdgeInsets.all(16),
              child: Align(
                alignment: Alignment.topLeft,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 720),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      // Save is in the AppBar, so the error goes to the top
                      // where it is seen, not below a long scroll.
                      if (_error != null) ...[
                        Text(_error!, style: TextStyle(color: scheme.error)),
                        const SizedBox(height: 8),
                      ],
                      if (!railMode) ...[
                        _sectionDropdown(),
                        const SizedBox(height: 8),
                      ],
                      _body(),
                    ],
                  ),
                ),
              ),
            );
            if (!railMode) return form;
            return Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                NavigationRail(
                  selectedIndex: _Section.values.indexOf(_section),
                  onDestinationSelected: (i) =>
                      setState(() => _section = _Section.values[i]),
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
                Expanded(child: form),
              ],
            );
          },
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
        (v) => setState(() => _draft = _draft.copyWith(readerFontSize: v)),
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
        (v) => setState(() => _draft = _draft.copyWith(sortDescending: v)),
      ),
      _choice<String>(
        'Density',
        _draft.density,
        const ['comfortable', 'compact'],
        (v) => v[0].toUpperCase() + v.substring(1),
        (v) => setState(() => _draft = _draft.copyWith(density: v)),
      ),
      _switch(
        'Confirm before moving mail to Trash',
        _draft.confirmDelete,
        (v) => setState(() => _draft = _draft.copyWith(confirmDelete: v)),
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
        (v) => setState(() => _draft = _draft.copyWith(markReadDelaySecs: v)),
        enabled: _draft.autoMarkRead,
      ),
      _switch(
        'Load remote images (not recommended)',
        _draft.loadRemoteImages,
        (v) => setState(() => _draft = _draft.copyWith(loadRemoteImages: v)),
      ),
      _choice<String>(
        'Clicking a link in a message',
        _draft.linkClickAction,
        const ['examine', 'browser'],
        (v) => switch (v) {
          'browser' => 'Open directly in browser',
          _ => 'Show safety dialog first (recommended)',
        },
        (v) => setState(() => _draft = _draft.copyWith(linkClickAction: v)),
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
      ),
      _switch(
        'Always include a plain-text version alongside HTML',
        _draft.includePlain,
        (v) => setState(() => _draft = _draft.copyWith(includePlain: v)),
      ),
      _choice<bool>(
        'Replies start',
        _draft.replyBelowQuote,
        const [false, true],
        (v) => v ? 'Below quote' : 'Above quote',
        (v) => setState(() => _draft = _draft.copyWith(replyBelowQuote: v)),
      ),
      _switch(
        'Use signature',
        _draft.signatureEnabled,
        (v) => setState(() => _draft = _draft.copyWith(signatureEnabled: v)),
      ),
      TextField(
        controller: _signature,
        maxLines: 3,
        onChanged: (v) => _draft = _draft.copyWith(signatureText: v),
        decoration: const InputDecoration(labelText: 'Signature'),
      ),
      _switch(
        'Request read receipt',
        _draft.requestMdn,
        (v) => setState(() => _draft = _draft.copyWith(requestMdn: v)),
      ),
    ],
  );

  Widget _sync() => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      _switch(
        'Save a copy of sent mail in Sent',
        _draft.sentCopy,
        (v) => setState(() => _draft = _draft.copyWith(sentCopy: v)),
      ),
      _switch(
        'Suggest recipients from sent mail',
        _draft.collectContacts,
        (v) => setState(() => _draft = _draft.copyWith(collectContacts: v)),
      ),
      _choice<int>(
        'Check for new mail',
        _draft.syncIntervalMinutes,
        const [0, 5, 10, 15, 30, 60],
        (v) => v == 0 ? 'Manually' : 'Every ${v}m',
        (v) => setState(() => _draft = _draft.copyWith(syncIntervalMinutes: v)),
        help: Platform.isAndroid
            ? 'Battery-saving checks run at most every 15 minutes. With '
                  'push, any interval but Manually turns it on.'
            : null,
      ),
      // The scheduler is an Android-only capability: only there Doze
      // defers the battery-saving worker until the phone is unlocked.
      if (Platform.isAndroid)
        _choice<String>(
          'Background check method',
          _draft.backgroundScheduler,
          const [schedulerWorkmanager, schedulerAlarm, schedulerPush],
          (v) => switch (v) {
            schedulerAlarm => 'On-time alarm',
            schedulerPush => 'Push (IMAP IDLE)',
            _ => 'Battery-saving (recommended)',
          },
          (v) =>
              setState(() => _draft = _draft.copyWith(backgroundScheduler: v)),
          help:
              'Push: the server announces new mail as it arrives. The '
              'on-time alarm checks at the interval, in standby too.',
        ),
      _switch(
        'Show notifications for new mail',
        _draft.notificationsEnabled,
        (v) =>
            setState(() => _draft = _draft.copyWith(notificationsEnabled: v)),
      ),
      const SizedBox(height: 8),
      OutlinedButton.icon(
        onPressed: _saving ? null : _sendTestNotification,
        icon: const Icon(Icons.notifications_outlined),
        label: const Text('Send test notification'),
      ),
      // A platform capability, not a layout choice: only Android has the
      // background worker this reports on.
      if (Platform.isAndroid) ...[
        const SizedBox(height: 16),
        const BackgroundCheckStatus(),
      ],
    ],
  );

  // A Builder, not the State's context: the page body is built inside a
  // LayoutBuilder callback, where `context.select` on the State's context
  // asserts. The Builder's own build is a real build.
  Widget _about() => Builder(builder: _aboutSection);

  Widget _aboutSection(BuildContext context) {
    final accounts = context.select<MailState, List<Account>>(
      (s) => s.accounts,
    );
    final accountId = context.select<MailState, int>((s) => s.accountId);
    final busy = context.select<MailState, bool>((s) => s.isBusy);
    final status = context.select<MailState, String>((s) => s.status);
    final statusIsError = context.select<MailState, bool>(
      (s) => s.statusIsError,
    );
    final info = MailCore.instance.info;
    _capsAccountId ??= accountId;
    final caps = _capsAccountId == null
        ? null
        : context.read<MailState>().capabilitiesFor(_capsAccountId!);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SelectableText('Mailclient ${info.version}'),
        SelectableText('License: ${info.license}'),
        SelectableText('Database: ${info.dbPath}'),
        const SizedBox(height: 16),
        Text(
          'Server capabilities',
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        Wrap(
          crossAxisAlignment: WrapCrossAlignment.center,
          spacing: 8,
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(
                minWidth: 160,
                maxWidth: MailDialog.maxWidth(context, 360),
              ),
              child: DropdownButton<int>(
                value: accounts.any((a) => a.id == _capsAccountId)
                    ? _capsAccountId
                    : null,
                hint: const Text('Add an account first'),
                isExpanded: true,
                items: [
                  for (final a in accounts)
                    DropdownMenuItem(
                      value: a.id,
                      child: Text(a.email, overflow: TextOverflow.ellipsis),
                    ),
                ],
                onChanged: (id) => setState(() => _capsAccountId = id),
              ),
            ),
            OutlinedButton(
              onPressed: _capsAccountId == null || _capsAccountId! < 0
                  ? null
                  : () => context.read<MailState>().refreshCapabilities(
                      _capsAccountId!,
                    ),
              child: const Text('Refresh'),
            ),
          ],
        ),
        const SizedBox(height: 8),
        if (caps == null)
          Row(
            children: [
              if (busy)
                const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              if (busy) const SizedBox(width: 8),
              Expanded(
                child: Text(
                  busy
                      ? 'Loading capabilities…'
                      : statusIsError && status.isNotEmpty
                      ? status
                      : 'No capabilities loaded yet — press Refresh.',
                ),
              ),
            ],
          )
        else ...[
          SelectableText('${caps['email'] ?? ''} · ${caps['imap_host'] ?? ''}'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final c in ((caps['capabilities'] as List?) ?? const []))
                Chip(
                  label: Text(
                    '$c',
                    style: const TextStyle(fontFamily: 'monospace'),
                  ),
                  visualDensity: VisualDensity.compact,
                ),
            ],
          ),
        ],
      ],
    );
  }

  Future<void> _sendTestNotification() async {
    if (!await requestNotificationPermission()) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Notifications are not allowed')),
      );
      return;
    }
    await showTestNotification();
    if (!mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(const SnackBar(content: Text('Test notification sent')));
  }

  Future<void> _save() async {
    final state = context.read<MailState>();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final before = state.settings;
      final now = _values(_draft);
      final old = _values(before);
      // Only what changed, in one all-or-nothing batch: a failure leaves
      // every setting as it was, and an untouched interval does not
      // reschedule the sync timers.
      final writes = {
        for (final e in now.entries)
          if (old[e.key] != e.value) e.key: e.value,
      };
      await state.setSettings(writes);
      final d = _draft;
      if (d.sortField != before.sortField ||
          d.sortDescending != before.sortDescending) {
        // Sort is its own call: the two keys only make sense together.
        await state.setSort(d.sortField, d.sortDescending);
      }
      // Background checks just got enabled: ask for the notification
      // permission now, not on some later cold start.
      // Then the battery exemption, without which Doze postpones the
      // worker by hours while the phone sleeps.
      if (d.syncIntervalMinutes > 0 && before.syncIntervalMinutes <= 0) {
        await requestNotificationPermission();
        final power = await backgroundPowerStatus();
        if (power != null && !power.unrestricted) {
          await requestUnrestrictedBackground();
        }
      }
      // The alarm scheduler and push's keep-alive need the exact-alarm
      // grant on Android 14+; without it they still fire, just not exact.
      if (d.backgroundScheduler != schedulerWorkmanager &&
          d.backgroundScheduler != before.backgroundScheduler &&
          d.syncIntervalMinutes > 0) {
        if (!await exactAlarmPermitted()) {
          await requestExactAlarm();
        }
      }
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

  /// Every stored setting of [d] in its raw string form.
  static Map<String, String> _values(AppSettings d) => {
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
    SettingKeys.linkClickAction: d.linkClickAction,
    SettingKeys.syncInterval: '${d.syncIntervalMinutes}',
    SettingKeys.backgroundScheduler: d.backgroundScheduler,
    SettingKeys.notificationsEnabled: _yn(d.notificationsEnabled),
    SettingKeys.signatureEnabled: _yn(d.signatureEnabled),
    SettingKeys.signatureText: d.signatureText,
    SettingKeys.replyBelowQuote: _yn(d.replyBelowQuote),
    SettingKeys.requestMdn: _yn(d.requestMdn),
    SettingKeys.uiScale: '${d.uiScale}',
  };

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

  /// Secondary hint under a setting: small and muted, so the labels carry
  /// the page and the hints stay out of the way.
  Widget _help(String text) => Text(
    text,
    style: Theme.of(context).textTheme.bodySmall
        ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant),
  );

  Widget _switch(
    String title,
    bool value,
    ValueChanged<bool> onChanged, {
    String? help,
  }) {
    return SwitchListTile(
      title: Text(title),
      subtitle: help == null ? null : _help(help),
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
    DropdownButton<T> control({required bool expanded}) => DropdownButton<T>(
      value: options.contains(value) ? value : options.first,
      isExpanded: expanded,
      items: [
        for (final o in options)
          DropdownMenuItem(
            value: o,
            child: Text(label(o), overflow: TextOverflow.ellipsis),
          ),
      ],
      onChanged: enabled ? (v) => v != null ? onChanged(v) : null : null,
    );
    final labels = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [Text(title), if (help != null) _help(help)],
    );
    // Label above the control on narrow/zoomed layouts: label-beside-control
    // rows squeeze the dropdown (or the label) to zero there. Wide screens
    // keep the compact side-by-side form.
    if (MailDialog.isNarrow(context)) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            labels,
            const SizedBox(height: 2),
            // Expanded: a long option label ellipsizes instead of
            // overflowing the dropdown at 360px / large text.
            control(expanded: true),
          ],
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Expanded(child: labels),
          const SizedBox(width: 8),
          // Flexible + isExpanded: the button caps at the remaining width and
          // ellipsizes instead of overflowing the row on narrow dialogs.
          Flexible(child: control(expanded: true)),
        ],
      ),
    );
  }

  /// Section selector for pages too narrow or short for the rail.
  Widget _sectionDropdown() {
    return DropdownButton<_Section>(
      value: _section,
      isExpanded: true,
      items: [
        for (final s in _Section.values)
          DropdownMenuItem(value: s, child: Text(_label(s))),
      ],
      onChanged: (s) => setState(() => _section = s ?? _section),
    );
  }
}
