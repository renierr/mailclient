/* mailclient promo — deterministic p5.js timeline.
 * window.renderFrame(f) draws frame f in [0,1800). 1920x1080, 30fps, 60s.
 * No randomness, no wall-clock: every visual is a pure function of f. */
const W = 1920, H = 1080, FPS = 30, SCENE_LEN = 300, NSC = 6, TOTAL = 1800;
const INK = [255, 255, 255], SUB = [201, 212, 234], DIM = [138, 148, 173];
const CYAN = [125, 211, 252], BLUE = [56, 189, 248], MINT = [110, 231, 183];
const VIOLET = [167, 139, 250], AMBER = [251, 191, 36];
const NAMES = ['Meet mailclient', 'Effortless setup', 'A calm workspace', 'Blazing search', 'Compose and read', 'Get mailclient'];
const CAPS = [
  'Meet mailclient — email that respects your attention',
  'Three accounts, one calm place — setup takes a minute',
  'Sidebar, list, reader — responsive down to narrow screens',
  'Full-text search across everything — even offline',
  'Expressive writing, protective reading — by default',
  'mailclient — inbox, minus the chaos',
];

const clamp01 = v => v < 0 ? 0 : v > 1 ? 1 : v;
const easeOutCubic = t => 1 - Math.pow(1 - t, 3);
const easeInOut = t => t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
const easeOutBack = t => { const c = 1.70158; return 1 + (c + 1) * Math.pow(t - 1, 3) + c * Math.pow(t - 1, 2); };
const P = (lt, a, b) => clamp01((lt - a) / (b - a));
const rise = (lt, delay, dur, dist) => (1 - easeOutCubic(P(lt, delay, delay + dur))) * dist;

function setup() {
  createCanvas(W, H);
  pixelDensity(1);
  noLoop();
  textFont('Noto Sans');
}

function vignette() {
  const ctx = drawingContext;
  const g = ctx.createRadialGradient(W / 2, H * 0.44, 320, W / 2, H * 0.44, 1150);
  g.addColorStop(0, 'rgba(0,0,0,0)');
  g.addColorStop(1, 'rgba(2,4,12,0.5)');
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, W, H);
}

function label(txt, x, y, size, col, align, bold) {
  noStroke();
  fill(col[0], col[1], col[2], col.length > 3 ? col[3] : 255);
  textStyle(bold ? BOLD : NORMAL);
  textSize(size);
  textAlign(align === undefined ? CENTER : align, CENTER);
  try { drawingContext.letterSpacing = '0px'; } catch (e) {}
  text(txt, x, y);
}

function eyebrow(txt, x, y, dyOff) {
  noStroke();
  fill(CYAN[0], CYAN[1], CYAN[2]);
  textStyle(BOLD); textSize(30); textAlign(x === W / 2 ? CENTER : LEFT, CENTER);
  try { drawingContext.letterSpacing = '6px'; } catch (e) {}
  text(txt, x, y + dyOff);
  try { drawingContext.letterSpacing = '0px'; } catch (e) {}
}

function card(x, y, w, h, r) {
  noStroke();
  fill(255, 255, 255, 15);
  rect(x, y, w, h, r === undefined ? 22 : r);
  noFill();
  stroke(255, 255, 255, 34);
  strokeWeight(2);
  rect(x, y, w, h, r === undefined ? 22 : r);
  noStroke();
}

function bar(x, y, w, h, col, alpha) {
  noStroke();
  fill(col[0], col[1], col[2], alpha === undefined ? 255 : alpha);
  rect(x, y, w, h, Math.min(9, h / 2));
}

function ghostNum(n) {
  noFill();
  stroke(255, 255, 255, 13);
  strokeWeight(3);
  textStyle(BOLD); textSize(470); textAlign(RIGHT, CENTER);
  text(n, 1900, 560);
  noStroke();
}

function chrome(f, sc) {
  label('mailclient', 80, 74, 34, CYAN, LEFT, true);
  label('0' + (sc + 1) + ' / 06 · ' + NAMES[sc], W - 80, 74, 28, DIM, RIGHT, false);
  const pf = f / (TOTAL - 1);
  bar(0, 0, W, 5, [255, 255, 255], 26);
  bar(0, 0, W * pf, 5, BLUE, 255);
  textStyle(NORMAL); textSize(30); textAlign(CENTER, CENTER);
  const tw = textWidth(CAPS[sc]);
  noStroke(); fill(4, 8, 20, 150);
  rect(W / 2 - tw / 2 - 42, 912, tw + 84, 66, 33);
  noFill(); stroke(255, 255, 255, 30); strokeWeight(2);
  rect(W / 2 - tw / 2 - 42, 912, tw + 84, 66, 33);
  noStroke();
  label(CAPS[sc], W / 2, 947, 30, INK, CENTER, false);
}

/* ---------------- scenes ---------------- */

function scene1(lt) {
  ghostNum('01');
  eyebrow('A DESKTOP MAIL CLIENT FOR LINUX', W / 2, 208, rise(lt, 0, 40, 30));
  label('Inbox, minus', W / 2, 330 + rise(lt, 8, 44, 44), 104, INK, CENTER, true);
  label('the chaos.', W / 2, 442 + rise(lt, 14, 44, 44), 104, INK, CENTER, true);
  const uw = 300 + 240 * easeOutCubic(P(lt, 40, 90));
  bar(W / 2 - uw / 2, 512, uw, 7, BLUE, 255);
  label('Rust core  ·  SQLite cache  ·  Qt Quick interface', W / 2, 566 + rise(lt, 40, 40, 26), 36, SUB, CENTER, false);
  // mini 3-pane mockup, staggered rise
  const panes = [[470, 300], [790, 340], [1130, 300]];
  const dy = [rise(lt, 70, 50, 120), rise(lt, 84, 50, 120), rise(lt, 98, 50, 120)];
  const al = [P(lt, 70, 110), P(lt, 84, 124), P(lt, 98, 138)];
  drawingContext.save();
  drawingContext.globalAlpha = Math.min(al[0], al[1], al[2]) < 1 ? al[1] : 1;
  for (let i = 0; i < 3; i++) {
    drawingContext.save();
    drawingContext.globalAlpha = al[i];
    card(panes[i][0], 648 + dy[i], panes[i][1], 218);
    bar(panes[i][0] + 30, 688 + dy[i], panes[i][1] - 60, 22, i === 1 ? BLUE : [255, 255, 255], i === 1 ? 255 : 70);
    bar(panes[i][0] + 30, 728 + dy[i], (panes[i][1] - 60) * 0.72, 18, [255, 255, 255], 42);
    bar(panes[i][0] + 30, 760 + dy[i], (panes[i][1] - 60) * 0.55, 18, [255, 255, 255], 42);
    drawingContext.restore();
  }
  drawingContext.restore();
  bar(505, 688 + dy[0], 15, 15, CYAN, 255 * al[0]);
  bar(825, 728 + dy[1], 15, 15, CYAN, 255 * al[1]);
}

function scene2(lt) {
  ghostNum('02');
  eyebrow('MULTI-ACCOUNT IMAP + SMTP', W / 2, 150, rise(lt, 0, 40, 30));
  label('Set up in seconds.', W / 2, 268 + rise(lt, 8, 44, 44), 100, INK, CENTER, true);
  const steps = [
    'Add an account — host, port, encryption',
    'Folders map themselves — Inbox, Sent, Drafts, Archive',
    'Passwords live in the OS keyring — never in the database',
  ];
  for (let i = 0; i < 3; i++) {
    const d = 30 + i * 26, r = rise(lt, d, 40, 34), a = P(lt, d, d + 30);
    drawingContext.save(); drawingContext.globalAlpha = a;
    bar(300, 408 + i * 78 + r, 64, 64, [BLUE[0], BLUE[1], BLUE[2]], 70);
    label(String(i + 1), 332, 442 + i * 78 + r, 34, CYAN, CENTER, true);
    label(steps[i], 392, 442 + i * 78 + r, 37, INK, LEFT, false);
    drawingContext.restore();
  }
  // account cards pop with overshoot
  const names2 = ['you@example.com', 'work account', 'side project'];
  const cols = [BLUE, VIOLET, MINT];
  for (let i = 0; i < 3; i++) {
    const t = easeOutBack(P(lt, 130 + i * 30, 170 + i * 30));
    const s = Math.max(0.01, t), cx = 340 + i * 426 + 193;
    drawingContext.save();
    drawingContext.translate(cx, 770);
    drawingContext.scale(s, s);
    drawingContext.translate(-cx, -770);
    drawingContext.globalAlpha = P(lt, 130 + i * 30, 150 + i * 30);
    card(340 + i * 426, 716, 386, 108);
    bar(340 + i * 426, 716, 386, 108, cols[i], 46);
    label(names2[i], 340 + i * 426 + 193, 771, 33, INK, CENTER, true);
    drawingContext.restore();
  }
}

function scene3(lt) {
  ghostNum('03');
  eyebrow('SIDEBAR  ·  LIST  ·  READER', W / 2, 118, rise(lt, 0, 40, 30));
  label('Three panes. Zero noise.', W / 2, 226 + rise(lt, 8, 44, 44), 92, INK, CENTER, true);
  const a = P(lt, 30, 80), dy = rise(lt, 30, 60, 90);
  drawingContext.save(); drawingContext.globalAlpha = a;
  drawingContext.translate(0, dy);
  // sidebar
  card(150, 330, 320, 520);
  label('Folders', 182, 366, 28, CYAN, LEFT, true);
  const folders = [['Inbox', 1], ['Sent', 0], ['Drafts', 0], ['Archive', 0], ['Trash', 0]];
  for (let i = 0; i < 5; i++) {
    if (i === 0) bar(150, 396 + i * 62, 320, 56, BLUE, 60);
    label(folders[i][0], 182, 426 + i * 62, 29, i === 0 ? INK : SUB, LEFT, i === 0);
    if (i === 0) { bar(392, 410, 46, 30, BLUE, 230); label('12', 415, 426, 24, INK, CENTER, true); }
    if (i === 2) { bar(392, 534, 38, 30, VIOLET, 200); label('3', 411, 550, 24, INK, CENTER, true); }
  }
  // message list with sweeping selection
  card(494, 330, 640, 520);
  const idx = Math.floor(lt / 45) % 5, prev = (idx + 4) % 5;
  const hy = 352 + prev * 96 + ((352 + idx * 96) - (352 + prev * 96)) * easeInOut(P(lt % 45, 0, 12));
  bar(494, hy, 640, 88, BLUE, 66);
  bar(494, hy, 7, 88, BLUE, 255);
  const subjects = ['Quarterly invoice attached', 'Re: launch plan Friday', 'Photos from the cabin trip', 'Your receipt from Example', 'Welcome to the beta group'];
  for (let i = 0; i < 5; i++) {
    if (i !== 4) bar(526, 372 + i * 96, 15, 15, CYAN, i < 2 ? 255 : 90);
    label(subjects[i], 556, 380 + i * 96, 28, i === 4 ? DIM : INK, LEFT, i < 2);
    bar(556, 404 + i * 96, 300 - i * 22, 15, [255, 255, 255], 44);
  }
  // reader
  card(1158, 330, 612, 520);
  bar(1194, 362, 380, 32, [255, 255, 255], 90);
  bar(1194, 408, 240, 20, [255, 255, 255], 52);
  for (let i = 0; i < 4; i++) bar(1194, 452 + i * 34, 540 - (i === 3 ? 170 : 0), 15, [255, 255, 255], 40);
  bar(1194, 620, 540, 66, AMBER, 60);
  noFill(); stroke(AMBER[0], AMBER[1], AMBER[2], 160); strokeWeight(2);
  rect(1194, 620, 540, 66, 14); noStroke();
  label('Remote images blocked — show once', 1464, 654, 26, AMBER, CENTER, false);
  bar(1194, 716, 150, 46, BLUE, 120);
  bar(1358, 716, 150, 46, [255, 255, 255], 40);
  label('Open', 1269, 740, 26, INK, CENTER, true);
  label('Save', 1433, 740, 26, SUB, CENTER, false);
  drawingContext.restore();
}

function scene4(lt) {
  ghostNum('04');
  eyebrow('OFFLINE-FIRST SQLITE CACHE', W / 2, 150, rise(lt, 0, 40, 30));
  label('Find anything, instantly.', W / 2, 268 + rise(lt, 8, 44, 44), 100, INK, CENTER, true);
  const q = 'invoice', n = Math.min(q.length, Math.floor(easeInOut(P(lt, 30, 110)) * (q.length + 1)));
  const shown = q.slice(0, n);
  card(460, 400, 1000, 108);
  label(shown, 510, 456, 52, INK, LEFT, false);
  if (Math.floor(lt / 15) % 2 === 0 && n < q.length) bar(510 + textWidth(shown) + 8, 420, 5, 60, CYAN, 255);
  if (n >= q.length) {
    const a = P(lt, 120, 140);
    drawingContext.save(); drawingContext.globalAlpha = a;
    label('128 hits · 0.02 s', 480, 566, 40, MINT, LEFT, true);
    const rows = ['Quarterly invoice attached — Today', 'Invoice #2418 — Tuesday', 'Re: invoice correction — Monday'];
    for (let i = 0; i < 3; i++) {
      const d = 135 + i * 22, rr = rise(lt, d, 30, 30), aa = P(lt, d, d + 24);
      drawingContext.save(); drawingContext.globalAlpha = aa;
      card(460, 600 + i * 84 + rr, 1000, 70, 16);
      bar(492, 622 + i * 84 + rr, 13, 13, CYAN, 255);
      label(rows[i], 520, 636 + i * 84 + rr, 29, INK, LEFT, false);
      drawingContext.restore();
    }
    drawingContext.restore();
  }
  label('Type 3 letters — the FTS index answers over subject, sender, body', W / 2, 872 + rise(lt, 150, 40, 26), 33, SUB, CENTER, false);
}

function scene5(lt) {
  ghostNum('05');
  eyebrow('COMPOSE  ·  READ', W / 2, 130, rise(lt, 0, 40, 30));
  label('Write and read with confidence.', W / 2, 240 + rise(lt, 8, 44, 44), 88, INK, CENTER, true);
  const lx = -780 + 940 * easeOutCubic(P(lt, 20, 70));
  const rx = 1920 - 940 * easeOutCubic(P(lt, 40, 90));
  drawingContext.save(); drawingContext.globalAlpha = P(lt, 20, 45);
  card(lx, 350, 780, 500);
  label('COMPOSE', lx + 44, 398, 30, CYAN, LEFT, true);
  const cl = ['Rich-text editor, attachments, drafts', 'Smart send format, plain twin optional', 'From-domain guard keeps SPF, DKIM', 'and DMARC aligned', 'Queued locally — sends even if', 'you close the window'];
  for (let i = 0; i < 6; i++) {
    bar(lx + 44, 446 + i * 56, 14, 14, i < 4 ? CYAN : MINT, 255);
    label(cl[i], lx + 74, 454 + i * 56, 30, INK, LEFT, false);
  }
  drawingContext.restore();
  drawingContext.save(); drawingContext.globalAlpha = P(lt, 40, 65);
  card(rx, 350, 780, 500);
  label('READ SAFELY', rx + 44, 398, 30, MINT, LEFT, true);
  const rl = ['Sanitized HTML, remote images blocked', 'Link-verify dialog before opening', 'Reply-To shown inline — no surprises', 'Raw headers on demand', 'Attachments download on demand,', 'then stay cached offline'];
  for (let i = 0; i < 6; i++) {
    bar(rx + 44, 446 + i * 56, 14, 14, i < 4 ? MINT : CYAN, 255);
    label(rl[i], rx + 74, 454 + i * 56, 30, INK, LEFT, false);
  }
  drawingContext.restore();
}

function scene6(lt) {
  ghostNum('06');
  eyebrow('BACKGROUND SYNC + OMARCHY WIDGET', W / 2, 140, rise(lt, 0, 40, 30));
  label('Quietly in sync.', W / 2, 256 + rise(lt, 8, 44, 44), 100, INK, CENTER, true);
  const bl = ['Startup, folder-open and background polling', 'Omarchy bar widget with unread badge', 'Headless --sync-once and --status JSON for scripts'];
  for (let i = 0; i < 3; i++) {
    const d = 30 + i * 24, r = rise(lt, d, 36, 30), a = P(lt, d, d + 28);
    drawingContext.save(); drawingContext.globalAlpha = a;
    bar(W / 2 - 560, 392 + i * 66 + r, 14, 14, MINT, 255);
    label(bl[i], W / 2 - 530, 400 + i * 66 + r, 34, INK, LEFT, false);
    drawingContext.restore();
  }
  // CTA with breathing glow
  const a = P(lt, 120, 160), pulse = 0.5 + 0.5 * Math.sin(lt * 0.07);
  drawingContext.save(); drawingContext.globalAlpha = a;
  drawingContext.shadowBlur = 34 + 22 * pulse;
  drawingContext.shadowColor = 'rgba(52,211,153,0.55)';
  card(W / 2 - 470, 600, 940, 120, 60);
  drawingContext.shadowBlur = 0;
  noFill(); stroke(MINT[0], MINT[1], MINT[2], 220); strokeWeight(3);
  rect(W / 2 - 470, 600, 940, 120, 60); noStroke();
  label('Free and open — run ./dev.sh to try it', W / 2, 662, 44, INK, CENTER, true);
  label('mailclient', W / 2, 800 + rise(lt, 150, 44, 30), 110, CYAN, CENTER, true);
  label('Rust  ·  Qt 6  ·  SQLite  ·  Omarchy first', W / 2, 868, 32, SUB, CENTER, false);
  drawingContext.restore();
}

const SCENES = [scene1, scene2, scene3, scene4, scene5, scene6];

function renderFrame(f) {
  f = Math.max(0, Math.min(TOTAL - 1, Math.floor(f)));
  const sc = Math.min(NSC - 1, Math.floor(f / SCENE_LEN));
  const lt = f - sc * SCENE_LEN;
  background(10, 15, 36);
  vignette();
  const aIn = easeOutCubic(P(lt, 0, 14));
  const aOut = 1 - easeInOut(P(lt, SCENE_LEN - 18, SCENE_LEN));
  drawingContext.save();
  drawingContext.globalAlpha = Math.min(aIn, aOut);
  SCENES[sc](lt);
  drawingContext.restore();
  chrome(f, sc);
}

function draw() {
  if (typeof window !== 'undefined' && window.__frame !== undefined) renderFrame(window.__frame);
}
