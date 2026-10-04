import 'package:flutter/material.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../dialogs/mail_dialog.dart';

/// The auto-collected contact list: search, rename via alias, remove.
///
/// Contacts grow out of transferred mail, so the explanatory line stays —
/// otherwise an address book that fills itself looks like a bug.
class ContactsDialog extends StatefulWidget {
  const ContactsDialog({super.key, this.fullscreen = false});

  /// Fullscreen page instead of a floating dialog — used on phones, where a
  /// dialog plus the on-screen keyboard leaves no usable room.
  final bool fullscreen;

  static Future<void> show(BuildContext context) async {
    await MailDialog.showForm(
      context,
      dialog: (_) => const ContactsDialog(),
      page: (_) => const ContactsDialog(fullscreen: true),
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

  /// Cleanup review state: suggested removals with a multi-select.
  bool _reviewing = false;
  List<CleanupCandidate> _candidates = const [];
  final Set<String> _selected = {};
  bool _reviewLoading = false;

  /// Last failed load/alias/remove, shown in the dialog itself: the status
  /// bar is hidden behind it (a whole page away on phones).
  String? _error;

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

  String get _emptyText =>
      _search.text.isEmpty ? 'No contacts yet' : 'No matching contacts found';

  TextStyle get _emptyStyle =>
      TextStyle(color: Theme.of(context).colorScheme.outline);

  Future<void> _reload() async {
    setState(() => _loading = true);
    try {
      final list = await MailCore.instance.contacts(
        prefix: _search.text.trim(),
      );
      if (!mounted) return;
      setState(() {
        _contacts = list;
        _loading = false;
        _error = null;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _error = '$e';
      });
    }
  }

  void _enterReview() {
    setState(() {
      _reviewing = true;
      _editing = null;
      _selected.clear();
    });
    _reloadCandidates();
  }

  void _exitReview() {
    setState(() {
      _reviewing = false;
      _selected.clear();
    });
    _reload();
  }

  Future<void> _reloadCandidates() async {
    setState(() => _reviewLoading = true);
    try {
      final list = await MailCore.instance.cleanupCandidates();
      if (!mounted) return;
      setState(() {
        _candidates = list;
        // Drop selections for rows that are no longer suggested.
        _selected.retainAll(list.map((c) => c.contact.address));
        _reviewLoading = false;
        _error = null;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _reviewLoading = false;
        _error = '$e';
      });
    }
  }

  String _reasonText(CleanupCandidate c) => c.reasonText;

  @override
  Widget build(BuildContext context) {
    if (widget.fullscreen) return _page();
    final narrow = MailDialog.isNarrow(context);
    return Dialog(
      insetPadding: MailDialog.insets(context, wideH: 16),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MailDialog.maxWidth(context, 560),
          maxHeight: MailDialog.maxHeight(context, 560),
        ),
        child: Padding(
          padding: EdgeInsets.all(narrow ? 12 : 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Contacts', style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 4),
              _explainer(),
              if (!_reviewing) ...[
                const SizedBox(height: 12),
                _searchField(),
              ],
              _reviewTools(),
              _errorLine(),
              const SizedBox(height: 8),
              Expanded(child: _listBody()),
              Align(
                alignment: Alignment.centerRight,
                child: Wrap(
                  spacing: 8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    ..._reviewActionButtons(),
                    TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: const Text('Close'),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Search field, shared by the dialog and the fullscreen page.
  Widget _searchField() {
    return TextField(
      controller: _search,
      onChanged: (_) => _reload(),
      textInputAction: TextInputAction.search,
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
    );
  }

  Widget _errorLine() {
    final error = _error;
    if (error == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Text(
        error,
        style: TextStyle(color: Theme.of(context).colorScheme.error),
      ),
    );
  }

  /// Loading or empty state, or null when there are rows to show.
  Widget? _placeholder() {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_contacts.isEmpty) {
      return Center(child: Text(_emptyText, style: _emptyStyle));
    }
    return null;
  }

  /// List for the dialog, whose header is short enough to stay fixed.
  Widget _listBody() {
    if (_reviewing) return _reviewListBody();
    if (_placeholder() case final Widget placeholder) return placeholder;
    return ListView.separated(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      itemCount: _contacts.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) => _row(_contacts[i]),
    );
  }

  /// The cleanup review list: suggested removals with a multi-select.
  Widget _reviewListBody() {
    if (_reviewLoading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_candidates.isEmpty) {
      return Center(
        child: Text(
          'No cleanup suggestions — your list looks tidy',
          style: _emptyStyle,
          textAlign: TextAlign.center,
        ),
      );
    }
    return ListView.separated(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      itemCount: _candidates.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) => _reviewRow(_candidates[i]),
    );
  }

  /// "N suggestions" plus select-all/clear, shown above the review list.
  Widget _reviewTools() {
    if (!_reviewing || _reviewLoading || _candidates.isEmpty) {
      return const SizedBox.shrink();
    }
    final all = _selected.length == _candidates.length;
    return Row(
      children: [
        Expanded(
          child: Text(
            '${_candidates.length} suggestions',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
        TextButton(
          onPressed: () => setState(() {
            if (all) {
              _selected.clear();
            } else {
              _selected.addAll(_candidates.map((c) => c.contact.address));
            }
          }),
          child: Text(all ? 'Clear' : 'Select all'),
        ),
      ],
    );
  }

  /// Review toggle + bulk remove, shared by the dialog footer and the page
  /// header. The dialog appends its own Close button.
  List<Widget> _reviewActionButtons() => [
    if (!_reviewing)
      TextButton(
        onPressed: _enterReview,
        child: const Text('Review suggestions'),
      ),
    if (_reviewing)
      TextButton(onPressed: _exitReview, child: const Text('Back')),
    if (_reviewing)
      FilledButton(
        style: MailDialog.dangerStyle(context),
        onPressed: _selected.isEmpty ? null : _removeSelected,
        child: Text('Remove selected (${_selected.length})'),
      ),
  ];

  /// Explanation line, shared by the dialog and the page.
  Widget _explainer() {
    return Text(
      _reviewing
          ? 'These look like automated senders or addresses seen only once, long ago. Tick the ones to forget.'
          : 'Auto-collected from transferred mail. Set an alias to rename someone just for you.',
      style: Theme.of(context).textTheme.bodySmall,
    );
  }

  /// Fullscreen contacts page for phones (see [ContactsDialog.fullscreen]).
  /// Header and rows scroll as one lazy sliver list: a fixed header over an
  /// Expanded list overflows a landscape phone once the keyboard is up.
  Widget _page() {
    final placeholder = _reviewing ? null : _placeholder();
    final reviewingBusy = _reviewing && _reviewLoading;
    final reviewingEmpty =
        _reviewing && !_reviewLoading && _candidates.isEmpty;
    return Scaffold(
      appBar: AppBar(title: const Text('Contacts')),
      body: SafeArea(
        child: CustomScrollView(
          keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
          slivers: [
            SliverPadding(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
              sliver: SliverToBoxAdapter(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _explainer(),
                    if (!_reviewing) ...[
                      const SizedBox(height: 12),
                      _searchField(),
                    ],
                    _reviewTools(),
                    _errorLine(),
                    const SizedBox(height: 4),
                    Wrap(
                      spacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: _reviewActionButtons(),
                    ),
                  ],
                ),
              ),
            ),
            if (placeholder != null || reviewingBusy || reviewingEmpty)
              SliverFillRemaining(
                hasScrollBody: false,
                child:
                    placeholder ??
                    (reviewingBusy
                        ? const Center(child: CircularProgressIndicator())
                        : Center(
                            child: Text(
                              'No cleanup suggestions — your list looks tidy',
                              style: _emptyStyle,
                              textAlign: TextAlign.center,
                            ),
                          )),
              )
            else if (_reviewing)
              SliverList.separated(
                itemCount: _candidates.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, i) => _reviewRow(_candidates[i]),
              )
            else
              SliverList.separated(
                itemCount: _contacts.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, i) => _row(_contacts[i]),
              ),
          ],
        ),
      ),
    );
  }

  /// One suggested removal: checkbox plus who and why.
  Widget _reviewRow(CleanupCandidate c) {
    final contact = c.contact;
    final name = contact.alias.isNotEmpty
        ? contact.alias
        : (contact.name.isNotEmpty ? contact.name : '(no alias)');
    return CheckboxListTile(
      dense: true,
      controlAffinity: ListTileControlAffinity.leading,
      value: _selected.contains(contact.address),
      onChanged: (v) => setState(() {
        if (v == true) {
          _selected.add(contact.address);
        } else {
          _selected.remove(contact.address);
        }
      }),
      title: Text(
        name,
        overflow: TextOverflow.ellipsis,
        style: contact.alias.isEmpty && contact.name.isEmpty
            ? const TextStyle(fontStyle: FontStyle.italic)
            : null,
      ),
      subtitle: Text(
        [
          contact.address,
          _reasonText(c),
          if (contact.sentCount > 0) 'sent ${contact.sentCount}',
        ].join(' · '),
        overflow: TextOverflow.ellipsis,
        maxLines: 2,
      ),
    );
  }

  Widget _row(Contact c) {
    if (_editing == c.address) return _aliasRow(c);
    final name = c.alias.isNotEmpty
        ? c.alias
        : (c.name.isNotEmpty ? c.name : '(no alias)');
    return ListTile(
      dense: true,
      title: Text(
        name,
        overflow: TextOverflow.ellipsis,
        style: c.alias.isEmpty && c.name.isEmpty
            ? const TextStyle(fontStyle: FontStyle.italic)
            : null,
      ),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(c.address, overflow: TextOverflow.ellipsis),
          // The seen-count lived in the trailing row as a Chip next to two
          // 48px buttons and overflowed the trailing width on narrow dialogs
          // (RenderFlex +13px). It fits here with no width pressure at all.
          Text(
            [
              'seen ${c.timesSeen}',
              if (c.sentCount > 0) 'sent ${c.sentCount}',
              if (c.alias.isNotEmpty && c.name.isNotEmpty) 'Was: ${c.name}',
            ].join(' · '),
            style: Theme.of(context).textTheme.bodySmall,
            overflow: TextOverflow.ellipsis,
          ),
        ],
      ),
      // Compact 32px targets: two full-size IconButtons plus a Chip never fit
      // the trailing slot of a narrow dialog.
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: 'Edit alias',
            iconSize: 18,
            padding: const EdgeInsets.all(4),
            constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
            icon: const Icon(Icons.edit_outlined),
            onPressed: () => setState(() {
              _alias.text = c.alias;
              _editing = c.address;
            }),
          ),
          IconButton(
            tooltip: 'Remove',
            iconSize: 18,
            padding: const EdgeInsets.all(4),
            constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
            icon: const Icon(Icons.close),
            onPressed: () => _remove(c),
          ),
        ],
      ),
      // Qt needs double-click/Enter to edit; single tap must not hijack the
      // row into an editor on touch screens.
      onTap: null,
    );
  }

  Widget _aliasRow(Contact c) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Alias for ${c.address}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
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
    setState(() => _error = null);
    try {
      await MailCore.instance.setContactAlias(c.address, _alias.text.trim());
      if (!mounted) return;
      setState(() => _editing = null);
      await _reload();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    }
  }

  Future<void> _remove(Contact c) async {
    final confirmed =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Remove contact?'),
            content: Text(
              'Forget ${c.address}? It reappears the next time mail arrives from it.',
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
    if (!confirmed || !mounted) return;
    setState(() => _error = null);
    try {
      await MailCore.instance.deleteContact(c.address);
      if (!mounted) return;
      await _reload();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    }
  }

  Future<void> _removeSelected() async {
    final count = _selected.length;
    if (count == 0) return;
    final confirmed =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Remove contacts?'),
            content: Text(
              'Forget $count ${count == 1 ? 'contact' : 'contacts'}? '
              '${count == 1 ? 'It reappears' : 'They reappear'} the next time mail arrives from '
              '${count == 1 ? 'it' : 'them'}.',
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
    if (!confirmed || !mounted) return;
    setState(() => _error = null);
    try {
      await MailCore.instance.deleteContacts(_selected.toList());
      if (!mounted) return;
      setState(_selected.clear);
      await _reloadCandidates();
      await _reload();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    }
  }
}
