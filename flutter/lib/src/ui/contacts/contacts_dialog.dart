import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';

/// The auto-collected contact list: search, rename via alias, remove.
///
/// Contacts grow out of transferred mail, so the explanatory line stays —
/// otherwise an address book that fills itself looks like a bug.
class ContactsDialog extends StatefulWidget {
  const ContactsDialog({super.key});

  static Future<void> show(BuildContext context) async {
    await showDialog(
      context: context,
      builder: (_) => const ContactsDialog(),
    );
  }

  @override
  State<ContactsDialog> createState() => _ContactsDialogState();
}

class _ContactsDialogState extends State<ContactsDialog> {
  final _search = TextEditingController();
  final _alias = TextEditingController();
  List<Contact> _contacts = const [];
  String? _editing;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    _search.dispose();
    _alias.dispose();
    super.dispose();
  }

  String get _emptyText => _search.text.isEmpty
      ? 'No contacts yet'
      : 'No matching contacts found';

  TextStyle get _emptyStyle =>
      TextStyle(color: Theme.of(context).colorScheme.outline);

  Future<void> _reload() async {
    setState(() => _loading = true);
    try {
      final list =
          await MailCore.instance.contacts(prefix: _search.text.trim());
      if (!mounted) return;
      setState(() {
        _contacts = list;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _loading = false);
      context.read<MailState>().showStatus('$e', isError: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      insetPadding:
          const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Contacts',
                  style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 4),
              Text(
                'Auto-collected from transferred mail. Set an alias to rename someone just for you.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _search,
                onChanged: (_) => _reload(),
                decoration: InputDecoration(
                  labelText: 'Search by alias, name or address',
                  prefixIcon: const Icon(Icons.search),
                  suffixIcon: _search.text.isEmpty
                      ? null
                      : IconButton(
                          icon: const Icon(Icons.clear),
                          onPressed: () {
                            _search.clear();
                            _reload();
                          },
                        ),
                ),
              ),
              const SizedBox(height: 8),
              Expanded(
                child: _loading
                    ? const Center(child: CircularProgressIndicator())
                    : _contacts.isEmpty
                        ? Center(child: Text(_emptyText, style: _emptyStyle))
                        : ListView.separated(
                            itemCount: _contacts.length,
                            separatorBuilder: (_, _) =>
                                const Divider(height: 1),
                            itemBuilder: (context, i) =>
                                _row(_contacts[i]),
                          ),
              ),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: const Text('Close'),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _row(Contact c) {
    if (_editing == c.address) return _aliasRow(c);
    final name = c.alias.isNotEmpty
        ? c.alias
        : (c.name.isNotEmpty ? c.name : '(no alias)');
    return ListTile(
      title: Text(name,
          style: c.alias.isEmpty && c.name.isEmpty
              ? const TextStyle(fontStyle: FontStyle.italic)
              : null),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(c.address),
          if (c.alias.isNotEmpty && c.name.isNotEmpty) Text('Was: ${c.name}'),
        ],
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Chip(
            label: Text('seen ${c.timesSeen}'),
            visualDensity: VisualDensity.compact,
          ),
          IconButton(
            tooltip: 'Edit alias',
            icon: const Icon(Icons.edit_outlined, size: 18),
            onPressed: () => setState(() {
              _alias.text = c.alias;
              _editing = c.address;
            }),
          ),
          IconButton(
            tooltip: 'Remove',
            icon: const Icon(Icons.close, size: 18),
            onPressed: () => _remove(c),
          ),
        ],
      ),
      onTap: () => setState(() {
        _alias.text = c.alias;
        _editing = c.address;
      }),
    );
  }

  Widget _aliasRow(Contact c) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Alias for ${c.address}',
              style: Theme.of(context).textTheme.bodySmall),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _alias,
                  autofocus: true,
                  decoration: InputDecoration(
                    hintText: c.name.isNotEmpty ? c.name : 'Alias name',
                  ),
                  onSubmitted: (_) => _saveAlias(c),
                ),
              ),
              IconButton(
                tooltip: 'Save',
                icon: const Icon(Icons.check),
                onPressed: () => _saveAlias(c),
              ),
              IconButton(
                tooltip: 'Cancel',
                icon: const Icon(Icons.close),
                onPressed: () => setState(() => _editing = null),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _saveAlias(Contact c) async {
    try {
      await MailCore.instance.setContactAlias(c.address, _alias.text.trim());
      if (!mounted) return;
      setState(() => _editing = null);
      await _reload();
    } catch (e) {
      if (!mounted) return;
      context.read<MailState>().showStatus('$e', isError: true);
    }
  }

  Future<void> _remove(Contact c) async {
    final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('Remove contact?'),
            content: Text(
                'Forget ${c.address}? It reappears the next time mail arrives from it.'),
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
    if (!confirmed) return;
    try {
      await MailCore.instance.deleteContact(c.address);
      if (!mounted) return;
      await _reload();
    } catch (e) {
      if (!mounted) return;
      context.read<MailState>().showStatus('$e', isError: true);
    }
  }
}
