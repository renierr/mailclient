import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mailclient/src/ffi/mail_core.dart';
import 'package:mailclient/src/models/models.dart';
import 'package:mailclient/src/models/settings.dart';
import 'package:mailclient/src/state/mail_state.dart';
import 'package:mailclient/src/ui/reader/attachment_card.dart';
import 'package:mailclient/src/ui/reader/event_card.dart';
import 'package:mailclient/src/ui/reader/inline_images_banner.dart';
import 'package:mailclient/src/ui/reader/mail_html_view.dart';
import 'package:mailclient/src/ui/reader/reader_header.dart';
import 'package:mailclient/src/ui/reader/reader_pane.dart';
import 'package:provider/provider.dart';

/// A core that holds one message and nothing else.
class _OneMessageCore implements MailCore {
  _OneMessageCore(this.body);

  final Map<String, dynamic> body;

  @override
  Stream<JobEvent> jobEvents() => const Stream.empty();

  @override
  Future<MessageBody> message(int folderId, int uid) async =>
      MessageBody.fromJson(body);

  @override
  Future<MessageHeaders> messageHeaders(int folderId, int uid) =>
      Future.error(StateError('no headers in this test'));

  // The reader document is the core's; these stand-ins only keep the
  // widgets building.
  @override
  ReaderPaint readerPaint(bool colored, bool dark, bool keepOriginal) =>
      !colored
      ? ReaderPaint.theme
      : dark && !keepOriginal
      ? ReaderPaint.darkened
      : ReaderPaint.original;

  @override
  ReaderPalette readerPalette(ReaderPaint paint, ReaderPalette theme) => theme;

  @override
  String readerBody(String body, ReaderPaint paint, {bool fit = false}) => body;

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  // The wide shell puts the reader in a Row, which hands it a loose height.
  // The reader has to fill it anyway: it once sized itself to its empty
  // hover bubble and showed nothing at all.
  testWidgets('the reader fills a loosely constrained pane', (tester) async {
    final core = _OneMessageCore({
      'uid': 7,
      'subject': 'Plain',
      'from': 'someone@example.com',
      'body_text': 'Hello plain world',
      'is_html': false,
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: const MaterialApp(
          home: Scaffold(
            body: Row(children: [Expanded(child: ReaderPane())]),
          ),
        ),
      ),
    );
    await state.openMessage(7);
    await tester.pump();

    expect(tester.takeException(), isNull);
    final pane = tester.getSize(find.byType(ReaderPane));
    final screen = tester.getSize(find.byType(Scaffold));
    expect(pane.height, screen.height);
    expect(find.text('Hello plain world'), findsOneWidget);
  });

  // One scrolling page: header and attachments move with the body, and
  // nothing overflows at phone width.
  testWidgets('header and attachments scroll with the body on a phone', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 640);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final core = _OneMessageCore({
      'uid': 8,
      'subject': 'A subject long enough to wrap onto a second line here',
      'from': 'someone@example.com',
      'body_text': List.filled(200, 'line of body text').join('\n'),
      'is_html': false,
      'attachments': [
        {'id': 1, 'filename': 'a-rather-long-file-name.pdf', 'size': 2048},
        {'id': 2, 'filename': 'b.txt', 'size': 4},
      ],
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: const MaterialApp(home: Scaffold(body: ReaderPane())),
      ),
    );
    await state.openMessage(8);
    await tester.pump();

    expect(tester.takeException(), isNull);
    expect(find.text('2 attachments'), findsOneWidget);
    final subject = find.textContaining('A subject long enough');
    final before = tester.getTopLeft(subject).dy;
    await tester.drag(find.byType(CustomScrollView), const Offset(0, -300));
    await tester.pump();
    expect(tester.getTopLeft(subject).dy, lessThan(before - 200));
  });

  // Original colours show the mail as sent: its own palette and its
  // original fixed-width layout, sideways scroll included. The toggle
  // flips MailHtmlView.fitWidths with it.
  testWidgets('original colours also restore the original width layout', (
    tester,
  ) async {
    final core = _OneMessageCore({
      'uid': 9,
      'subject': 'Newsletter',
      'from': 'news@example.com',
      'body_text': 'fallback',
      'is_html': true,
      'html_colored': true,
      'body_html': '<table width="600"><tr><td>hi</td></tr></table>',
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    await tester.pumpWidget(
      ChangeNotifierProvider.value(
        value: state,
        child: MaterialApp(
          theme: ThemeData.dark(),
          home: const Scaffold(body: ReaderPane()),
        ),
      ),
    );
    await state.openMessage(9);
    await tester.pump();

    expect(tester.takeException(), isNull);
    MailHtmlView view() =>
        tester.widget<MailHtmlView>(find.byType(MailHtmlView));
    expect(view().fitWidths, isTrue);

    await tester.tap(find.byTooltip('Show original colours'));
    await tester.pump();
    expect(view().fitWidths, isFalse);

    await tester.tap(find.byTooltip('Darken to match the theme'));
    await tester.pump();
    expect(view().fitWidths, isTrue);
  });

  // Over a WebView (Android) the header passes touches through to the page:
  // display text sits under IgnorePointer so drags and flings starting on
  // it are the WebView's own, while the buttons stay tappable. Elsewhere
  // the header handles touches as before.
  testWidgets('pass-through header keeps buttons live, text untouchable', (
    tester,
  ) async {
    final body = {
      'uid': 10,
      'subject': 'Through subject',
      'from': 'someone@example.com',
      'body_text': 'x',
      'is_html': false,
    };
    MailCore.debugInstance = _OneMessageCore(body);
    Widget header(bool passThrough) {
      return ChangeNotifierProvider.value(
        value: MailState(MailCore.instance),
        child: MaterialApp(
          home: Scaffold(
            body: ReaderHeader(
              message: MessageBody.fromJson(body),
              headersFuture: Future<MessageHeaders?>.value(null),
              details: true,
              onToggleDetails: () {},
              onClose: () {},
              passThrough: passThrough,
            ),
          ),
        ),
      );
    }

    await tester.pumpWidget(header(true));
    await tester.pump();
    expect(tester.takeException(), isNull);
    // Subject, sender block and details all pass through: an ignoring
    // IgnorePointer above the text. (The framework adds its own
    // non-ignoring ones higher up; those stay out of this match.)
    final ignoring = find.byWidgetPredicate(
      (w) => w is IgnorePointer && w.ignoring,
    );
    expect(
      find.ancestor(of: find.text('Through subject'), matching: ignoring),
      findsOneWidget,
    );
    // … while the buttons keep their taps.
    expect(find.byTooltip('Back to the list'), findsOneWidget);
    expect(find.byTooltip('Reply'), findsOneWidget);

    await tester.pumpWidget(header(false));
    await tester.pump();
    expect(tester.takeException(), isNull);
    expect(
      find.ancestor(of: find.text('Through subject'), matching: ignoring),
      findsNothing,
    );
    expect(find.byTooltip('Reply'), findsOneWidget);
  });

  // The banner and card pass through the same way: labels fall to the page,
  // actions stay tappable.
  testWidgets('pass-through banner and card keep actions live', (tester) async {
    var downloaded = false;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              InlineImagesBanner(
                count: 2,
                busy: false,
                onDownload: () => downloaded = true,
                passThrough: true,
              ),
              AttachmentCard(
                message: MessageBody.fromJson({
                  'uid': 11,
                  'subject': 'Files',
                  'from': 'someone@example.com',
                  'body_text': 'x',
                  'is_html': false,
                  'attachments': [
                    {'id': 1, 'filename': 'a.pdf', 'size': 10},
                  ],
                }),
                passThrough: true,
              ),
            ],
          ),
        ),
      ),
    );
    await tester.pump();
    expect(tester.takeException(), isNull);
    final ignoring = find.byWidgetPredicate(
      (w) => w is IgnorePointer && w.ignoring,
    );
    expect(
      find.ancestor(
        of: find.textContaining('embedded images'),
        matching: ignoring,
      ),
      findsOneWidget,
    );
    expect(
      find.ancestor(of: find.text('a.pdf'), matching: ignoring),
      findsOneWidget,
    );
    await tester.tap(find.text('Download'));
    await tester.pump();
    expect(downloaded, isTrue);
  });

  testWidgets('renders calendar event card when invite is present', (
    tester,
  ) async {
    final core = _OneMessageCore({
      'uid': 12,
      'subject': 'Sprint Review',
      'from': 'alice@example.org',
      'body_text': 'Meeting agenda...',
      'is_html': false,
      'event': {
        'summary': 'Sprint Review',
        'location': 'Room 101',
        'organizer': 'Alice <alice@example.org>',
        'formatted_time': 'Tue, Oct 6, 2026 · 14:00 – 15:00',
        'is_all_day': false,
        'is_cancelled': false,
        'attachment_id': 99,
      },
      'attachments': [
        {
          'id': 99,
          'filename': 'invite.ics',
          'mime_type': 'text/calendar',
          'size': 120,
        },
      ],
    });
    MailCore.debugInstance = core;
    final state = MailState(core);
    addTearDown(state.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ChangeNotifierProvider<MailState>.value(
            value: state,
            child: const ReaderPane(),
          ),
        ),
      ),
    );
    await state.openMessage(12);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.byType(EventCard), findsOneWidget);
    expect(find.text('Sprint Review'), findsWidgets);
    expect(find.text('Tue, Oct 6, 2026 · 14:00 – 15:00'), findsOneWidget);
    expect(find.text('Room 101'), findsOneWidget);
    expect(find.text('Open in Calendar'), findsOneWidget);
    expect(find.text('Save .ics'), findsOneWidget);
  });

  // One file, one card: the `.ics` the event preview already opens and
  // saves is hidden from the attachment list, so the two cards never
  // stack (and overlap) for the same file.
  testWidgets('event attachment hides from the attachment card', (
    tester,
  ) async {
    final body = {
      'uid': 13,
      'subject': 'Meeting',
      'from': 'mail@example.com',
      'body_text': 'Here is your test calendar',
      'is_html': false,
      'event': {
        'summary': 'Mailclient Development Sync',
        'location': 'Meeting Room Quattro',
        'organizer': 'Cody <cody@example.com>',
        'formatted_time': 'Tue, Oct 6, 2026 · 16:00 – 17:00',
        'is_cancelled': false,
        'attachment_id': 99,
        'save_name': 'invite.ics',
      },
      'attachments': [
        {
          'id': 99,
          'filename': 'invite.ics',
          'file_name': 'invite.ics',
          'mime_type': 'text/calendar',
          'size': 594,
          'size_text': '594 B',
        },
      ],
    };
    MailCore.debugInstance = _OneMessageCore(body);
    final state = MailState(MailCore.instance);
    addTearDown(state.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ChangeNotifierProvider<MailState>.value(
            value: state,
            child: const ReaderPane(),
          ),
        ),
      ),
    );
    await state.openMessage(13);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.byType(EventCard), findsOneWidget);
    // No second card for the same file, and no dangling empty card.
    expect(find.byType(AttachmentCard), findsNothing);
    expect(find.text('1 attachment'), findsNothing);
    expect(find.text('invite.ics'), findsNothing);
    // The event card still offers both actions.
    expect(find.text('Open in Calendar'), findsOneWidget);
  });

  // An invite next to a real file: the card lists only the other file.
  testWidgets('attachment card lists files besides the event invite', (
    tester,
  ) async {
    final body = {
      'uid': 14,
      'subject': 'Meeting',
      'from': 'mail@example.com',
      'body_text': 'x',
      'is_html': false,
      'event': {
        'summary': 'Sync',
        'formatted_time': 'Tue, Oct 6, 2026 · 16:00 – 17:00',
        'is_cancelled': false,
        'attachment_id': 99,
        'save_name': 'invite.ics',
      },
      'attachments': [
        {
          'id': 99,
          'filename': 'invite.ics',
          'file_name': 'invite.ics',
          'mime_type': 'text/calendar',
          'size': 594,
          'size_text': '594 B',
        },
        {
          'id': 100,
          'filename': 'agenda.pdf',
          'file_name': 'agenda.pdf',
          'mime_type': 'application/pdf',
          'size': 10,
          'size_text': '10 B',
        },
      ],
    };
    MailCore.debugInstance = _OneMessageCore(body);
    final state = MailState(MailCore.instance);
    addTearDown(state.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ChangeNotifierProvider<MailState>.value(
            value: state,
            child: const ReaderPane(),
          ),
        ),
      ),
    );
    await state.openMessage(14);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.byType(EventCard), findsOneWidget);
    expect(find.byType(AttachmentCard), findsOneWidget);
    expect(find.text('agenda.pdf'), findsOneWidget);
    expect(find.text('invite.ics'), findsNothing);
  });

  // The OS open hands the calendar the type it registers, not the mail's
  // header spelling — and never an empty or generic lie.
  test('open mime canonicalizes calendar and falls back by extension', () {
    AttachmentInfo ics(String mime) => AttachmentInfo(
      id: 1,
      filename: 'invite.ics',
      fileName: 'invite.ics',
      mimeType: mime,
      size: 594,
      sizeText: '594 B',
      isInline: false,
    );
    expect(openMimeType(ics('text/calendar')), 'text/calendar');
    expect(openMimeType(ics('application/ics')), 'text/calendar');
    expect(openMimeType(ics('TEXT/X-VCALENDAR')), 'text/calendar');
    expect(
      openMimeType(ics('application/octet-stream')),
      'text/calendar',
    );
    expect(openMimeType(ics('')), 'text/calendar');
    const pdf = AttachmentInfo(
      id: 2,
      filename: 'a.pdf',
      fileName: 'a.pdf',
      mimeType: 'application/octet-stream',
      size: 10,
      sizeText: '10 B',
      isInline: false,
    );
    expect(openMimeType(pdf), 'application/pdf');
    const unknown = AttachmentInfo(
      id: 3,
      filename: 'blob.dat',
      fileName: 'blob.dat',
      mimeType: 'application/octet-stream',
      size: 3,
      sizeText: '3 B',
      isInline: false,
    );
    expect(openMimeType(unknown), '*/*');
  });
}
