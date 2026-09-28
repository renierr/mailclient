#!/usr/bin/env bun
// explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
//
// Turns script.mjs into everything the deck needs to play in sync:
//
//   build/audio/<scene>-<hash>.*     one narration clip per scene (cached by text + voice)
//   build/narration.wav              all clips laid out on one timeline
//   build/timeline.js                scene times, cue times, subtitle cues, mouth envelope
//   build/subtitles.srt / .vtt       the same subtitles as sidecar files
//
// Voice: Kokoro (local neural TTS, offline once set up) by default, or Microsoft Edge's
// online voices (voice.engine: 'edge'). Python, packages and models live in one
// per-user cache shared by all videos — see "shared python + models" below.

import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as script from '../script.mjs';

const ENGINE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(ENGINE, '..');
const BUILD = path.join(ROOT, 'build');
const AUDIO = path.join(BUILD, 'audio');
const FFMPEG = process.env.FFMPEG || 'ffmpeg';

const { scenes, voice } = script;
const pronounce = script.pronounce ?? {};
const RATE = 24000; // Hz, mono — what both engines deliver
const FPS = 30;     // resolution of the mouth envelope
// Silences in seconds; script.mjs may override any of them with `export const timing`.
const { leadIn: LEAD_IN, tail: TAIL, firstLeadIn: FIRST_LEAD_IN, lastTail: LAST_TAIL } = {
  leadIn: 0.55,     // before each scene's narration
  tail: 0.95,       // after it, before the next scene starts
  firstLeadIn: 1.4, // the first scene: a moment for the avatar to pop in
  lastTail: 3.2,    // the last scene lingers on the goodbye
  ...script.timing,
};

const MARKER = /^\[([\w-]+)\]/;
const PUNCT = /^([^\p{L}\p{N}]*)(.*?)([^\p{L}\p{N}]*)$/u;

fs.mkdirSync(AUDIO, { recursive: true });

// ---------------------------------------------------------------- script → words

function parseScene(scene) {
  const words = [];
  const cueAt = {}; // cue id → index of the display word it precedes
  let spoken = '';
  for (let token of scene.say.split(/\s+/).filter(Boolean)) {
    let m;
    while ((m = MARKER.exec(token))) {
      cueAt[m[1]] = words.length;
      token = token.slice(m[0].length);
    }
    if (!token) continue;
    const [, pre, core, post] = PUNCT.exec(token);
    const say = pre + (pronounce[core] ?? core) + post;
    if (spoken) spoken += ' ';
    words.push({ text: token, spokenStart: spoken.length, spokenEnd: spoken.length + say.length });
    spoken += say;
  }
  return { ...scene, words, cueAt, spoken };
}

// ---------------------------------------------------------------- voice engine

// Kokoro is local and the default. Edge is picked explicitly, or recognised by an
// Edge-style voice name ("en-US-…") so scripts written before Kokoro keep their voice.
const ENGINE_NAME = voice.engine ?? (/^[a-z]{2,3}-[A-Z]{2}-/.test(voice.name ?? '') ? 'edge' : 'kokoro');
const SETTINGS = {
  kokoro: { name: voice.name ?? 'af_heart', speed: voice.speed ?? 1 },
  edge: { name: voice.name ?? 'en-US-AvaMultilingualNeural', rate: voice.rate ?? '+0%', pitch: voice.pitch ?? '+0Hz' },
}[ENGINE_NAME];
if (!SETTINGS) throw new Error(`Unknown voice.engine "${ENGINE_NAME}" in script.mjs — use "kokoro" or "edge".`);
if (ENGINE_NAME === 'kokoro' && !/^[ab]/.test(SETTINGS.name)) {
  throw new Error(`Kokoro voice "${SETTINGS.name}": only English voices (a… US, b… UK) are set up, e.g. af_heart, bf_emma.`);
}

// ---------------------------------------------------------------- shared python + models
//
// One per-user cache, shared by every video on the machine, so the Python runtime,
// packages and voice models are set up once and not per project:
//   <cache>/python    uv-managed Python runtimes      <cache>/uv   uv's package cache
//   <cache>/venv-<engine>  one environment per engine  <cache>/hf   Kokoro model + voices
// EXPLAINER_VIDEO_CACHE overrides the location.

const CACHE = process.env.EXPLAINER_VIDEO_CACHE || (process.platform === 'win32'
  ? path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local'), 'explainer-video')
  : process.platform === 'darwin'
    ? path.join(os.homedir(), 'Library', 'Caches', 'explainer-video')
    : path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache'), 'explainer-video'));
const HF_HOME = path.join(CACHE, 'hf');

const PACKAGES = {
  kokoro: {
    probe: 'import kokoro, en_core_web_sm',
    install: [
      'kokoro>=0.9.4',
      'en_core_web_sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl',
    ],
    // CPU-only PyTorch: a fraction of the size of the default CUDA build.
    index: ['--extra-index-url', 'https://download.pytorch.org/whl/cpu'],
  },
  edge: { probe: 'import edge_tts', install: ['edge-tts'], index: [] },
};

const ok = (cmd, args, env) => spawnSync(cmd, args, { env, encoding: 'utf8' }).status === 0;

function venvPython(venv) {
  for (const rel of ['Scripts/python.exe', 'bin/python.exe', 'bin/python']) {
    const p = path.join(venv, rel);
    if (fs.existsSync(p)) return p;
  }
  return null;
}

/** A native CPython 3.10–3.12 (Kokoro's range). MSYS2/MinGW Pythons cannot install PyTorch. */
function basePython() {
  const candidates = [process.env.PYTHON, ['py', '-3.12'], ['py', '-3.11'], ['py', '-3.10'], 'python3.12', 'python3.11', 'python3', 'python'];
  const check = 'import sys, sysconfig; ok = (3, 10) <= sys.version_info[:2] <= (3, 12) and "mingw" not in sysconfig.get_platform(); sys.exit(0 if ok else 1)';
  for (const c of candidates.filter(Boolean)) {
    const [cmd, ...pre] = Array.isArray(c) ? c : [c];
    if (ok(cmd, [...pre, '-c', check])) return [cmd, ...pre];
  }
  return null;
}

function ensurePython(engine) {
  const venv = path.join(CACHE, `venv-${engine}`);
  const pkg = PACKAGES[engine];
  let py = venvPython(venv);
  if (py && ok(py, ['-c', pkg.probe])) return py;

  fs.mkdirSync(CACHE, { recursive: true });
  const env = { ...process.env, UV_PYTHON_INSTALL_DIR: path.join(CACHE, 'python'), UV_CACHE_DIR: path.join(CACHE, 'uv') };
  console.log(`Setting up the ${engine} voice in ${venv} (once per machine) …`);
  if (ok('uv', ['--version'])) {
    // Offline first: whatever an earlier setup downloaded is in the uv cache already.
    const uv = (args) => ok('uv', [...args, '--offline'], env) || execFileSync('uv', args, { stdio: 'inherit', env });
    uv(['venv', '--quiet', '--allow-existing', '--python', '3.12', venv]);
    py = venvPython(venv);
    uv(['pip', 'install', '--quiet', '--python', py, ...pkg.install, ...pkg.index, '--index-strategy', 'unsafe-best-match']);
  } else {
    const base = basePython();
    if (!base) {
      throw new Error('The voice needs uv (recommended: https://docs.astral.sh/uv/) or a native Python 3.10–3.12 on the PATH.');
    }
    if (!py) execFileSync(base[0], [...base.slice(1), '-m', 'venv', venv], { stdio: 'inherit' });
    py = venvPython(venv);
    execFileSync(py, ['-m', 'pip', 'install', '--quiet', ...pkg.install, ...pkg.index], { stdio: 'inherit' });
  }
  return py;
}

/** True when the Kokoro model and this voice are already cached: then no network is touched at all. */
function kokoroCached(name) {
  const snapshots = path.join(HF_HOME, 'hub', 'models--hexgrad--Kokoro-82M', 'snapshots');
  if (!fs.existsSync(snapshots)) return false;
  return fs.readdirSync(snapshots).some((snap) =>
    fs.existsSync(path.join(snapshots, snap, 'kokoro-v1_0.pth')) && fs.existsSync(path.join(snapshots, snap, 'voices', `${name}.pt`)));
}

function synthesise(parsed) {
  const ext = ENGINE_NAME === 'edge' ? 'mp3' : 'wav';
  const todo = [];
  for (const s of parsed) {
    const hash = createHash('sha1').update(JSON.stringify([ENGINE_NAME, SETTINGS, s.spoken])).digest('hex').slice(0, 10);
    s.audio = path.join(AUDIO, `${s.id}-${hash}.${ext}`);
    s.wordsFile = path.join(AUDIO, `${s.id}-${hash}.json`);
    if (!fs.existsSync(s.audio) || !fs.existsSync(s.wordsFile)) {
      todo.push({ id: s.id, text: s.spoken, audio: s.audio, words: s.wordsFile });
    }
  }
  if (todo.length) {
    const py = ensurePython(ENGINE_NAME);
    const job = path.join(AUDIO, 'job.json');
    fs.writeFileSync(job, JSON.stringify({ engine: ENGINE_NAME, settings: SETTINGS, scenes: todo }));
    const env = { ...process.env, HF_HOME, PYTHONIOENCODING: 'utf-8', PYTHONWARNINGS: 'ignore' };
    if (ENGINE_NAME === 'kokoro' && kokoroCached(SETTINGS.name)) env.HF_HUB_OFFLINE = '1';
    const where = ENGINE_NAME === 'edge' ? 'online, Microsoft Edge' : env.HF_HUB_OFFLINE ? 'local, offline' : 'local; downloading the model once';
    console.log(`Synthesising ${todo.length} scene(s) with ${ENGINE_NAME} ${SETTINGS.name} (${where}) …`);
    execFileSync(py, [path.join(ENGINE, 'tts.py'), job], { stdio: 'inherit', env });
  } else {
    console.log('Narration unchanged, reusing cached audio.');
  }
  for (const s of parsed) s.boundaries = JSON.parse(fs.readFileSync(s.wordsFile, 'utf8'));
}

function decode(file) {
  const buf = execFileSync(FFMPEG, ['-v', 'error', '-i', file, '-f', 's16le', '-ac', '1', '-ar', String(RATE), '-'], {
    maxBuffer: 1 << 28,
  });
  return new Int16Array(buf.buffer, buf.byteOffset, buf.length / 2);
}

// ---------------------------------------------------------------- timing

/** Gives every display word a start/end (seconds into its clip) from the TTS word boundaries. */
function alignWords(s, clipDuration) {
  const lower = s.spoken.toLowerCase();
  let cursor = 0;
  const anchored = [];
  for (const b of s.boundaries) {
    const at = lower.indexOf(b.text.toLowerCase(), cursor);
    if (at < 0) continue;
    anchored.push({ ...b, offset: at });
    cursor = at + b.text.length;
  }
  for (const w of s.words) {
    const inside = anchored.filter((b) => b.offset >= w.spokenStart && b.offset < w.spokenEnd);
    if (inside.length) {
      w.start = inside[0].start;
      w.end = inside.at(-1).end;
    } else {
      // No boundary for this word (rare) — interpolate by character position.
      const frac = w.spokenStart / s.spoken.length;
      w.start = frac * clipDuration;
      w.end = w.start + ((w.spokenEnd - w.spokenStart) / s.spoken.length) * clipDuration;
    }
  }
  for (let i = 1; i < s.words.length; i++) {
    if (s.words[i].start < s.words[i - 1].start) s.words[i].start = s.words[i - 1].end;
  }
}

/** Splits a scene's words into subtitle cues: one sentence each, long ones halved at a natural break. */
function subtitleCues(words) {
  const MAX = 58;
  const len = (ws) => ws.reduce((n, w) => n + w.text.length + 1, -1);
  const split = (ws) => {
    if (len(ws) <= MAX || ws.length < 4) return [ws];
    let best = 1;
    let bestScore = Infinity;
    for (let i = 2; i < ws.length - 1; i++) {
      const left = len(ws.slice(0, i));
      const right = len(ws.slice(i));
      const score = Math.abs(left - right) - (/[,:;]$/.test(ws[i - 1].text) ? 18 : 0);
      if (score < bestScore) [best, bestScore] = [i, score];
    }
    return [...split(ws.slice(0, best)), ...split(ws.slice(best))];
  };
  const cues = [];
  let sentence = [];
  for (const w of words) {
    sentence.push(w);
    if (/[.!?]["']?$/.test(w.text)) {
      cues.push(...split(sentence));
      sentence = [];
    }
  }
  if (sentence.length) cues.push(...split(sentence));
  return cues;
}

// ---------------------------------------------------------------- output helpers

function wav(samples) {
  const header = Buffer.alloc(44);
  const bytes = samples.length * 2;
  header.write('RIFF', 0);
  header.writeUInt32LE(36 + bytes, 4);
  header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(RATE, 24);
  header.writeUInt32LE(RATE * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write('data', 36);
  header.writeUInt32LE(bytes, 40);
  return Buffer.concat([header, Buffer.from(samples.buffer, samples.byteOffset, bytes)]);
}

function stamp(t, sep) {
  const ms = Math.round(t * 1000);
  const h = String(Math.floor(ms / 3600000)).padStart(2, '0');
  const m = String(Math.floor(ms / 60000) % 60).padStart(2, '0');
  const s = String(Math.floor(ms / 1000) % 60).padStart(2, '0');
  return `${h}:${m}:${s}${sep}${String(ms % 1000).padStart(3, '0')}`;
}

// ---------------------------------------------------------------- main

const parsed = scenes.map(parseScene);
synthesise(parsed);

let t = 0;
const clips = [];
const timeline = { fps: FPS, scenes: [], subtitles: [] };

parsed.forEach((s, i) => {
  const pcm = decode(s.audio);
  const clipDuration = pcm.length / RATE;
  alignWords(s, clipDuration);

  const lead = i === 0 ? FIRST_LEAD_IN : LEAD_IN;
  const tail = (i === parsed.length - 1 ? LAST_TAIL : TAIL) + (s.hold ?? 0);
  const start = t;
  const voiceAt = start + lead;
  const duration = lead + clipDuration + tail;
  clips.push({ at: voiceAt, pcm });

  const cues = {};
  for (const [id, index] of Object.entries(s.cueAt)) {
    cues[id] = +(lead + (s.words[index]?.start ?? clipDuration)).toFixed(3);
  }
  timeline.scenes.push({ id: s.id, start: +start.toFixed(3), duration: +duration.toFixed(3), voice: [+voiceAt.toFixed(3), +(voiceAt + clipDuration).toFixed(3)], cues });

  const groups = subtitleCues(s.words);
  groups.forEach((g, gi) => {
    const next = groups[gi + 1];
    const cueStart = voiceAt + g[0].start - 0.08;
    const cueEnd = next ? voiceAt + next[0].start - 0.08 : voiceAt + g.at(-1).end + 0.5;
    timeline.subtitles.push({
      start: +cueStart.toFixed(3),
      end: +cueEnd.toFixed(3),
      words: g.map((w) => [w.text, +(voiceAt + w.start).toFixed(3), +(voiceAt + w.end).toFixed(3)]),
    });
  });
  t += duration;
});

timeline.duration = +t.toFixed(3);

// Lay the clips onto one track.
const track = new Int16Array(Math.ceil(t * RATE));
for (const { at, pcm } of clips) track.set(pcm, Math.round(at * RATE));
fs.writeFileSync(path.join(BUILD, 'narration.wav'), wav(track));

// Mouth envelope: RMS per video frame, normalised so normal speech opens it fully.
const hop = RATE / FPS;
const rms = [];
for (let f = 0; f * hop < track.length; f++) {
  let sum = 0;
  const from = Math.floor(f * hop);
  const to = Math.min(track.length, Math.floor((f + 1) * hop));
  for (let k = from; k < to; k++) sum += track[k] * track[k];
  rms.push(Math.sqrt(sum / Math.max(1, to - from)));
}
const loud = [...rms].filter((v) => v > 200).sort((a, b) => a - b);
const ref = loud[Math.floor(loud.length * 0.9)] || 1;
timeline.mouth = rms.map((v) => Math.min(99, Math.round((v / ref) * 99)));

fs.writeFileSync(
  path.join(BUILD, 'timeline.js'),
  `// Generated by engine/voice.mjs from script.mjs — do not edit.\nwindow.TIMELINE = ${JSON.stringify(timeline)};\n`
);

const srt = [];
const vtt = ['WEBVTT', ''];
timeline.subtitles.forEach((c, i) => {
  const text = c.words.map((w) => w[0]).join(' ');
  srt.push(String(i + 1), `${stamp(c.start, ',')} --> ${stamp(c.end, ',')}`, text, '');
  vtt.push(`${stamp(c.start, '.')} --> ${stamp(c.end, '.')}`, text, '');
});
fs.writeFileSync(path.join(BUILD, 'subtitles.srt'), srt.join('\n'));
fs.writeFileSync(path.join(BUILD, 'subtitles.vtt'), vtt.join('\n'));

console.log(`Timeline: ${timeline.scenes.length} scenes, ${timeline.duration.toFixed(1)} s → build/timeline.js, build/narration.wav`);
for (const s of timeline.scenes) {
  const missing = Object.entries(s.cues).filter(([, v]) => v == null);
  if (missing.length) console.warn(`  ${s.id}: cue(s) without a word: ${missing.map(([k]) => k).join(', ')}`);
}
