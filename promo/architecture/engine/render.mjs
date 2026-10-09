#!/usr/bin/env bun
// explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
//
// Renders the deck into an MP4: a headless Chromium steps through the timeline
// frame by frame (window.seek), ffmpeg encodes the screenshots, and the narration
// is muxed in. Subtitles are part of the page, so they are baked into the picture.
//
//   bun engine/render.mjs [--fps 30] [--workers 4] [--from 0] [--to <end>]
//                          [--nosubs] [--out build/<name>.mp4]
//   bun engine/render.mjs --stills 3,12.5,40   PNG snapshots only, for a quick look
//   bun engine/render.mjs --check              one still per scene (fully built up) plus
//                                               layout warnings: off-stage, overflowing text,
//                                               content under the subtitles or the avatar

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BUILD = path.join(ROOT, 'build');
const FFMPEG = process.env.FFMPEG || 'ffmpeg';

const { values: opt } = parseArgs({
  options: {
    fps: { type: 'string', default: '30' },
    workers: { type: 'string', default: String(Math.max(1, Math.min(6, Math.floor(os.cpus().length / 2)))) },
    from: { type: 'string' },
    to: { type: 'string' },
    nosubs: { type: 'boolean', default: false },
    out: { type: 'string' },
    stills: { type: 'string' },
    check: { type: 'boolean', default: false },
    crf: { type: 'string', default: '18' },
  },
});

if (!fs.existsSync(path.join(BUILD, 'timeline.js'))) {
  console.error('No build/timeline.js yet — run `bun run voice` first.');
  process.exit(1);
}

const NAME = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')).name?.replace(/-(video|presentation)$/, '') || 'video';
const url = `${pathToFileURL(path.join(ROOT, 'deck', 'index.html')).href}?render${opt.nosubs ? '&nosubs' : ''}`;

async function openPage(browser) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  page.on('console', (m) => m.type() === 'warning' || m.type() === 'error' ? console.warn(`  [deck] ${m.text()}`) : null);
  page.on('pageerror', (e) => console.error(`  [deck] ${e.message}`));
  await page.goto(url);
  const info = await page.evaluate(() => window.deckReady);
  return { page, info };
}

/** Runs in the page: what in this scene sits where it should not. */
function checkLayout(sceneId) {
  const scene = document.querySelector(`.scene[data-scene="${sceneId}"]`);
  if (!scene) return [`no <section data-scene="${sceneId}"> in the deck`];
  const label = (el) => {
    const text = (el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 40);
    return `<${el.tagName.toLowerCase()}${el.className && typeof el.className === 'string' ? ` class="${el.className}"` : ''}>${text ? ` "${text}"` : ''}`;
  };
  const visible = (el) => {
    for (let n = el; n && n !== scene; n = n.parentElement) if (parseFloat(getComputedStyle(n).opacity) < 0.05) return false;
    return true;
  };
  const hasText = (el) => [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
  const subsTop = 1080 - 58 - 110;
  const av = document.querySelector('#avatar').getBoundingClientRect();
  const avatarBox = { left: av.left + av.width * 0.15, right: av.right - av.width * 0.15, top: av.top + av.height * 0.15, bottom: av.bottom - av.height * 0.1 };
  const issues = new Set();
  for (const el of scene.querySelectorAll('*')) {
    if (!visible(el)) continue;
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height || r.width >= 1900) continue;
    const leaf = hasText(el) || el.matches('[data-fx], .card, .term, .node, .pill, .step, .fact, .msg');
    if (!leaf) continue;
    if (r.left < -2 || r.right > 1922 || r.top < -2) issues.add(`off stage: ${label(el)}`);
    const cs = getComputedStyle(el);
    if (el.scrollWidth > el.clientWidth + 2 && (cs.whiteSpace !== 'normal' || cs.overflowX !== 'visible')) issues.add(`text overflows its box: ${label(el)}`);
    if (hasText(el) && r.bottom > subsTop && r.right > 300 && r.left < 1620) issues.add(`under the subtitle area: ${label(el)}`);
    const ox = Math.min(r.right, avatarBox.right) - Math.max(r.left, avatarBox.left);
    const oy = Math.min(r.bottom, avatarBox.bottom) - Math.max(r.top, avatarBox.top);
    if (ox > 20 && oy > 20 && hasText(el)) issues.add(`behind the avatar: ${label(el)}`);
  }
  return [...issues];
}

function run(args, { input } = {}) {
  const proc = spawn(FFMPEG, ['-hide_banner', '-loglevel', 'error', '-y', ...args], { stdio: [input ? 'pipe' : 'ignore', 'inherit', 'inherit'] });
  const done = new Promise((resolve, reject) => {
    proc.on('error', reject);
    proc.on('close', (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited with ${code}`))));
  });
  return { proc, done };
}

function write(stream, buf) {
  return stream.write(buf) ? Promise.resolve() : new Promise((r) => stream.once('drain', r));
}

// Playwright's own Chromium if it was downloaded (`bunx playwright install chromium`),
// otherwise the Edge or Chrome already on the machine, otherwise a system Chromium
// (BROWSER_PATH, or the usual Linux locations).
async function launch() {
  const paths = [process.env.BROWSER_PATH, '/usr/bin/chromium', '/usr/bin/chromium-browser', '/snap/bin/chromium']
    .filter((p) => p && fs.existsSync(p));
  const tries = [
    ...(process.env.BROWSER_CHANNEL ? [{ channel: process.env.BROWSER_CHANNEL }] : []),
    {}, { channel: 'msedge' }, { channel: 'chrome' },
    ...paths.map((executablePath) => ({ executablePath })),
  ];
  for (const opts of tries) {
    try {
      return await chromium.launch(opts);
    } catch (e) {
      if (opts === tries.at(-1)) throw e;
    }
  }
}

const browser = await launch();
try {
  if (opt.check) {
    const { page } = await openPage(browser);
    const dir = path.join(BUILD, 'stills');
    fs.mkdirSync(dir, { recursive: true });
    const scenes = await page.evaluate(() => window.TIMELINE.scenes.map((s) => ({ id: s.id, at: s.start + s.duration - 0.6 })));
    let problems = 0;
    for (const [i, scene] of scenes.entries()) {
      await page.evaluate((t) => window.seek(t), scene.at);
      const file = path.join(dir, `${String(i + 1).padStart(2, '0')}-${scene.id}.png`);
      await page.screenshot({ path: file });
      const issues = await page.evaluate(checkLayout, scene.id);
      problems += issues.length;
      console.log(`${issues.length ? '!' : '✓'} ${scene.id.padEnd(14)} ${path.relative(process.cwd(), file)}`);
      for (const issue of issues) console.log(`    ${issue}`);
    }
    await browser.close();
    console.log(problems ? `${problems} layout warning(s) — look at the stills.` : 'No layout warnings. Still look at the stills.');
    process.exit(0);
  }

  if (opt.stills) {
    const { page } = await openPage(browser);
    const dir = path.join(BUILD, 'stills');
    fs.mkdirSync(dir, { recursive: true });
    for (const t of opt.stills.split(',').map(Number)) {
      await page.evaluate((t) => window.seek(t), t);
      const file = path.join(dir, `t${t.toFixed(2).padStart(7, '0')}.png`);
      await page.screenshot({ path: file });
      console.log(path.relative(process.cwd(), file));
    }
    await browser.close();
    process.exit(0);
  }

  const fps = Number(opt.fps);
  const probe = await openPage(browser);
  const duration = probe.info.duration;
  await probe.page.close();

  const from = Number(opt.from ?? 0);
  const to = Math.min(duration, Number(opt.to ?? duration));
  const first = Math.round(from * fps);
  const total = Math.round(to * fps) - first;
  const workers = Math.max(1, Math.min(Number(opt.workers), Math.ceil(total / fps)));
  const per = Math.ceil(total / workers);
  const tmp = fs.mkdtempSync(path.join(BUILD, 'render-'));
  console.log(`Rendering ${total} frames (${(total / fps).toFixed(1)} s at ${fps} fps) with ${workers} worker(s) …`);

  let doneFrames = 0;
  const started = Date.now();
  const tick = setInterval(() => {
    const rate = doneFrames / ((Date.now() - started) / 1000);
    const left = rate ? Math.round((total - doneFrames) / rate) : '?';
    process.stdout.write(`\r  ${doneFrames}/${total} frames · ${rate.toFixed(1)} fps · ~${left}s left   `);
  }, 1000);

  const segments = await Promise.all(Array.from({ length: workers }, async (_, w) => {
    const a = first + w * per;
    const b = Math.min(first + total, a + per);
    const file = path.join(tmp, `seg${w}.mp4`);
    if (b <= a) return null;
    const { page } = await openPage(browser);
    const enc = run([
      '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'mjpeg', '-i', '-',
      '-c:v', 'libx264', '-preset', 'medium', '-crf', opt.crf, '-pix_fmt', 'yuv420p', '-r', String(fps), file,
    ], { input: true });
    for (let f = a; f < b; f++) {
      await page.evaluate((t) => window.seek(t), f / fps);
      await write(enc.proc.stdin, await page.screenshot({ type: 'jpeg', quality: 94 }));
      doneFrames++;
    }
    enc.proc.stdin.end();
    await enc.done;
    await page.close();
    return file;
  }));
  clearInterval(tick);
  process.stdout.write('\n');

  const list = path.join(tmp, 'list.txt');
  fs.writeFileSync(list, segments.filter(Boolean).map((f) => `file '${f.replace(/\\/g, '/')}'`).join('\n'));
  const out = path.resolve(opt.out ?? path.join(BUILD, `${NAME}${opt.nosubs ? '-nosubs' : ''}.mp4`));
  await run([
    '-f', 'concat', '-safe', '0', '-i', list,
    '-ss', String(from), '-t', String(to - from), '-i', path.join(BUILD, 'narration.wav'),
    '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-shortest', '-movflags', '+faststart', out,
  ]).done;
  fs.rmSync(tmp, { recursive: true, force: true });
  console.log(`Done in ${Math.round((Date.now() - started) / 1000)} s → ${path.relative(process.cwd(), out)}`);
} finally {
  await browser.close();
}
