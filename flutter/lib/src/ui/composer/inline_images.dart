import 'package:file_picker/file_picker.dart';

import '../../ffi/mail_core.dart';
import '../../state/mail_state.dart';

/// Images shown inside a Flutter-composed mail.
///
/// The body is a plain Markdown text field, so an image sits in it as a
/// short token — `![name](inline:3)` — while its `data:` URL lives here.
/// [MarkdownMail.toHtml] swaps the token for the image on send; the core
/// then turns each `data:` image into an inline `cid:` part. Deleting the
/// token from the text drops the image.
class InlineImages {
  final Map<int, String> _urls = {};
  int _next = 1;

  /// Token id → `data:` URL, for [MarkdownMail.toHtml].
  Map<int, String> get urls => Map.unmodifiable(_urls);

  /// The Markdown token for image [id].
  static String token(int id, String name) =>
      '![${name.replaceAll(RegExp(r'[\[\]]'), '')}](inline:$id)';

  /// Let the user pick image files and load each as a `data:` URL. Returns
  /// the tokens to insert; files that cannot go inline (type, size) are
  /// reported on the status line and skipped.
  Future<List<String>> pick(MailState state) async {
    List<PlatformFile> files;
    try {
      files = await FilePicker.pickFiles(type: FileType.image);
    } catch (e) {
      state.showStatus(
        'No file picker available ($e). On Linux this needs zenity, kdialog or qarma installed.',
        isError: true,
      );
      return const [];
    }
    return load(state, [
      for (final f in files)
        if (f.path != null && f.path!.isNotEmpty) (path: f.path!, name: f.name),
    ]);
  }

  /// Load [files] (picked or dropped) as inline images; returns their
  /// tokens. Files that cannot go inline are reported and skipped.
  Future<List<String>> load(
    MailState state,
    List<({String path, String name})> files,
  ) async {
    final tokens = <String>[];
    for (final (:path, :name) in files) {
      try {
        final url = await MailCore.instance.imageDataUrl(path);
        final id = _next++;
        _urls[id] = url;
        tokens.add(token(id, name));
      } catch (e) {
        state.showStatus(
          e.toString().replaceFirst('Exception: ', ''),
          isError: true,
        );
      }
    }
    return tokens;
  }
}
