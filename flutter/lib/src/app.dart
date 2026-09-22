import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'ffi/mail_core.dart';
import 'state/mail_state.dart';
import 'theme/app_theme.dart';
import 'ui/shell/mail_shell.dart';

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
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState lifecycle) {
    // The only reliable "we are going away" signal Flutter offers. Dropping
    // the pooled IMAP sessions here means the server reaps them now rather
    // than on its own timeout; missing it costs nothing worse than that.
    if (lifecycle == AppLifecycleState.detached) {
      widget.core.shutdown();
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
              (s) => s.settings.uiScale);
          final mq = MediaQuery.of(context);
          return MediaQuery(
            data: mq.copyWith(
              textScaler: TextScaler.linear(scale),
            ),
            child: child ?? const SizedBox.shrink(),
          );
        },
        home: const MailShell(),
      ),
    );
  }
}
