import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../ffi/mail_core.dart';
import '../../models/models.dart';
import '../../state/mail_state.dart';

/// How the composer was opened — what to prefill and what Send replaces.
enum ComposeMode { blank, reply, replyAll, forward, draft }

/// The starting point for a composer. Built by the static `show*` helpers
/// from a message or a stored draft, so the dialog itself only edits text.
class ComposerInitial {
  const ComposerInitial({
    required this.mode,
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
/// Plain text only, deliberately: the Qt frontend's WYSIWYG editor has no
/// Flutter equivalent in scope, and `compose_send_format = auto` sends plain
/// text unless the body carries real formatting. Quote blocks use `> `
/// citations, which survive every format.
class ComposerDialog extends StatefulWidget {
  const ComposerDialog({super.key, required this.initial});

  final ComposerInitial initial;

  /// Blank message with the signature applied.
  static Future<void> showBlank(BuildContext context) async {
    final state = context.read<MailState>();
    await showDialog(
      context: context,
      builder: (_) => ComposerDialog(
        initial: ComposerInitial(
          mode: ComposeMode.blank,
          body: _signatureBlock(state),
        ),
      ),
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
    final answerTo =
        message.replyTo.isNotEmpty ? message.replyTo : message.from;
    final quote = _quote(message, settings.replyBelowQuote);
    final notice = message.replyTo.isNotEmpty &&
            !_sameAddress(message.replyTo, message.from)
        ? 'Replies to this mail go to ${message.replyTo} — not to the sender (${message.from}).'
        : '';
    await showDialog(
      context: context,
      builder: (_) => ComposerDialog(
        initial: ComposerInitial(
          mode: replyAll ? ComposeMode.replyAll : ComposeMode.reply,
          to: answerTo,
          cc: replyAll ? message.cc : '',
          subject: _subjectPrefix(message.subject, 'Re:'),
          body: quote + _signatureBlock(state),
          showCc: replyAll && message.cc.isNotEmpty,
          replyNotice: notice,
        ),
      ),
    );
  }

  /// Forward with a `— Forwarded message —` header and the quoted body.
  static Future<void> showForward(
      BuildContext context, MessageBody message) async {
    final state = context.read<MailState>();
    final header = '— Forwarded message —\n'
        'From: ${message.from}\n'
        'Date: ${message.date}\n'
        'Subject: ${message.subject}\n\n';
    await showDialog(
      context: context,
      builder: (_) => ComposerDialog(
        initial: ComposerInitial(
          mode: ComposeMode.forward,
          subject: _subjectPrefix(message.subject, 'Fwd:'),
          body: header + _quoteBody(message) + _signatureBlock(state),
        ),
      ),
    );
  }

  /// Continue a stored draft. Attachments come back as metadata; saving
  /// replaces the server copy, which the dialog calls out.
  static Future<void> showDraft(
      BuildContext context, int accountId, int uid) async {
    final state = context.read<MailState>();
    try {
      final form = await MailCore.instance.draftForm(accountId, uid);
      final attachments = ((form['attachments'] as List?) ?? const [])
          .whereType<Map<String, dynamic>>()
          .map(AttachmentInfo.fromJson)
          .toList(growable: false);
      if (!context.mounted) return;
      await showDialog(
        context: context,
        builder: (_) => ComposerDialog(
          initial: ComposerInitial(
            mode: ComposeMode.draft,
            to: '${form['to'] ?? ''}',
            cc: '${form['cc'] ?? ''}',
            bcc: '${form['bcc'] ?? ''}',
            replyTo: '${form['reply_to'] ?? ''}',
            subject: '${form['subject'] ?? ''}',
            body: _draftText(form),
            draftUid: (form['draft_uid'] as num?)?.toInt() ?? uid,
            showCc: '${form['cc'] ?? ''}'.isNotEmpty,
            showBcc: '${form['bcc'] ?? ''}'.isNotEmpty,
            serverAttachments:
                attachments.where((a) => !a.isInline).toList(growable: false),
          ),
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
    final cited = 'On ${message.date}, ${message.from} wrote:\n'
        '${_quoteBody(message)}\n';
    return below ? '\n\n$cited' : '$cited\n';
  }

  static String _quoteBody(MessageBody message) {
    final text =
        message.bodyText.isNotEmpty ? message.bodyText : _stripTags(message.bodyHtml);
    return text.split('\n').map((l) => '> $l').join('\n');
  }

  /// Last resort for a quote when the core stored no plain twin: drop the
  /// tags, keep the words. The reader never renders this — it only quotes.
  static String _stripTags(String html) =>
      html.replaceAll(RegExp(r'<[^>]*>'), ' ').replaceAll(RegExp(r'\s+'), ' ').trim();

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
  late final TextEditingController _to;
  late final TextEditingController _cc;
  late final TextEditingController _bcc;
  late final TextEditingController _senderName;
  late final TextEditingController _replyToCtrl;
  late final TextEditingController _subject;
  late final TextEditingController _body;
  bool _showCc = false;
  bool _showBcc = false;
  bool _showReplyTo = false;
  bool _dirty = false;
  bool _working = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    final i = widget.initial;
    _to = TextEditingController(text: i.to)..addListener(_edited);
    _cc = TextEditingController(text: i.cc)..addListener(_edited);
    _bcc = TextEditingController(text: i.bcc)..addListener(_edited);
    _senderName = TextEditingController()..addListener(_edited);
    _replyToCtrl = TextEditingController(text: i.replyTo)
      ..addListener(_edited);
    _subject = TextEditingController(text: i.subject)..addListener(_edited);
    _body = TextEditingController(text: i.body)..addListener(_edited);
    _showCc = i.showCc;
    _showBcc = i.showBcc;
    _showReplyTo = i.replyTo.isNotEmpty;
  }

  void _edited() {
    if (!_dirty) setState(() => _dirty = true);
  }

  @override
  void dispose() {
    for (final c in [
      _to,
      _cc,
      _bcc,
      _senderName,
      _replyToCtrl,
      _subject,
      _body
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<MailState>();
    final account = state.account;
    final width = MediaQuery.sizeOf(context).width;
    return Dialog(
      insetPadding: EdgeInsets.symmetric(
        horizontal: width < 700 ? 8 : 40,
        vertical: 24,
      ),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 720),
        child: Padding(
          padding: const EdgeInsets.all(20),
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
                _Notice(text: widget.initial.replyNotice),
              ],
              const SizedBox(height: 12),
              Expanded(
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text('From: ${account?.email ?? ''}',
                          style: Theme.of(context).textTheme.bodyMedium),
                      _RecipientField(
                          label: 'To',
                          controller: _to,
                          onToggleCc: () =>
                              setState(() => _showCc = !_showCc),
                          onToggleBcc: () =>
                              setState(() => _showBcc = !_showBcc),
                          onToggleReplyTo: () => setState(
                              () => _showReplyTo = !_showReplyTo)),
                      if (_showCc)
                        _RecipientField(label: 'Cc', controller: _cc),
                      if (_showBcc)
                        _RecipientField(label: 'Bcc', controller: _bcc),
                      if (_showReplyTo)
                        TextField(
                          controller: _replyToCtrl,
                          decoration: const InputDecoration(
                            labelText: 'Reply-To',
                            helperText:
                                'Replies to this message go here instead of From',
                          ),
                        ),
                      TextField(
                        controller: _senderName,
                        decoration: InputDecoration(
                          labelText: 'Sender name',
                          hintText: account?.displayName ?? '',
                        ),
                      ),
                      TextField(
                        controller: _subject,
                        decoration: const InputDecoration(labelText: 'Subject'),
                      ),
                      const SizedBox(height: 8),
                      TextField(
                        controller: _body,
                        maxLines: 14,
                        minLines: 8,
                        decoration: const InputDecoration(
                          labelText: 'Message',
                          alignLabelWithHint: true,
                          border: OutlineInputBorder(),
                        ),
                      ),
                      if (widget.initial.serverAttachments.isNotEmpty) ...[
                        const SizedBox(height: 8),
                        _Notice(
                          text:
                              '${widget.initial.serverAttachments.length} file(s) live on the server copy of this draft. Saving replaces it — re-attach them afterwards.',
                        ),
                        Wrap(
                          spacing: 8,
                          children: [
                            for (final a in widget
                                .initial.serverAttachments)
                              Chip(
                                avatar: const Icon(Icons.attach_file, size: 16),
                                label: Text(a.filename),
                              ),
                          ],
                        ),
                      ],
                      const SizedBox(height: 4),
                      Row(
                        children: [
                          const Icon(Icons.attach_file, size: 16),
                          const SizedBox(width: 4),
                          const Text('No files attached. '),
                          TextButton(
                            onPressed: () => state.showStatus(
                                'Attaching files is not wired yet — the file picker is still an open decision.'),
                            child: const Text('Add (mock)'),
                          ),
                        ],
                      ),
                      if (_error != null) ...[
                        const SizedBox(height: 4),
                        Text(_error!,
                            style: TextStyle(
                                color: Theme.of(context).colorScheme.error)),
                      ],
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Wrap(
                alignment: WrapAlignment.end,
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
                  OutlinedButton(
                    onPressed: _working ? null : _saveDraft,
                    child: _working
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child:
                                CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Text('Save draft'),
                  ),
                  FilledButton(
                    onPressed: _working ? null : _send,
                    child: const Text('Send'),
                  ),
                ],
              ),
            ],
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

  Map<String, dynamic> _form() => {
        'to': _to.text,
        'cc': _cc.text,
        'bcc': _bcc.text,
        'from': '',
        'from_name': _senderName.text,
        'reply_to': _showReplyTo ? _replyToCtrl.text : '',
        'subject': _subject.text,
        'body': _body.text,
        'body_html': '',
        'attachments': const [],
        'draft_uid': widget.initial.draftUid,
      };

  Future<void> _send() async {
    final state = context.read<MailState>();
    setState(() {
      _working = true;
      _error = null;
    });
    try {
      // Validation and queueing happen inline in the core: a mistake comes
      // back here with the composer still open and the text intact. Only the
      // SMTP submit runs in the background, reported on the status line.
      await MailCore.instance.sendMail(
          state.accountId, state.folderId, _form());
      if (!mounted) return;
      Navigator.of(context).pop();
      state.showStatus('Sending…');
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _working = false;
        _error = _message(e);
      });
    }
  }

  Future<void> _saveDraft() async {
    final state = context.read<MailState>();
    setState(() {
      _working = true;
      _error = null;
    });
    try {
      await MailCore.instance.saveDraft(state.accountId, _form());
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _working = false;
        _error = _message(e);
      });
    }
  }

  Future<void> _deleteDraft() async {
    final state = context.read<MailState>();
    final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('Delete draft?'),
            content: const Text(
                'The server copy is destroyed permanently. This cannot be undone.'),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(true),
                child: const Text('Delete draft'),
              ),
            ],
          ),
        ) ??
        false;
    if (!confirmed || !mounted) return;
    try {
      await MailCore.instance
          .deleteDraft(state.accountId, widget.initial.draftUid);
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
    final choice = await showDialog<_DiscardChoice>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('Unsent changes'),
            content: const Text(
                'Discard this message, or keep it as a draft first?'),
            actions: [
              TextButton(
                onPressed: () =>
                    Navigator.of(context).pop(_DiscardChoice.cancel),
                child: const Text('Cancel'),
              ),
              TextButton(
                onPressed: () =>
                    Navigator.of(context).pop(_DiscardChoice.discard),
                child: const Text('Discard'),
              ),
              FilledButton(
                onPressed: () =>
                    Navigator.of(context).pop(_DiscardChoice.save),
                child: const Text('Save draft'),
              ),
            ],
          ),
        ) ??
        _DiscardChoice.cancel;
    if (!mounted) return;
    switch (choice) {
      case _DiscardChoice.cancel:
        break;
      case _DiscardChoice.discard:
        Navigator.of(context).pop();
      case _DiscardChoice.save:
        await _saveDraft();
    }
  }

  static String _message(Object e) =>
      e is Exception ? e.toString().replaceFirst('Exception: ', '') : '$e';
}

enum _DiscardChoice { cancel, discard, save }

class _Notice extends StatelessWidget {
  const _Notice({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Text(text, style: Theme.of(context).textTheme.bodySmall),
    );
  }
}

/// A recipient line with contact suggestions from sent mail.
///
/// Only the segment being typed is completed; picking a suggestion replaces
/// just that segment, so a half-typed list is never clobbered.
class _RecipientField extends StatelessWidget {
  const _RecipientField({
    required this.label,
    required this.controller,
    this.onToggleCc,
    this.onToggleBcc,
    this.onToggleReplyTo,
  });

  final String label;
  final TextEditingController controller;
  final VoidCallback? onToggleCc;
  final VoidCallback? onToggleBcc;
  final VoidCallback? onToggleReplyTo;

  @override
  Widget build(BuildContext context) {
    final collect =
        context.select<MailState, bool>((s) => s.settings.collectContacts);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: collect
              ? Autocomplete<Contact>(
                  fieldViewBuilder:
                      (context, fieldController, focusNode, onSubmit) {
                    // Keep the outer controller authoritative: the inner one
                    // mirrors it, and edits flow back through it.
                    if (fieldController.text != controller.text) {
                      fieldController.text = controller.text;
                    }
                    return TextField(
                      controller: fieldController,
                      focusNode: focusNode,
                      onChanged: (v) {
                        if (v != controller.text) controller.text = v;
                      },
                      decoration:
                          InputDecoration(labelText: label),
                    );
                  },
                  optionsBuilder: (value) async {
                    final query = _currentSegment(value.text);
                    if (query.isEmpty) return const Iterable<Contact>.empty();
                    try {
                      return await MailCore.instance
                          .contacts(prefix: query);
                    } catch (_) {
                      return const Iterable<Contact>.empty();
                    }
                  },
                  displayStringForOption: (c) => c.address,
                  onSelected: (c) {
                    controller.text =
                        _replaceSegment(controller.text, c.address);
                  },
                )
              : TextField(
                  controller: controller,
                  decoration: InputDecoration(labelText: label),
                ),
        ),
        if (onToggleCc != null)
          TextButton(onPressed: onToggleCc, child: const Text('Cc')),
        if (onToggleBcc != null)
          TextButton(onPressed: onToggleBcc, child: const Text('Bcc')),
        if (onToggleReplyTo != null)
          TextButton(
              onPressed: onToggleReplyTo, child: const Text('Reply-To')),
      ],
    );
  }

  static String _currentSegment(String text) {
    final parts = text.split(RegExp(r'[,;]'));
    return parts.isEmpty ? '' : parts.last.trim();
  }

  static String _replaceSegment(String text, String address) {
    final idx = text.lastIndexOf(RegExp(r'[,;]'));
    final head = idx < 0 ? '' : '${text.substring(0, idx + 1)} ';
    return '$head$address';
  }
}
