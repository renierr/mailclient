import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../state/mail_state.dart';

/// Shows "Moved 3 to Trash — Undo" whenever [MailState] offers an undo.
///
/// Sits above the shell (under the app's ScaffoldMessenger) and only turns
/// each new offer into a SnackBar; the queueing and the undo itself live in
/// the core. A newer offer replaces the older one's bar, as the older action
/// is then only undoable until its own grace period ends.
class UndoSnackBarHost extends StatefulWidget {
  const UndoSnackBarHost({super.key, required this.child});

  final Widget child;

  @override
  State<UndoSnackBarHost> createState() => _UndoSnackBarHostState();
}

class _UndoSnackBarHostState extends State<UndoSnackBarHost> {
  MailState? _state;
  int _shownSeq = 0;
  ScaffoldFeatureController<SnackBar, SnackBarClosedReason>? _bar;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final state = context.read<MailState>();
    if (!identical(state, _state)) {
      _state?.removeListener(_onChange);
      _state = state..addListener(_onChange);
      _shownSeq = state.undoOffer?.seq ?? 0;
    }
  }

  @override
  void dispose() {
    _state?.removeListener(_onChange);
    super.dispose();
  }

  void _onChange() {
    final state = _state;
    final offer = state?.undoOffer;
    // Taken back (Ctrl+Z, or the bar's own button): the bar has nothing
    // left to offer.
    if (offer == null) {
      _bar?.close();
      _bar = null;
    }
    if (state == null || offer == null || offer.seq == _shownSeq) return;
    _shownSeq = offer.seq;
    final messenger = ScaffoldMessenger.maybeOf(context);
    if (messenger == null) return;
    messenger.hideCurrentSnackBar();
    final bar = _bar = messenger.showSnackBar(
      SnackBar(
        content: Text(offer.label),
        duration: Duration(seconds: state.undoGraceSecs),
        // A bar with an action persists by default; this one must go
        // when the grace period ends, as the action is then gone too.
        persist: false,
        behavior: SnackBarBehavior.floating,
        action: SnackBarAction(
          label: 'Undo',
          onPressed: () => state.undo(offer.batch),
        ),
      ),
    );
    bar.closed.whenComplete(() {
      if (identical(_bar, bar)) _bar = null;
    });
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
