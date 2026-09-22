import 'package:flutter/material.dart';

import 'src/app.dart';
import 'src/ffi/mail_core.dart';

/// Load the Rust core, then start the UI.
///
/// The core is loaded before the first frame on purpose: it opens the database
/// and runs migrations, and a UI that paints an empty mailbox because that
/// failed is worse than a UI that has not painted yet. A failure here is
/// therefore shown as its own screen rather than swallowed.
Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    final core = await MailCore.load();
    runApp(MailApp(core: core));
  } catch (e, stack) {
    debugPrint('mailclient: core failed to load: $e\n$stack');
    runApp(_CoreLoadFailed(error: '$e'));
  }
}

class _CoreLoadFailed extends StatelessWidget {
  const _CoreLoadFailed({required this.error});

  final String error;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(Icons.error_outline, size: 48),
                const SizedBox(height: 16),
                const Text('The mail core could not be loaded.'),
                const SizedBox(height: 8),
                SelectableText(error, textAlign: TextAlign.center),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
