import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'ffi/mail_core.dart';
import 'state/mail_state.dart';
import 'sync/background_sync.dart';
import 'theme/app_theme.dart';
import 'ui/shell/mail_shell.dart';
import 'ui/shell/undo_snack_bar_host.dart';

/// The app, with the loaded core wired into the widget tree.
class MailApp extends StatefulWidget {
  const MailApp({super.key, required this.core});

  final MailCore core;

  @override
  State<MailApp> createState() => _MailAppState();
}

class _MailAppState extends State<MailApp> with WidgetsBindingObserver {
  late final MailState _state = MailState(widget.core)..start();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    WidgetsBinding.instance.addPostFrameCallback((_) => _wireNotifications());
  }

  /// Route notification taps to the tapped message and ask Android 13+ for
  /// the runtime notification permission (only when background checks exist
  /// to notify about). Waits for the mailbox to be ready first, so a
  /// cold-start tap lands on a loaded list rather than an empty shell.
  Future<void> _wireNotifications() async {
    if (!Platform.isAndroid) return;
    for (var i = 0; i < 150 && _state.loading && mounted; i++) {
      await Future.delayed(const Duration(milliseconds: 200));
    }
    if (!mounted) return;
    await clearMailNotification();
    await _markSeen();
    if ((await _state.backgroundPlan()).any) {
      await requestNotificationPermission();
    }
    await listenForBackgroundEvents(
      onOpen: (payload) async {
        final target = parseOpenPayload(payload);
        if (target == null || !_state.hasAccounts) return;
        await _state.openMail(
          accountId: target.accountId,
          folderId: target.folderId,
          uid: target.uid,
        );
      },
      onMailChanged: _state.reloadFromCache,
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState lifecycle) {
    // The only reliable "we are going away" signal Flutter offers. Dropping
    // the pooled IMAP sessions here means the server reaps them now rather
    // than on its own timeout; missing it costs nothing worse than that.
    if (lifecycle == AppLifecycleState.detached) {
      widget.core.shutdown();
    }
    if (!Platform.isAndroid) return;
    switch (lifecycle) {
      case AppLifecycleState.resumed:
        unawaited(_backInForeground());
      case AppLifecycleState.paused:
        // Mail the foreground synced while the user had the app open was
        // on screen: the background check must not alert for it later.
        unawaited(_markSeen());
      default:
        break;
    }
  }

  /// Back from the background: the user is looking at the mailbox, so the
  /// notification has done its job, and the list catches up.
  Future<void> _backInForeground() async {
    await clearMailNotification();
    await _markSeen();
    await _state.resumed();
  }

  Future<void> _markSeen() async {
    try {
      await widget.core.backgroundMarkSeen();
    } catch (_) {
      // Only costs a duplicate alert later.
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _state.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ChangeNotifierProvider<MailState>.value(
      value: _state,
      child: MaterialApp(
        title: 'Mail',
        theme: AppTheme.light(),
        darkTheme: AppTheme.dark(),
        debugShowCheckedModeBanner: false,
        // The interface-scale setting, applied as text scaling: layout
        // already adapts by width, so type is what grows.
        builder: (context, child) {
          final scale = context.select<MailState, double>(
            (s) => s.settings.uiScale,
          );
          final mq = MediaQuery.of(context);
          return MediaQuery(
            data: mq.copyWith(textScaler: TextScaler.linear(scale)),
            child: child ?? const SizedBox.shrink(),
          );
        },
        home: const UndoSnackBarHost(child: MailShell()),
      ),
    );
  }
}
