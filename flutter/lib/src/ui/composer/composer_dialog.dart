import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';
import '../dialogs/mail_dialog.dart';
import 'composer_drop_target.dart';
import 'composer_editor.dart';
import 'composer_header_row.dart';
import 'composer_quote.dart';
import 'composer_toggle.dart';
import 'composer_widgets.dart';
import 'inline_images.dart';
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
    this.quoteHtml = '',
    this.quoteFirst = false,
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

  /// The quoted original (reply/forward), carried beside the text box and
  /// appended to `body_html` on send — see [ComposerQuote].
  final String quoteHtml;

  /// The quote goes above the user's text (bottom-posting).
  final bool quoteFirst;
}

/// Compose, reply, forward and draft editing.
///
/// Markdown plain-text editing, rendered to HTML on send: the toolbar wraps
/// the selection in `**bold**` / `*italic*` / `> quote` syntax and
/// [MarkdownMail] converts it to `body_html`, so marked-up text arrives
/// formatted (auto send format goes multipart). Unformatted text sends
/// exactly as before — plain, with no HTML twin. The quoted original of a
/// reply or forward is not in the text box: the core prepares it as HTML
/// and [ComposerQuote] carries it beside the editor until send.
class ComposerDialog extends StatefulWidget {
  const ComposerDialog({super.key, required this.initial});

  final ComposerInitial initial;

  /// Single routing point for every entry. Always a full page, on every
  /// width: this frontend is built for phones, where a Scaffold resizes for
  /// the keyboard natively and a floating dialog leaves no usable room. (The
  /// Qt desktop client keeps its resizable composer window.)
  static Future<void> _open(
    BuildContext context,
    ComposerInitial initial,
  ) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        fullscreenDialog: true,
        builder: (_) => ComposerDialog(initial: initial),
      ),
    );
  }

  /// Blank message with the signature applied.
  static Future<void> showBlank(BuildContext context) async {
    final draft = await _draft(() => MailCore.instance.blankDraft());
    if (!context.mounted) return;
    await _open(
      context,
      ComposerInitial(mode: ComposeMode.blank, body: _bodyFor(draft)),
    );
  }

  /// Reply or reply-all. Recipients, subject, quote and signature come
  /// prepared by the core (`mailcore::compose::answer`), as in Qt.
  static Future<void> showReply(
    BuildContext context,
    MessageBody message, {
    bool replyAll = false,
  }) => _showAnswer(
    context,
    message,
    replyAll ? ComposeMode.replyAll : ComposeMode.reply,
  );

  /// Forward with a `— Forwarded message —` header and the quoted body.
  static Future<void> showForward(BuildContext context, MessageBody message) =>
      _showAnswer(context, message, ComposeMode.forward);

  static Future<void> _showAnswer(
    BuildContext context,
    MessageBody message,
    ComposeMode mode,
  ) async {
    final state = context.read<MailState>();
    final wire = switch (mode) {
      ComposeMode.replyAll => 'reply_all',
      ComposeMode.forward => 'forward',
      _ => 'reply',
    };
    final draft = await _draft(
      () => MailCore.instance.answerDraft(state.folderId, message.uid, wire),
    );
    if (!context.mounted) return;
    if (draft == null) {
      state.showStatus('This message is no longer available', isError: true);
      return;
    }
    await _open(
      context,
      ComposerInitial(
        mode: mode,
        to: draft.to,
        cc: draft.cc,
        subject: draft.subject,
        body: _bodyFor(draft),
        showCc: draft.cc.isNotEmpty,
        replyNotice: draft.noticeAddr.isEmpty
            ? ''
            : 'Replies to this mail go to ${draft.noticeAddr} — not to the sender (${draft.noticeSender}).',
        quoteHtml: draft.quoteHtml,
        quoteFirst: draft.quoteFirst,
      ),
    );
  }

  static Future<AnswerDraft?> _draft(
    Future<AnswerDraft> Function() load,
  ) async {
    try {
      return await load();
    } catch (_) {
      return null;
    }
  }

  /// The text box starts with room to type, then the signature.
  static String _bodyFor(AnswerDraft? draft) {
    final sig = draft?.signatureText ?? '';
    return sig.isEmpty ? '' : '\n\n$sig';
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

  static String _draftText(Map<String, dynamic> form) {
    final html = '${form['body_html'] ?? ''}';
    final text = '${form['body'] ?? ''}';
    return text.isNotEmpty ? text : html;
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
  final InlineImages _images = InlineImages();
  bool _showCc = false;
  bool _showBcc = false;
  bool _showReplyTo = false;

  /// The send-format setting, as last built (see [ComposerEditor]).
  String _sendFormat = 'auto';
  bool _dirty = false;

  /// The quoted original still goes out (see [ComposerQuote]).
  bool _keepQuote = true;
  bool _sending = false;
  bool _savingDraft = false;
  bool get _working => _sending || _savingDraft;
  bool get _hasRecipients =>
      _to.text.trim().isNotEmpty ||
      _cc.text.trim().isNotEmpty ||
      _bcc.text.trim().isNotEmpty;
  String? _error;

  /// The account the composer was opened for, pinned at open: switching
  /// accounts behind an open composer must not send or save as the other one.
  late final int _accountId;
  late final int _folderId;
  late final Account? _account;

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
    final state = context.read<MailState>();
    _accountId = state.accountId;
    _folderId = state.folderId;
    _account = state.account;
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
    final accountEmail = _account?.email ?? '';
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
    _senderName.text = _account?.fromName ?? '';
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
    final sendFormat = context.select<MailState, String>(
      (s) => s.settings.sendFormat,
    );
    _sendFormat = sendFormat;
    return _page();
  }

  static String _formatLabel(String format) => switch (format) {
    'plain' => 'Plain text',
    'multipart' => 'Multipart',
    'html' => 'HTML',
    _ => 'Auto',
  };

  /// Cc/Bcc/Reply-To rows show while toggled on, and always while they hold
  /// text — hiding a filled field would send addresses the user cannot see.
  bool get _ccShown => _showCc || _cc.text.isNotEmpty;
  bool get _bccShown => _showBcc || _bcc.text.isNotEmpty;
  bool get _replyToShown => _showReplyTo || _replyToCtrl.text.isNotEmpty;

  String get _title => switch (widget.initial.mode) {
    ComposeMode.blank => 'New message',
    ComposeMode.reply => 'Reply',
    ComposeMode.replyAll => 'Reply all',
    ComposeMode.forward => 'Forward',
    ComposeMode.draft => 'Edit draft',
  };

  /// The composer page: the Scaffold shrinks for the keyboard natively, so
  /// every field stays reachable while typing.
  Widget _page() {
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
          Center(
            child: Padding(
              padding: const EdgeInsets.only(right: 16),
              child: Text(
                'Send as: ${_formatLabel(_sendFormat)}',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ),
          ),
        ],
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_error != null) _errorLine(),
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
          // Qt order, at the bottom where the thumb is and above the
          // keyboard: Discard, Save draft, Send. Wrap, not Row, so the
          // buttons stack instead of overflowing a narrow phone.
          child: Wrap(
            alignment: WrapAlignment.end,
            spacing: 8,
            runSpacing: 8,
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
            ],
          ),
        ),
      ),
    );
  }

  /// All composer fields.
  Widget _fieldsColumn() {
    return ComposerDropTarget(onFiles: _dropFiles, child: _fields());
  }

  Widget _fields() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _senderRow(),
        ..._addressRows(),
        ComposerHeaderRow(
          label: 'Subject',
          child: TextField(
            controller: _subject,
            textInputAction: TextInputAction.next,
            decoration: ComposerHeaderRow.field(),
          ),
        ),
        const SizedBox(height: 12),
        if (_quoteShown && widget.initial.quoteFirst) ...[
          _quoteCard(),
          const SizedBox(height: 8),
        ],
        ComposerEditor(
          controller: _body,
          images: () => _images.urls,
          sendFormat: _sendFormat,
          onBold: () => _wrapBody('**', '**'),
          onItalic: () => _wrapBody('*', '*'),
          onQuote: _quoteBody,
          onBullet: _bulletBody,
          onImage: _insertImages,
          onAttach: _pickFiles,
        ),
        if (_quoteShown && !widget.initial.quoteFirst) ...[
          const SizedBox(height: 8),
          _quoteCard(),
        ],
        ..._extras(),
      ],
    );
  }

  /// From: display name and the local part, with the account's domain fixed
  /// after it. Side by side on wide screens, stacked where a Row would
  /// squeeze both fields unreadably thin.
  Widget _senderRow() {
    final name = TextField(
      controller: _senderName,
      textInputAction: TextInputAction.next,
      decoration: ComposerHeaderRow.field(
        hint: _account?.displayName ?? 'Your name',
      ),
    );
    final address = TextField(
      controller: _fromLocal,
      keyboardType: TextInputType.emailAddress,
      textInputAction: TextInputAction.next,
      // Right-aligned so the local part sits neatly beside the fixed
      // domain suffix, like the Qt composer.
      textAlign: TextAlign.end,
      decoration: ComposerHeaderRow.field(
        hint: 'address',
        suffix: _domain.isEmpty
            ? null
            : Tooltip(
                message: "Fixed to this account's domain",
                child: Text(
                  _domain,
                  style: TextStyle(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ),
      ),
    );
    return ComposerHeaderRow(
      label: 'From',
      trailing: ComposerToggle(
        icon: Icons.reply,
        label: 'Reply-To',
        tooltip: 'Set Reply-To address',
        active: _replyToShown,
        onPressed: () => setState(() => _showReplyTo = !_replyToShown),
      ),
      child: MailDialog.isNarrow(context)
          ? Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [name, address],
            )
          : Row(
              children: [
                Expanded(flex: 2, child: name),
                const SizedBox(width: 12),
                Expanded(flex: 3, child: address),
              ],
            ),
    );
  }

  /// To, then whichever of Cc/Bcc/Reply-To are open. Cc and Bcc toggle
  /// from the To line, like the Qt composer.
  List<Widget> _addressRows() {
    return [
      ComposerHeaderRow(
        label: 'To',
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ComposerToggle(
              label: 'Cc',
              tooltip: 'Show Cc field',
              active: _ccShown,
              onPressed: () => setState(() => _showCc = !_ccShown),
            ),
            ComposerToggle(
              label: 'Bcc',
              tooltip: 'Show Bcc field',
              active: _bccShown,
              onPressed: () => setState(() => _showBcc = !_bccShown),
            ),
          ],
        ),
        child: RecipientField(controller: _to),
      ),
      if (_ccShown)
        ComposerHeaderRow(
          label: 'Cc',
          child: RecipientField(controller: _cc),
        ),
      if (_bccShown)
        ComposerHeaderRow(
          label: 'Bcc',
          child: RecipientField(
            controller: _bcc,
            hint: 'Hidden from the other recipients',
          ),
        ),
      if (_replyToShown)
        ComposerHeaderRow(
          label: 'Reply-To',
          child: TextField(
            controller: _replyToCtrl,
            keyboardType: TextInputType.emailAddress,
            textInputAction: TextInputAction.next,
            decoration: ComposerHeaderRow.field(
              hint: 'Replies go here instead of From',
            ),
          ),
        ),
    ];
  }

  /// Server-attachment notice and the picked files, below the editor.
  List<Widget> _extras() {
    return [
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
      AttachmentTray(
        picked: _picked,
        onChanged: () => setState(() => _dirty = true),
      ),
    ];
  }

  Future<void> _pickFiles() async {
    if (await AttachmentTray.pick(context.read<MailState>(), _picked) &&
        mounted) {
      setState(() => _dirty = true);
    }
  }

  /// Send/save failure, at the top of the page (whose actions are in the
  /// AppBar), so it is never scrolled out of sight below a long body.
  Widget _errorLine() => Padding(
    padding: const EdgeInsets.only(top: 8),
    child: Text(
      _error!,
      style: TextStyle(color: Theme.of(context).colorScheme.error),
    ),
  );

  /// The address as the core will see it: edited local part, locked domain —
  /// or the whole account address when the field is blank.
  String _effectiveFrom(String accountEmail) {
    final local = _fromLocal.text.trim();
    if (local.isEmpty) return accountEmail;
    if (local.contains('@')) return local;
    return '$local$_domain';
  }

  bool get _quoteShown => _keepQuote && widget.initial.quoteHtml.isNotEmpty;

  Widget _quoteCard() => ComposerQuote(
    html: widget.initial.quoteHtml,
    forward: widget.initial.mode == ComposeMode.forward,
    onRemove: () => setState(() {
      _keepQuote = false;
      _dirty = true;
    }),
  );

  Map<String, dynamic> _form() {
    final bodyText = _body.text;
    final quote = _quoteShown ? widget.initial.quoteHtml : '';
    // Markdown renders to the HTML twin only when the text carries real
    // formatting or a quote rides along; otherwise body_html stays empty and
    // the mail goes out plain, with no HTML part. With a quote the core
    // derives the plain part from the HTML (a plain original's `> ` quote
    // keeps an Auto send text/plain), sanitizes it again and picks the shape.
    final own = MarkdownMail.hasFormatting(bodyText) || quote.isNotEmpty
        ? MarkdownMail.toHtml(bodyText, images: _images.urls)
        : '';
    final bodyHtml = quote.isEmpty
        ? own
        : widget.initial.quoteFirst
        ? '$quote$own'
        : '$own$quote';
    return {
      'to': _to.text,
      'cc': _cc.text,
      'bcc': _bcc.text,
      'from': _effectiveFrom(_account?.email ?? ''),
      'from_name': _senderName.text,
      'reply_to': _replyToShown ? _replyToCtrl.text : '',
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
      await MailCore.instance.sendMail(_accountId, _folderId, _form());
      if (!mounted) return;
      Navigator.of(context).pop();
      state.showStatus('Sending…');
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _sending = false;
        _error = coreErrorText(e);
      });
    }
  }

  Future<void> _saveDraft() async {
    if (_working) return;
    setState(() {
      _savingDraft = true;
      _error = null;
    });
    try {
      await MailCore.instance.saveDraft(_accountId, _form());
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _savingDraft = false;
        _error = coreErrorText(e);
      });
    }
  }

  Future<void> _deleteDraft() async {
    if (_working) return;
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
      await MailCore.instance.deleteDraft(_accountId, widget.initial.draftUid);
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = coreErrorText(e));
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

  /// Pick images and put their tokens at the cursor (see [InlineImages]).
  Future<void> _insertImages() async {
    _insertTokens(await _images.pick(context.read<MailState>()));
  }

  /// Dropped files: other files attach at once; images ask whether they go
  /// inline or attach.
  Future<void> _dropFiles(List<({String path, String name})> files) async {
    if (_working) return;
    final core = MailCore.instance;
    final images = files.where((f) => core.isInlineImage(f.path)).toList();
    final others = files.where((f) => !core.isInlineImage(f.path)).toList();
    _attach(others);
    if (images.isEmpty) return;
    final placement = await askImagePlacement(context, images.length);
    if (!mounted || placement == null) return;
    if (placement == ImagePlacement.attach) {
      _attach(images);
    } else {
      _insertTokens(await _images.load(context.read<MailState>(), images));
    }
  }

  void _attach(List<({String path, String name})> files) {
    var added = false;
    for (final f in files) {
      if (_picked.any((p) => p.path == f.path)) continue;
      _picked.add(PickedFile(path: f.path, name: f.name));
      added = true;
    }
    if (added) setState(() => _dirty = true);
  }

  void _insertTokens(List<String> tokens) {
    if (tokens.isEmpty || !mounted) return;
    final text = _body.text;
    final at = _body.selection.start >= 0 ? _body.selection.start : text.length;
    final insert = tokens.join('\n');
    _body.value = TextEditingValue(
      text: '${text.substring(0, at)}$insert${text.substring(at)}',
      selection: TextSelection.collapsed(offset: at + insert.length),
    );
    setState(() => _dirty = true);
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
