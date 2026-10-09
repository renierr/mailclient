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
  mailcore: 'mail core',
  mailapp: 'mail app',
  mailffi: 'mail F F I',
  IMAP: 'eye map',
  SMTP: 'S M T P',
  QML: 'Q M L',
  SQLite: 'sequel lite',
  Qt: 'cute',
  JNI: 'J N I',
  JSON: 'jay son',
  WorkManager: 'work manager',
};

// Optional: silences around the narration, in seconds.
// export const timing = { leadIn: 0.55, tail: 0.95, firstLeadIn: 1.4, lastTail: 3.2 };

export const scenes = [
  {
    id: 'intro',
    say: "[hi]Hi again! I'm the mailclient team's fluffy stand-in. [title]Today: how mailclient is built, who does what, and when.",
  },
  {
    id: 'layers',
    say: '[core]At the bottom sits one Rust core, called mailcore. [adapt]On top of it, two thin adapters. [qt]One feeds the Qt desktop app on Linux and Windows, [android]the other the native Android app, written in Kotlin.',
  },
  {
    id: 'core',
    say: '[brain]The core is the brain. [sync]It talks IMAP and SMTP to your mail servers, [cache]keeps everything in a local SQLite cache, [search]runs the full text search, [rules]and makes every decision both apps share, like what a filter keeps. [noui]It has no user interface at all.',
  },
  {
    id: 'adapters',
    say: '[thin]The adapters are translators, nothing more. [mailapp]mailapp turns core calls into Qt properties and signals. [mailffi]mailffi does the same for Kotlin, through JNI. [json]Lists and records cross as JSON, [plain]ids, flags and bytes as plain values.',
  },
  {
    id: 'frontends',
    say: '[ui]The frontends draw. [qml]QML on the desktop, [compose]Jetpack Compose on Android. [how]They decide how things look: layout, gestures and themes. [same]Features stay the same in both, only the layout may differ.',
  },
  {
    id: 'open',
    say: 'So what happens when you open a folder? [s1]The app asks its adapter, [s2]the core reads the local cache, [s3]the rows come back as JSON, [s4]and the app paints them. That is instant, even offline. [sync]Meanwhile, the app asks for fresh mail.',
  },
  {
    id: 'jobs',
    say: 'Fetching mail is a job, like anything that needs the network. [s1]It is queued onto one background thread, so the window never freezes. [s2]There the core talks to the server and updates the cache. [s3]When it is done, an event tells the app what changed, [s4]and the app re-reads just that. [bg]On Android, WorkManager wakes the same core for background mail checks.',
  },
  {
    id: 'rule',
    say: 'One rule keeps it all together: core first. [s1]A new feature starts in the core, with its tests. [s2]Then the adapters pass it through. [s3]The two user interfaces come last. [dup]If you catch yourself writing the same logic twice, it belongs in the core.',
  },
  {
    id: 'outro',
    say: '[sum]One core, two thin adapters, two frontends. [bye]Easy to reason about. Happy hacking!',
  },
];
