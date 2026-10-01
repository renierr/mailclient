import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ui/dialogs/mail_dialog.dart';

void main() {
  test('name initial plus the domain initial', () {
    expect(senderInitials('Alice', 'alice@example.com'), 'AE');
    expect(senderInitials('Alice', 'news@mail.example.org'), 'AE');
    expect(senderInitials('Alice', 'a@shop.example.co.uk'), 'AE');
    expect(senderInitials('', 'bob@example.net'), 'BE');
    expect(senderInitials('"Carol"', 'c@example.com'), 'CE');
  });

  test('without a usable domain only one letter', () {
    expect(senderInitials('Alice', ''), 'A');
    expect(senderInitials('Alice', 'alice'), 'A');
    expect(senderInitials('', ''), '?');
  });
}
