import 'dart:async';

import 'package:file_picker/file_picker.dart';

import '../../ffi/mail_core.dart';
import '../../state/mail_state.dart';

/// "Save as .eml…" from the list's row menu and the reader's ⋮ menu.
///
/// Naming, attachment download and MIME assembly are mailcore's
/// (`suggestedEmlName`, `prepareEmlExport`, `exportMessageEmlBytes`); this
/// waits for the download, then asks the platform picker where to put the
/// bytes (the only way under Android's scoped storage).
Future<void> exportMessageAsEml(MailState state, int folderId, int uid) async {
  final core = MailCore.instance;
  try {
    // Register before queueing, so a fast (all cached) finish cannot slip by.
    final finished = state.nextFinished('Export');
    await core.prepareEmlExport(folderId, uid);
    final done = await finished.timeout(const Duration(minutes: 2));
    if (!done.ok) {
      state.showStatus(
        'Could not export message: ${coreErrorText(done.status)}',
        isError: true,
      );
      return;
    }
    final fileName = core.suggestedEmlName(folderId, uid);
    final dest = await FilePicker.saveFile(
      dialogTitle: 'Export message as .eml',
      fileName: fileName,
      bytes: await core.exportMessageEmlBytes(folderId, uid),
      type: FileType.custom,
      allowedExtensions: const ['eml'],
    );
    if (dest == null) return;
    state.showStatus('Exported to $dest');
  } on TimeoutException {
    state.showStatus(
      'Could not export message: attachments are still downloading',
      isError: true,
    );
  } catch (e) {
    state.showStatus(
      'Could not export message: ${coreErrorText(e)}',
      isError: true,
    );
  }
}
