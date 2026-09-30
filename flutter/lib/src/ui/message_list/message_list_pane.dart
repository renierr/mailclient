import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../../models/models.dart';
import '../../models/settings.dart';
import '../../state/mail_state.dart';
import '../composer/composer_dialog.dart';
import 'message_list_widgets.dart';

export 'message_list_widgets.dart' show confirmDelete;

/// The message list for the selected folder — or account-wide search hits.
///
/// Rows come from the compact feed — subject, sender, snippet and flags, no
/// body — so opening a folder of two hundred mails costs no sanitizing work.
/// Selecting a row is what asks for the body.
class MessageListPane extends StatefulWidget {
  const MessageListPane({super.key, this.onMessageOpened});

  /// Lets a narrow layout navigate to the reader. Null in three-pane.
  final VoidCallback? onMessageOpened;

  @override
  State<MessageListPane> createState() => MessageListPaneState();
}

class MessageListPaneState extends State<MessageListPane> {
  int? _anchorUid;

  @override
  Widget build(BuildContext context) {
    final searching = context.select<MailState, bool>((s) => s.searching);
    if (searching) return _searchResults();
    return _folderList();
  }

  Widget _folderList() {
    final folderId = context.select<MailState, int>((s) => s.folderId);
    final messages = context.select<MailState, List<MessageSummary>>(
      (s) => s.messages,
    );
    final query = context.select<MailState, String>((s) => s.searchQuery);
    final openUid = context.select<MailState, int>((s) => s.openUid);
    final selected = context.select<MailState, Set<int>>(
      (s) => Set<int>.of(s.selectedUids),
    );
    final selectionMode = context.select<MailState, bool>(
      (s) => s.selectionMode,
    );
    final compact = context.select<MailState, bool>(
      (s) => s.settings.isCompact,
    );
    final syncing = context.select<MailState, bool>((s) => s.isSyncing);
    final drafts = context.select<MailState, bool>((s) => s.isDraftsFolder);
    // Qt parity: 1–2 letter input filters the folder instantly (substring);
    // 3+ letters run the FTS index via `searching`. Without this, short input
    // shows the whole unfiltered folder.
    final q = query.trim().toLowerCase();
    final shown = q.isEmpty
        ? messages
        : messages
              .where(
                (m) =>
                    m.subject.toLowerCase().contains(q) ||
                    m.from.toLowerCase().contains(q) ||
                    m.senderName.toLowerCase().contains(q) ||
                    m.snippet.toLowerCase().contains(q),
              )
              .toList(growable: false);
    if (folderId < 0) {
      return const EmptyPane(
        icon: Icons.folder_open_outlined,
        text: 'Pick a folder',
      );
    }
    return Column(
      children: [
        const MessageListHeader(),
        if (q.isNotEmpty)
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: Text(
              '${shown.length} match(es) for “${query.trim()}”',
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
        if (selectionMode && selected.isNotEmpty)
          BulkActionBar(onAction: () => setState(() => _anchorUid = null)),
        Expanded(
          child: shown.isEmpty
              ? EmptyPane(
                  icon: Icons.mail_outline,
                  text: syncing ? 'Syncing…' : 'Nothing here',
                )
              : ListView.separated(
                  // The pane is rebuilt from scratch whenever the reader
                  // takes its place (one and two panes), and an unkeyed
                  // scrollable shares its PageStorage slot with every other
                  // one on the route. A per-folder key brings the list back
                  // where it was, and each folder keeps its own position.
                  key: PageStorageKey<String>('message-list-$folderId'),
                  itemCount: shown.length + 1,
                  separatorBuilder: (_, _) => const Divider(height: 1),
                  itemBuilder: (context, i) {
                    // The tail row asks the server for the next older batch.
                    // It is a button rather than an infinite scroll on
                    // purpose: each press is a deliberate, sizeable download.
                    if (i == shown.length) return const LoadOlderTile();
                    final m = shown[i];
                    return MessageTile(
                      message: m,
                      selected: m.uid == openUid,
                      checked: selected.contains(m.uid),
                      selectionMode: selectionMode,
                      compact: compact,
                      onTap: () => _onRowTap(m, drafts),
                      onToggle: () => _onRowToggle(m),
                    );
                  },
                ),
        ),
      ],
    );
  }

  void _onRowTap(MessageSummary m, bool drafts) {
    final state = context.read<MailState>();
    if (state.selectionMode) {
      _onRowToggle(m);
      return;
    }
    setState(() => _anchorUid = m.uid);
    if (drafts) {
      // Drafts open in the composer, never in the reader.
      ComposerDialog.showDraft(context, state.accountId, m.uid);
      return;
    }
    state.openMessage(m.uid);
    widget.onMessageOpened?.call();
  }

  void _onRowToggle(MessageSummary m) {
    final state = context.read<MailState>();
    if (_shiftHeld && _anchorUid != null && _anchorUid != m.uid) {
      state.selectRange(_anchorUid!, m.uid);
    } else {
      state.toggleSelect(m.uid);
    }
    setState(() => _anchorUid = m.uid);
  }

  bool get _shiftHeld {
    final keys = HardwareKeyboard.instance.logicalKeysPressed;
    return keys.contains(LogicalKeyboardKey.shiftLeft) ||
        keys.contains(LogicalKeyboardKey.shiftRight);
  }

  Widget _searchResults() {
    final hits = context.select<MailState, List<SearchHit>>(
      (s) => s.searchHits,
    );
    final folderOnly = context.select<MailState, bool>(
      (s) => s.searchFolderOnly,
    );
    final query = context.select<MailState, String>((s) => s.searchQuery);
    final busy = context.select<MailState, bool>((s) => s.isBusy);
    return Column(
      children: [
        Container(
          width: double.infinity,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Text(
            '${hits.length} result(s) across ${folderOnly ? 'this folder' : 'this account'}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: hits.isEmpty
              ? EmptyPane(
                  icon: Icons.search_off_outlined,
                  text: busy
                      ? 'Searching the server…'
                      : 'No matches for “$query”',
                )
              : ListView.separated(
                  itemCount: hits.length,
                  separatorBuilder: (_, _) => const Divider(height: 1),
                  itemBuilder: (context, i) => SearchHitTile(
                    hit: hits[i],
                    onOpened: widget.onMessageOpened,
                  ),
                ),
        ),
      ],
    );
  }
}
