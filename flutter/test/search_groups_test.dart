import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/models/settings.dart';
import 'package:mailclient/src/ui/message_list/message_list_widgets.dart';

SearchHit hit(int uid, String folder) => SearchHit(
  uid: uid,
  folderId: -1,
  folder: folder,
  subject: 's$uid',
  from: 'a@example.com',
  date: '',
  snippet: '',
  unread: false,
  starred: false,
  hasAttachments: false,
);

void main() {
  test('a header starts each folder of the grouped hits', () {
    // Grouped by the core already (`feed::search_json`).
    final rows = groupHitsByFolder([
      hit(1, 'INBOX'),
      hit(3, 'INBOX'),
      hit(2, 'Archive'),
      hit(4, 'Archive'),
    ]);

    expect(rows.map((r) => r is SearchHit ? r.uid : r).toList(), [
      'INBOX',
      1,
      3,
      'Archive',
      2,
      4,
    ]);
  });

  test('similar results keep folder headers whatever the scope says', () {
    // A plain folder-scoped search is all one folder: no headers.
    expect(
      showFolderHeaders(folderOnly: true, isSimilar: false),
      isFalse,
    );
    expect(
      showFolderHeaders(folderOnly: false, isSimilar: false),
      isTrue,
    );
    // Similar spans the account even with the Folder toggle on (Qt does
    // the same), so the headers stay.
    expect(
      showFolderHeaders(folderOnly: true, isSimilar: true),
      isTrue,
    );
    expect(
      showFolderHeaders(folderOnly: false, isSimilar: true),
      isTrue,
    );
  });

  test('the same uid in two folders is two selection keys', () {
    expect(hit(5, 'INBOX').key == hit(5, 'Archive').key, isFalse);
    expect(hit(5, 'INBOX').key == hit(5, 'INBOX').key, isTrue);
  });

  test("a hit carries the list row's sender name and badge", () {
    final h = SearchHit.fromJson({
      'uid': 7,
      'folder_id': 3,
      'folder': 'INBOX',
      'subject': 'Hello',
      'from': 'ann@example.com',
      'from_name': 'Ann Example',
      'initials': 'AE',
      'avatar_light': '#112233',
      'avatar_dark': '#445566',
      'date': '09:41',
      'has_attachments': true,
    });
    final row = h.summary;

    expect(row.senderName, 'Ann Example');
    expect(row.badge.initials, 'AE');
    expect(row.badge.dark, '#445566');
    expect(row.date, '09:41');
    expect(row.hasAttachments, isTrue);
  });
}
