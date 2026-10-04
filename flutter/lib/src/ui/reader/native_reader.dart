import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../ffi/mail_core.dart';
import '../composer/composer_dialog.dart';
import '../../state/mail_state.dart';

/// Experiment (branch `experiment/native-reader`): the native
/// `ReaderActivity` for the scroll comparison, and the shell flows it
/// delegates back to Flutter.
///
/// Open carries ids plus a settings snapshot — never bodies (inline images
/// exceed the Binder transaction limit); the activity re-reads everything
/// else over JNI. Coming back, the activity asks for composer/find-similar
/// (`onReaderAction`) or a list refresh after its own mutations
/// (`onReaderChanged`).
class NativeReader {
  static const _channel = MethodChannel('mailclient/reader');

  /// Dialog context for delegated flows, set as the app's navigator key.
  static final GlobalKey<NavigatorState> navigatorKey =
      GlobalKey<NavigatorState>();

  static bool get available => !kIsWeb && Platform.isAndroid;

  static Future<bool> openMessage(MailState state, int uid) async {
    if (!available) return false;
    final settings = state.settings;
    try {
      await _channel.invokeMethod<void>('open', {
        'accountId': state.accountId,
        'folderId': state.folderId,
        'uid': uid,
        'autoMarkRead': settings.autoMarkRead,
        'markReadDelaySecs': settings.markReadDelaySecs,
        'loadRemoteImages': settings.loadRemoteImages,
        'linkClickAction': settings.linkClickAction,
        'readerScale': settings.readerScale,
        'deleteIsPermanent': state.deleteIsPermanent,
        'confirmDelete': settings.confirmDelete,
      });
      return true;
    } on PlatformException {
      return false;
    }
  }

  static void listenForActions(MailState state) {
    if (!available) return;
    _channel.setMethodCallHandler((call) async {
      switch (call.method) {
        case 'onReaderAction':
          final raw = call.arguments;
          if (raw is! String) return;
          final req = jsonDecode(raw);
          if (req is! Map) return;
          await _handleAction(state, '${req['kind']}', req);
        case 'onReaderChanged':
          await state.reloadFromCache();
      }
    });
  }

  static Future<void> _handleAction(
    MailState state,
    String kind,
    Map req,
  ) async {
    final folderId = (req['folderId'] as num?)?.toInt() ?? -1;
    final uid = (req['uid'] as num?)?.toInt() ?? -1;
    if (folderId < 0 || uid < 0) return;
    switch (kind) {
      case 'similar':
        await state.findSimilar(folderId, uid);
        return;
      case 'reply':
      case 'replyAll':
      case 'forward':
        break;
      default:
        return;
    }
    final context = navigatorKey.currentState?.overlay?.context;
    if (context == null) return;
    try {
      final message = await MailCore.instance.message(folderId, uid);
      if (!context.mounted) return;
      switch (kind) {
        case 'reply':
          await ComposerDialog.showReply(context, message);
        case 'replyAll':
          await ComposerDialog.showReply(context, message, replyAll: true);
        case 'forward':
          await ComposerDialog.showForward(context, message);
      }
    } catch (e) {
      state.showStatus(coreErrorText(e), isError: true);
    }
  }
}
