// The narration, one entry per scene. Scene ids match <section data-scene="…"> in
// deck/index.html, in the same order.
//
// `say` is spoken and shown as subtitles. A `[cue]` in front of a word marks the moment
// that word is spoken; deck elements with data-cue="cue" animate in right then. Markers
// are never spoken nor shown. `hold` (seconds) keeps a scene on screen longer.
//
// `pronounce` swaps words the voice gets wrong for how they should sound; the subtitles
// keep the written form. Keys match whole words, punctuation aside.

// kokoro: local and offline once set up (the default). Voices: af_heart, af_bella, am_michael,
// bf_emma, … (a = US, b = UK English). edge: Microsoft's online voices, e.g.
// { engine: 'edge', name: 'en-US-AvaMultilingualNeural', rate: '+4%' } — sends the text to Microsoft.
export const voice = {
  engine: 'kokoro',
  name: 'af_heart',
  speed: 1.0,
};

export const pronounce = {
  mailclient: 'mail client',
  IMAP: 'eye map',
  SMTP: 'S M T P',
  QML: 'Q M L',
  SQLite: 'sequel lite',
  Qt: 'cute',
  Omarchy: 'oh mar kee',
  FTS: 'F T S',
  Kotlin: 'kot lin',
};

// Optional: silences around the narration, in seconds.
// export const timing = { leadIn: 0.55, tail: 0.95, firstLeadIn: 1.4, lastTail: 3.2 };

export const scenes = [
  {
    id: 'intro',
    say: "[hi]Hi everyone! I'm the mailclient team's fluffy stand-in. [title]Meet mailclient: your inbox, minus the chaos. [apps]A desktop mail app for Omarchy Linux and Windows, with a native Android companion.",
  },
  {
    id: 'problem',
    say: 'Mail piles up fast. [a]Work and personal accounts scattered around, [b]clients that stall without network, [q1]which folder held that invoice, [q2]is this link safe to open, [q3]can I read everything offline?',
  },
  {
    id: 'core',
    say: '[core]Underneath there is one Rust core. [imap]It speaks IMAP and SMTP, caches everything in SQLite on your machine, [qt]and feeds two frontends: a Qt and QML desktop app for Linux and Windows, [android]and native Kotlin and Compose on Android. Same behaviour, every platform.',
  },
  {
    id: 'workspace',
    say: '[side]The workspace stays calm: sidebar, [list]message list, [read]and reader. [narrow]Three panes collapse to one on narrow screens, desktop or phone, [unread]with unread dots, stars and attachments at a glance.',
  },
  {
    id: 'search',
    say: '[type]Type three letters and full text search answers instantly, [hits]over subjects, senders and bodies, even offline. [syntax]Narrow it with is colon unread, has colon attachment, or after colon a date. [backfill]When the local cache runs thin, it quietly backfills from the server.',
  },
  {
    id: 'reader',
    say: '[read]Reading is protective: cleaned HTML with remote images blocked, [links]and a link check before anything opens. [cards]Meeting invites, delivery reports, read receipts, attached mail and contact cards all become simple cards, [attached]with attachments opened, saved or saved all.',
  },
  {
    id: 'compose',
    say: '[write]Writing is confident: rich text composer, drafts, attachments, read receipts and delivery confirmations. [later]Send later schedules the mail, [queue]and the outbox survives a closed window. [undo]Delete, archive and move stay undoable.',
  },
  {
    id: 'android',
    say: '[native]On Android, the native app runs the same core. [push]Opt in to push and new mail arrives in seconds, [quiet]while quiet hours keep nights silent. [actions]Notifications can be marked read, archived or answered right from the shade, [lock]and an app lock keeps the inbox private.',
  },
  {
    id: 'start',
    say: '[cmd]Build the Qt bundle with build dot sh dash dash qt, [install]install it locally, [dev]and hack on the live interface with dev dot sh. [android]On Android, build the signed APK with build dot sh dash dash android, or run it with the Android dev script.',
  },
  {
    id: 'outro',
    say: '[free]It is free and open source: Rust plus Qt plus Compose plus SQLite, offline first, on desktop and Android. [bye]Inbox, minus the chaos. Happy mailing!',
  },
];
