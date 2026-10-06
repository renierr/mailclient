import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/ui/sidebar/folder_sidebar.dart';

Folder _folder(
  int id,
  String path,
  String role, {
  int unread = 0,
  int total = 0,
  int depth = 0,
  String? leaf,
  bool? alwaysVisible,
}) {
  final json = <String, Object>{
    'id': id,
    'name': path,
    'role': role,
    'unread': unread,
    'count': total,
    'subscribed': true,
    'delimiter': '/',
    'depth': depth,
    'leaf': leaf ?? path,
  };
  if (alwaysVisible != null) json['always_visible'] = alwaysVisible;
  return Folder.fromJson(json);
}

void main() {
  group('collapseFolders', () {
    List<Folder> folders() => [
      _folder(1, 'INBOX', 'inbox'),
      _folder(
        2,
        'INBOX/Archive',
        'archive',
        depth: 1,
        leaf: 'Archive',
        alwaysVisible: true,
      ),
      _folder(
        3,
        'INBOX/Work',
        'custom',
        depth: 1,
        leaf: 'Work',
        unread: 2,
        total: 5,
        // The core marks the inbox's direct children always-visible, even
        // custom ones (servers that file everything below the inbox).
        alwaysVisible: true,
      ),
      _folder(4, 'Work', 'custom'),
      _folder(
        5,
        'Work/Client',
        'custom',
        depth: 1,
        leaf: 'Client',
        unread: 3,
        total: 7,
        alwaysVisible: false,
      ),
    ];

    test('custom children hide until expanded, known children always show', () {
      final rows = collapseFolders(folders(), {});
      expect(
        rows.map((r) => r.folder.path),
        ['INBOX', 'INBOX/Archive', 'INBOX/Work', 'Work'],
      );
      // INBOX hides nothing (every child stays visible): no chevron, own
      // counts only. Work folds its custom child away.
      expect(rows[0].collapsible, isFalse);
      expect(rows[0].expanded, isFalse);
      expect(rows[3].collapsible, isTrue);
    });

    test('the inbox direct children show; deeper customs fold away', () {
      final flat = [
        _folder(1, 'INBOX', 'inbox'),
        _folder(
          2,
          'INBOX/Mine',
          'custom',
          depth: 1,
          leaf: 'Mine',
          alwaysVisible: true,
        ),
        _folder(
          3,
          'INBOX/Mine/Deep',
          'custom',
          depth: 2,
          leaf: 'Deep',
          alwaysVisible: false,
        ),
      ];
      final rows = collapseFolders(flat, {});
      expect(
        rows.map((r) => r.folder.path),
        ['INBOX', 'INBOX/Mine'],
      );
      final mine = rows.firstWhere((r) => r.folder.path == 'INBOX/Mine');
      expect(mine.collapsible, isTrue);
    });

    test('a collapsed parent aggregates its hidden children counts', () {
      final rows = collapseFolders(folders(), {});
      final work = rows.firstWhere((r) => r.folder.path == 'Work');
      expect(work.unread, 3);
      expect(work.total, 7);
      // Nothing hides below INBOX here, so it carries only its own counts.
      final inbox = rows.firstWhere((r) => r.folder.path == 'INBOX');
      expect(inbox.unread, 0);
      expect(inbox.total, 0);
    });

    test('expanding reveals the children with their own counts', () {
      final rows = collapseFolders(folders(), {4});
      expect(
        rows.map((r) => r.folder.path),
        ['INBOX', 'INBOX/Archive', 'INBOX/Work', 'Work', 'Work/Client'],
      );
      final work = rows.firstWhere((r) => r.folder.path == 'Work');
      expect(work.expanded, isTrue);
      expect(work.unread, 0);
      final client = rows.firstWhere((r) => r.folder.path == 'Work/Client');
      expect(client.unread, 3);
    });

    test('a payload without the flag falls back to the role', () {
      final custom = _folder(1, 'Work', 'custom');
      final inbox = _folder(2, 'INBOX', 'inbox');
      expect(custom.alwaysVisible, isFalse);
      expect(inbox.alwaysVisible, isTrue);
    });
  });
}
