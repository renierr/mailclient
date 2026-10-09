#!/usr/bin/env bun
// explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
//
// Opens the deck in your default browser for live playback (narration + subtitles).
// Keys: Space pause · ← → scenes · C subtitles · F fullscreen. `?t=42` or `#demo` jumps.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
if (!fs.existsSync(path.join(ROOT, 'build', 'timeline.js'))) {
  console.error('No build/timeline.js yet — run `bun run voice` first.');
  process.exit(1);
}
const url = pathToFileURL(path.join(ROOT, 'deck', 'index.html')).href;
console.log(url);
const [cmd, args] =
  process.platform === 'win32' ? ['cmd', ['/c', 'start', '""', url]] :
  process.platform === 'darwin' ? ['open', [url]] : ['xdg-open', [url]];
spawn(cmd, args, { stdio: 'ignore', detached: true }).unref();
