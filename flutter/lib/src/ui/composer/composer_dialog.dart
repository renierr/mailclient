import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';
import 'composer_widgets.dart';
import 'markdown.dart';

/// How the composer was opened — what to prefill and what Send replaces.
enum ComposeMode { blank, reply, replyAll, forward, draft }

/// The starting point for a composer. Built by the static `show*` helpers
/// from a message or a stored draft, so the dialog itself only edits text.
class ComposerInitial {
  const ComposerInitial({
    required this.mode,
    this.fromAddr = '',
    this.to = '',
    this.cc = '',
    this.bcc = '',
    this.replyTo = '',
    this.subject = '',
    this.body = '',
    this.draftUid = -1,
    this.showCc = false,
    this.showBcc = false,
    this.serverAttachments = const [],
    this.replyNotice = '',
  });

  final ComposeMode mode;

  /// The stored From for a reopened draft. Blank/reply/forward leave it empty
  /// and the account address is used instead.
  final String fromAddr;
  final String to;
  final String cc;
  final String bcc;
  final String replyTo;
  final String subject;
  final String body;
  final int draftUid;
  final bool showCc;
  final bool showBcc;

  /// Attachments the server copy holds. They are metadata, not paths: saving
  /// replaces the server copy, so they are called out rather than silently
  /// dropped.
  final List<AttachmentInfo> serverAttachments;

  /// Shown when answering mail whose replies go somewhere unexpected.
  final String replyNotice;
}

/// Compose, reply, forward and draft editing.
///
/// Markdown plain-text editing, rendered to HTML on send: the toolbar wraps
/// the selection in `**bold**` / `*italic*` / `> quote` syntax and
/// [MarkdownMail] converts it to `body_html`, so marked-up text arrives
/// formatted (auto send format goes multipart). Unformatted text sends
/// exactly as before — plain, with no HTML twin. Quote blocks use `> `
/// citations, which survive every format.
class ComposerDialog extends StatefulWidget {
  const ComposerDialog({
    super.key,
    required this.initial,
    this.fullscreen = false,
  });

  final ComposerInitial initial;

  /// Fullscreen page instead of a floating dialog — used on phones, where a
  /// dialog plus the on-screen keyboard leaves no usable room.
  final bool fullscreen;

  /// Single routing point for every entry: fullscreen page on phones (a
  /// Scaffold resizes for the keyboard natively), dialog on wide screens.
  /// `barrierDismissible: false`: a tap outside must never drop a composition;
  /// closing goes through the dirty guard instead.
  static Future<void> _open(
    BuildContext context,
    ComposerInitial initial,
  ) async {
    await MailDialog.showForm(
      context,
      barrierDismissible: false,
      dialog: (_) => ComposerDialog(initial: initial),
      page: (_) => ComposerDialog(initial: initial, fullscreen: true),
    );
  }

  /// Blank message with the signature applied.
  static Future<void> showBlank(BuildContext context) async {
    final state = context.read<MailState>();
    await _open(
      context,
      ComposerInitial(mode: ComposeMode.blank, body: _signatureBlock(state)),
    );
  }

  /// Reply (or reply-all) quoting the open message as `> ` citations.
  static Future<void> showReply(
    BuildContext context,
    MessageBody message, {
    bool replyAll = false,
  }) async {
    final state = context.read<MailState>();
    final settings = state.settings;
    final answerTo = message.replyTo.isNotEmpty
        ? message.replyTo
        : message.from;
    final quote = _quote(message, settings.replyBelowQuote);
    final notice =
        message.replyTo.isNotEmpty &&
            !_sameAddress(message.replyTo, message.from)
        ? 'Replies to this mail go to ${message.replyTo} — not to the sender (${message.from}).'
        : '';
    await _open(
      context,
      ComposerInitial(
        mode: replyAll ? ComposeMode.replyAll : ComposeMode.reply,
        to: answerTo,
        cc: replyAll ? message.cc : '',
        subject: _subjectPrefix(message.subject, 'Re:'),
        body: quote + _signatureBlock(state),
        showCc: replyAll && message.cc.isNotEmpty,
        replyNotice: notice,
      ),
    );
  }

  /// Forward with a `— Forwarded message —` header and the quoted body.
  static Future<void> showForward(
    BuildContext context,
    MessageBody message,
  ) async {
    final state = context.read<MailState>();
    final header =
        '— Forwarded message —\n'
        'From: ${message.from}\n'
        'Date: ${message.date}\n'
        'Subject: ${message.subject}\n\n';
    await _open(
      context,
      ComposerInitial(
        mode: ComposeMode.forward,
        subject: _subjectPrefix(message.subject, 'Fwd:'),
        body: header + _quoteBody(message) + _signatureBlock(state),
      ),
    );
  }

  /// Continue a stored draft. Attachments come back as metadata; saving
  /// replaces the server copy, which the dialog calls out.
  static Future<void> showDraft(
    BuildContext context,
    int accountId,
    int uid,
  ) async {
    final state = context.read<MailState>();
    try {
      final form = await MailCore.instance.draftForm(accountId, uid);
      final attachments = ((form['attachments'] as List?) ?? const [])
          .whereType<Map<String, dynamic>>()
          .map(AttachmentInfo.fromJson)
          .toList(growable: false);
      if (!context.mounted) return;
      await _open(
        context,
        ComposerInitial(
          mode: ComposeMode.draft,
          fromAddr: '${form['from'] ?? ''}',
          to: '${form['to'] ?? ''}',
          cc: '${form['cc'] ?? ''}',
          bcc: '${form['bcc'] ?? ''}',
          replyTo: '${form['reply_to'] ?? ''}',
          subject: '${form['subject'] ?? ''}',
          body: _draftText(form),
          draftUid: (form['draft_uid'] as num?)?.toInt() ?? uid,
          showCc: '${form['cc'] ?? ''}'.isNotEmpty,
          showBcc: '${form['bcc'] ?? ''}'.isNotEmpty,
          serverAttachments: attachments
              .where((a) => !a.isInline)
              .toList(growable: false),
        ),
      );
    } catch (e) {
      state.showStatus('Draft is no longer available', isError: true);
    }
  }

  static String _subjectPrefix(String subject, String prefix) =>
      subject.toLowerCase().startsWith(prefix.toLowerCase())
      ? subject
      : '$prefix $subject';

  static String _quote(MessageBody message, bool below) {
    final cited =
        'On ${message.date}, ${message.from} wrote:\n'
        '${_quoteBody(message)}\n';
    return below ? '\n\n$cited' : '$cited\n';
  }

  static String _quoteBody(MessageBody message) {
    final text = message.bodyText.isNotEmpty
        ? message.bodyText
        : _stripTags(message.bodyHtml);
    return text.split('\n').map((l) => '> $l').join('\n');
  }

  /// Last resort for a quote when the core stored no plain twin: drop the
  /// tags, keep the words. The reader never renders this — it only quotes.
  static String _stripTags(String html) => html
      .replaceAll(RegExp(r'<[^>]*>'), ' ')
      .replaceAll(RegExp(r'\s+'), ' ')
      .trim();

  static String _signatureBlock(MailState state) {
    final s = state.settings;
    if (!s.signatureEnabled || s.signatureText.isEmpty) return '';
    return '\n\n-- \n${s.signatureText}';
  }

  static String _draftText(Map<String, dynamic> form) {
    final html = '${form['body_html'] ?? ''}';
    final text = '${form['body'] ?? ''}';
    return text.isNotEmpty ? text : html;
  }

  static bool _sameAddress(String a, String b) {
    String bare(String s) {
      final m = RegExp(r'<([^>]+)>').firstMatch(s);
      return (m?.group(1) ?? s).trim().toLowerCase();
    }

    return bare(a) == bare(b);
  }

  @override
  State<ComposerDialog> createState() => _ComposerDialogState();
}

class _ComposerDialogState extends State<ComposerDialog> {
  late final TextEditingController _fromLocal;
  late final TextEditingController _to;
  late final TextEditingController _cc;
  late final TextEditingController _bcc;
  late final TextEditingController _senderName;
  late final TextEditingController _replyToCtrl;
  late final TextEditingController _subject;
  late final TextEditingController _body;

  /// The account's domain, locked like in the Qt composer: sending as another
  /// domain breaks SPF and domain-aligned DKIM/DMARC. Only the local part
  /// edits.
  String _domain = '';

  /// Files picked this session: paths the core reads at send time, so no
  /// bytes cross into Dart state.
  final List<PickedFile> _picked = [];
  bool _showCc = false;
  bool _showBcc = false;
  bool _showReplyTo = false;
  bool _dirty = false;
  bool _sending = false;
  bool _savingDraft = false;
  bool get _working => _sending || _savingDraft;
  bool get _hasRecipients =>
      _to.text.trim().isNotEmpty ||
      _cc.text.trim().isNotEmpty ||
      _bcc.text.trim().isNotEmpty;
  String? _error;

  static String _localPartOf(String address) {
    final at = address.indexOf('@');
    return at < 0 ? address : address.substring(0, at);
  }

  static String _domainOf(String address) {
    final at = address.indexOf('@');
    return at < 0 ? '' : address.substring(at);
  }

  @override
  void initState() {
    super.initState();
    final i = widget.initial;
    // Controllers need the account address, which lives behind a context
    // lookup — defer to didChangeDependencies once.
    _fromLocal = TextEditingController()..addListener(_edited);
    _to = TextEditingController(text: i.to)..addListener(_edited);
    _cc = TextEditingController(text: i.cc)..addListener(_edited);
    _bcc = TextEditingController(text: i.bcc)..addListener(_edited);
    _senderName = TextEditingController()..addListener(_edited);
    _replyToCtrl = TextEditingController(text: i.replyTo)..addListener(_edited);
    _subject = TextEditingController(text: i.subject)..addListener(_edited);
    _body = TextEditingController(text: i.body)..addListener(_edited);
    _showCc = i.showCc;
    _showBcc = i.showBcc;
    _showReplyTo = i.replyTo.isNotEmpty;
  }

  bool _prefilled = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_prefilled) return;
    _prefilled = true;
    final state = context.read<MailState>();
    final accountEmail = state.account?.email ?? '';
    _domain = _domainOf(accountEmail);
    // Prefill must not mark the composer dirty: listeners are attached in
    // initState, so detach, fill, re-attach, then explicitly mark clean.
    // Otherwise every fresh composer instantly prompts "Unsent changes".
    for (final c in [_fromLocal, _senderName]) {
      c.removeListener(_edited);
    }
    // A reopened draft keeps its own local part; anything new starts from
    // the account address.
    _fromLocal.text = widget.initial.fromAddr.isNotEmpty
        ? _localPartOf(widget.initial.fromAddr)
        : _localPartOf(accountEmail);
    _senderName.text = state.account?.fromName ?? '';
    for (final c in [_fromLocal, _senderName]) {
      c.addListener(_edited);
    }
    _dirty = false;
  }

  void _edited() {
    if (!_dirty) setState(() => _dirty = true);
  }

  @override
  void dispose() {
    for (final c in [
      _fromLocal,
      _to,
      _cc,
      _bcc,
      _senderName,
      _replyToCtrl,
      _subject,
      _body,
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = context.read<MailState>();
    if (widget.fullscreen) return _page(state);
    final narrow = MailDialog.isNarrow(context);
    // Near-fullscreen on phones so the keyboard leaves a usable body field;
    // a floating 640px box would be covered by it.
    final maxW = MailDialog.maxWidth(context, 640);
    final maxH = MailDialog.maxHeight(context, 720);
    return PopScope(
      canPop: !_working && !_dirty,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop && !_working) {
          _maybeClose();
        }
      },
      child: Dialog(
        insetPadding: MailDialog.insets(context),
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: maxW, maxHeight: maxH),
          child: Padding(
            padding: EdgeInsets.all(narrow ? 12 : 20),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        _title,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                    ),
                    Text(
                      'Send as ${state.settings.sendFormat}',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
                if (widget.initial.replyNotice.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  ComposerNotice(
                    text: widget.initial.replyNotice,
                    danger: true,
                  ),
                ],
                const SizedBox(height: 12),
                Expanded(
                  child: AbsorbPointer(
                    absorbing: _working,
                    // No viewInsets padding here: the MailDialog.keyboardSafe
                    // wrapper already pads for the keyboard once.
                    child: SingleChildScrollView(child: _fieldsColumn()),
                  ),
                ),
                const SizedBox(height: 12),
                Wrap(
                  alignment: WrapAlignment.end,
                  spacing: 8,
                  children: _dialogActions(),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  String get _title => switch (widget.initial.mode) {
    ComposeMode.blank => 'New message',
    ComposeMode.reply => 'Reply',
    ComposeMode.replyAll => 'Reply all',
    ComposeMode.forward => 'Forward',
    ComposeMode.draft => 'Edit draft',
  };

  /// Fullscreen composer page for phones (see [ComposerDialog.fullscreen]):
  /// the Scaffold shrinks for the keyboard natively, so every field stays
  /// reachable while typing.
  Widget _page(MailState state) {
    return PopScope(
      canPop: !_working && !_dirty,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop && !_working) {
          _maybeClose();
        }
      },
      child: MailFormPage(
        title: _title,
        actions: [
          OutlinedButton(
            onPressed: _working ? null : _saveDraft,
            child: _savingDraft
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Text('Save'),
          ),
          FilledButton(
            onPressed: _working ? null : _send,
            child: _sending
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Text('Send'),
          ),
        ],
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Send as ${state.settings.sendFormat}',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            if (widget.initial.replyNotice.isNotEmpty) ...[
              const SizedBox(height: 8),
              ComposerNotice(text: widget.initial.replyNotice, danger: true),
            ],
            const SizedBox(height: 8),
            AbsorbPointer(absorbing: _working, child: _fieldsColumn()),
          ],
        ),
        bottomBar: Container(
          decoration: BoxDecoration(
            border: Border(
              top: BorderSide(color: Theme.of(context).dividerColor),
            ),
          ),
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
          child: Wrap(
            spacing: 8,
            children: [
              if (widget.initial.draftUid >= 0)
                TextButton.icon(
                  icon: const Icon(Icons.delete_outline),
                  label: const Text('Delete draft'),
                  onPressed: _working ? null : _deleteDraft,
                ),
              TextButton(
                onPressed: _working ? null : () => _maybeClose(),
                child: const Text('Discard'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// All composer fields, shared by the dialog and the fullscreen page.
  Widget _fieldsColumn() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _senderFields(),
        _addressFields(),
        _subjectField(),
        const SizedBox(height: 8),
        FormatToolbar(
          onBold: () => _wrapBody('**', '**'),
          onItalic: () => _wrapBody('*', '*'),
          onQuote: _quoteBody,
          onBullet: _bulletBody,
        ),
        _messageField(),
        _extrasSection(),
      ],
    );
  }

  /// Sender identity, shared by dialog and page: name beside the locked-domain
  /// From on wide screens, stacked full-width where a Row would squeeze both
  /// fields unreadably thin (narrow window, large text).
  Widget _senderFields() {
    final name = TextField(
      controller: _senderName,
      textInputAction: TextInputAction.next,
      decoration: InputDecoration(
        labelText: 'Sender name',
        hintText: context.read<MailState>().account?.displayName ?? '',
      ),
    );
    final from = TextField(
      controller: _fromLocal,
      keyboardType: TextInputType.emailAddress,
      textInputAction: TextInputAction.next,
      decoration: InputDecoration(
        labelText: 'From',
        suffixText: _domain,
        helperText: _domain.isEmpty ? null : 'Domain is fixed to this account',
      ),
    );
    if (MailDialog.isNarrow(context)) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [name, const SizedBox(height: 4), from],
      );
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        Expanded(flex: 2, child: name),
        const SizedBox(width: 8),
        Expanded(flex: 3, child: from),
      ],
    );
  }

  /// To/Cc/Bcc/Reply-To block, shared by dialog and page.
  Widget _addressFields() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        RecipientField(
          label: 'To',
          controller: _to,
          onToggleCc: () => setState(() => _showCc = !_showCc),
          onToggleBcc: () => setState(() => _showBcc = !_showBcc),
          onToggleReplyTo: () => setState(() => _showReplyTo = !_showReplyTo),
        ),
        if (_showCc) RecipientField(label: 'Cc', controller: _cc),
        if (_showBcc) RecipientField(label: 'Bcc', controller: _bcc),
        if (_showReplyTo)
          TextField(
            controller: _replyToCtrl,
            keyboardType: TextInputType.emailAddress,
            textInputAction: TextInputAction.next,
            decoration: const InputDecoration(
              labelText: 'Reply-To',
              helperText: 'Replies to this message go here instead of From',
            ),
          ),
      ],
    );
  }

  /// Subject line, shared by dialog and page.
  Widget _subjectField() {
    return TextField(
      controller: _subject,
      textInputAction: TextInputAction.next,
      decoration: const InputDecoration(labelText: 'Subject'),
    );
  }

  /// Message body, shared by dialog and page: short label (a long outlined
  /// label clips in the border gap at large text scales), Markdown hint moved
  /// to helper text. The syntax is rendered to HTML on send (see
  /// [MarkdownMail.toHtml]), so marked-up text arrives formatted.
  Widget _messageField() {
    return TextField(
      controller: _body,
      maxLines: null,
      minLines: 8,
      keyboardType: TextInputType.multiline,
      textInputAction: TextInputAction.newline,
      decoration: const InputDecoration(
        labelText: 'Message',
        helperText: 'Markdown: **bold**, *italic*, > quote — sent formatted',
        alignLabelWithHint: true,
        border: OutlineInputBorder(),
      ),
    );
  }

  /// Server-attachment notice, the picker tray and the error line, shared by
  /// dialog and page.
  Widget _extrasSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (widget.initial.serverAttachments.isNotEmpty) ...[
          const SizedBox(height: 8),
          ComposerNotice(
            text:
                '${widget.initial.serverAttachments.length} file(s) live on the server copy of this draft. Saving replaces it — re-attach them afterwards.',
          ),
          Wrap(
            spacing: 8,
            children: [
              for (final a in widget.initial.serverAttachments)
                Chip(
                  avatar: const Icon(Icons.attach_file, size: 16),
                  label: Text(a.filename),
                ),
            ],
          ),
        ],
        const SizedBox(height: 4),
        AttachmentPicker(
          picked: _picked,
          onChanged: () => setState(() => _dirty = true),
        ),
        if (_error != null) ...[
          const SizedBox(height: 4),
          Text(
            _error!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ],
    );
  }

  /// Dialog action row buttons, extracted so the dialog body stays readable.
  /// The fullscreen page spreads the same actions across AppBar + bottomBar.
  List<Widget> _dialogActions() {
    return [
      if (widget.initial.draftUid >= 0)
        TextButton.icon(
          icon: const Icon(Icons.delete_outline),
          label: const Text('Delete draft'),
          onPressed: _working ? null : _deleteDraft,
        ),
      TextButton(
        onPressed: _working ? null : () => _maybeClose(),
        child: const Text('Discard'),
      ),
      OutlinedButton(
        onPressed: _working ? null : _saveDraft,
        child: _savingDraft
            ? const SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : const Text('Save draft'),
      ),
      FilledButton(
        onPressed: _working ? null : _send,
        child: _sending
            ? const SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : const Text('Send'),
      ),
    ];
  }

  /// The address as the core will see it: edited local part, locked domain —
  /// or the whole account address when the field is blank.
  String _effectiveFrom(String accountEmail) {
    final local = _fromLocal.text.trim();
    if (local.isEmpty) return accountEmail;
    if (local.contains('@')) return local;
    return '$local$_domain';
  }

  Map<String, dynamic> _form() {
    final state = context.read<MailState>();
    final bodyText = _body.text;
    // Markdown renders to the HTML twin only when the text carries real
    // formatting; otherwise body_html stays empty and the mail goes out
    // exactly as before — plain, with no HTML part. The core sanitizes the
    // HTML again and picks multipart under `auto`.
    final bodyHtml = MarkdownMail.hasFormatting(bodyText)
        ? MarkdownMail.toHtml(bodyText)
        : '';
    return {
      'to': _to.text,
      'cc': _cc.text,
      'bcc': _bcc.text,
      'from': _effectiveFrom(state.account?.email ?? ''),
      'from_name': _senderName.text,
      'reply_to': _showReplyTo ? _replyToCtrl.text : '',
      'subject': _subject.text,
      'body': bodyText,
      'body_html': bodyHtml,
      'attachments': [for (final p in _picked) p.path],
      'draft_uid': widget.initial.draftUid,
    };
  }

  Future<void> _send() async {
    if (_working) return;
    if (!_hasRecipients) {
      setState(() => _error = 'Add at least one recipient (To, Cc or Bcc)');
      return;
    }
    final state = context.read<MailState>();
    setState(() {
      _sending = true;
      _error = null;
    });
    try {
      // Validation and queueing happen inline in the core: a mistake comes
      // back here with the composer still open and the text intact. Only the
      // SMTP submit runs in the background, reported on the status line.
      await MailCore.instance.sendMail(
        state.accountId,
        state.folderId,
        _form(),
      );
      if (!mounted) return;
      Navigator.of(context).pop();
      state.showStatus('Sending…');
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _sending = false;
        _error = _message(e);
      });
    }
  }

  Future<void> _saveDraft() async {
    if (_working) return;
    final state = context.read<MailState>();
    setState(() {
      _savingDraft = true;
      _error = null;
    });
    try {
      await MailCore.instance.saveDraft(state.accountId, _form());
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _savingDraft = false;
        _error = _message(e);
      });
    }
  }

  Future<void> _deleteDraft() async {
    if (_working) return;
    final state = context.read<MailState>();
    final confirmed =
        await MailDialog.show<bool>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Delete draft?'),
            content: const Text(
              'The server copy is destroyed permanently. This cannot be undone.',
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                style: MailDialog.dangerStyle(context),
                onPressed: () => Navigator.of(context).pop(true),
                child: const Text('Delete draft'),
              ),
            ],
          ),
        ) ??
        false;
    if (!confirmed || !mounted) return;
    try {
      await MailCore.instance.deleteDraft(
        state.accountId,
        widget.initial.draftUid,
      );
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = _message(e));
    }
  }

  /// Discard asks when there is unsent work — but never touches the server
  /// copy. Deleting that is the explicit button next to it.
  Future<void> _maybeClose() async {
    if (!_dirty) {
      Navigator.of(context).pop();
      return;
    }
    final choice =
        await MailDialog.show<DiscardChoice>(
          context,
          builder: (context) => AlertDialog(
            title: const Text('Unsent changes'),
            content: const Text(
              'Discard this message, or keep it as a draft first?',
            ),
            actions: [
              TextButton(
                onPressed: () =>
                    Navigator.of(context).pop(DiscardChoice.cancel),
                child: const Text('Cancel'),
              ),
              TextButton(
                onPressed: () =>
                    Navigator.of(context).pop(DiscardChoice.discard),
                child: const Text('Discard'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(DiscardChoice.save),
                child: const Text('Save draft'),
              ),
            ],
          ),
        ) ??
        DiscardChoice.cancel;
    if (!mounted) return;
    switch (choice) {
      case DiscardChoice.cancel:
        break;
      case DiscardChoice.discard:
        Navigator.of(context).pop();
      case DiscardChoice.save:
        await _saveDraft();
    }
  }

  static String _message(Object e) =>
      e is Exception ? e.toString().replaceFirst('Exception: ', '') : '$e';

  /// Wrap the body selection (or insert markers) with [prefix]/[suffix].
  /// Plain-text Markdown toolbar: the Qt WYSIWYG has no Flutter equivalent in
  /// scope, but bold/italic/quote must still be one tap, not memorised syntax.
  void _wrapBody(String prefix, String suffix) {
    final text = _body.text;
    final sel = _body.selection;
    final start = sel.start >= 0 ? sel.start : text.length;
    final end = sel.end >= 0 ? sel.end : text.length;
    final before = text.substring(0, start);
    final middle = text.substring(start, end);
    final after = text.substring(end);
    final insert = middle.isEmpty ? 'text' : middle;
    final next = '$before$prefix$insert$suffix$after';
    _body.value = TextEditingValue(
      text: next,
      selection: TextSelection.collapsed(
        offset: (before + prefix + insert + suffix).length,
      ),
    );
  }

  void _quoteBody() {
    final text = _body.text;
    final sel = _body.selection;
    if (sel.start < 0) {
      _body.text = text.split('\n').map((l) => '> $l').join('\n');
      return;
    }
    final before = text.substring(0, sel.start);
    final middle = text.substring(
      sel.start,
      sel.end >= 0 ? sel.end : sel.start,
    );
    final after = text.substring(sel.end >= 0 ? sel.end : sel.start);
    final quoted = middle.split('\n').map((l) => '> $l').join('\n');
    _body.text = '$before$quoted$after';
  }

  void _bulletBody() {
    final text = _body.text;
    final offset = _body.selection.start >= 0
        ? _body.selection.start
        : text.length;
    final lineStart = text.lastIndexOf('\n', offset <= 0 ? 0 : offset - 1) + 1;
    _body.text =
        '${text.substring(0, lineStart)}- ${text.substring(lineStart)}';
  }
}
