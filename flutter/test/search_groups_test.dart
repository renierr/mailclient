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
  test('hits group by folder in order of each folder\'s best hit', () {
    final rows = groupHitsByFolder([
      hit(1, 'INBOX'),
      hit(2, 'Archive'),
      hit(3, 'INBOX'),
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
