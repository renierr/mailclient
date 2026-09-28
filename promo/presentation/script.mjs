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
};

// Optional: silences around the narration, in seconds.
// export const timing = { leadIn: 0.55, tail: 0.95, firstLeadIn: 1.4, lastTail: 3.2 };

export const scenes = [
  {
    id: 'intro',
    say: "[hi]Hi everyone! I'm the mailclient team's fluffy stand-in. [title]Meet mailclient: your inbox, minus the chaos, a desktop mail app built for Omarchy Linux.",
  },
  {
    id: 'problem',
    say: 'Mail piles up fast. [a]Work and personal accounts scattered around, [b]clients that stall without network, [q1]which folder held that invoice, [q2]is this link safe to open, [q3]can I read everything offline?',
  },
  {
    id: 'core',
    say: '[core]Underneath there is one Rust core. [imap]It speaks IMAP and SMTP, caches everything in SQLite on your machine, [qt]and feeds two apps: a Qt desktop app, [flutter]and Flutter on Linux, Windows and Android.',
  },
  {
    id: 'workspace',
    say: '[side]The workspace stays calm: sidebar, [list]message list, [read]and reader. [narrow]It squeezes from three panes down to one on narrow screens, [unread]with unread pills, stars and attachments at a glance.',
  },
  {
    id: 'search',
    say: '[type]Type three letters and full text search answers instantly, [hits]over subjects, senders and bodies, even offline. [backfill]When the local cache runs thin, it quietly backfills from the server.',
  },
  {
    id: 'safe',
    say: '[read]Reading is protective: cleaned HTML with remote images blocked, [links]and a link check before anything opens. [write]Writing is confident: rich text composer, drafts, attachments, and queued sending that survives a closed window.',
  },
  {
    id: 'start',
    say: 'Getting started is simple. [cmd]Build the Qt bundle with build dot sh dash dash qt, [install]install it locally, [dev]and hack on the live interface with dev dot sh.',
  },
  {
    id: 'outro',
    say: '[free]It is free and open source, Rust plus Qt plus SQLite, offline first. [bye]Inbox, minus the chaos. Happy mailing!',
  },
];
