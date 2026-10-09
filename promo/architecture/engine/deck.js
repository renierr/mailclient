// explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
//
// Drives the whole deck from one number: the time t in seconds. frame(t) sets every
// animated property from t alone, so the page can play live (t = the narration's
// currentTime) or be stepped frame by frame by the renderer (window.seek(t)) and look
// identical either way. No CSS animations or transitions anywhere, for that reason.
//
// Per scene (<section class="scene" data-scene="id">):
//   data-avatar="corner"            avatar pose(s): center | hero | hero-right | corner | hidden,
//                                   later ones at a cue: "center, hero@title"
//   data-look="0.3,-0.8"            where the avatar's eyes point (-1..1)
//   data-wave="hi" / "bye+"         wave for a moment after the cue / from the cue to the end
//   data-confetti="bye"             confetti burst and a happy hop at the cue
// Per element: data-fx, data-cue, data-delay, data-dur, data-until (see the skill's reference).
//   <path data-fx="draw" data-flow> dots travel along the path once it is drawn
//   class="pulse"                   a ring that keeps pulsing outwards
// Project hooks: window.DECK_HOOKS = { sceneId(scene, tLocal, t) {…} }, defined before this file.

(function () {
  const T = window.TIMELINE;
  const params = new URLSearchParams(location.search);
  const RENDER = params.has('render');
  const stage = document.getElementById('stage');
  if (RENDER) document.body.classList.add('render');

  // The chrome every deck shares; a project's index.html only holds its scenes.
  const viewport = document.createElement('div');
  viewport.id = 'viewport';
  stage.before(viewport);
  viewport.appendChild(stage);
  stage.insertAdjacentHTML('afterbegin', `
    <div id="bg"><div class="blob b1"></div><div class="blob b2"></div><div class="blob b3"></div>
      <div class="grid"></div><canvas id="sparks" width="1920" height="1080"></canvas></div>`);
  stage.insertAdjacentHTML('beforeend', `
    <div id="avatar"></div>
    <div id="subs"><div class="subs-box"></div></div>
    <div id="progress"><div></div></div>
    <div id="start" hidden>
      <button id="play" aria-label="Play">▶</button>
      <p class="hint">Space pause · ← → scenes · C subtitles · F fullscreen</p>
      <p class="missing" hidden>No narration yet — run <code>bun run voice</code> first.</p>
    </div>`);
  document.body.insertAdjacentHTML('beforeend', '<audio id="narration" src="../build/narration.wav" preload="auto"></audio>');
  if (params.has('nosubs')) document.body.classList.add('no-subs');

  // ---------------------------------------------------------------- icons

  const ICONS = {
    mr: '<circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 21V9a9 9 0 0 0 9 9"/>',
    ticket: '<path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/><path d="M13 5v2M13 17v2M13 11v2"/>',
    book: '<path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/>',
    chat: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
    key: '<circle cx="7.5" cy="15.5" r="5.5"/><path d="m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"/>',
    file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M9 15l2 2 4-4"/>',
    plug: '<path d="M12 22v-5M9 8V2M15 8V2M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>',
    term: '<path d="m4 17 6-6-6-6M12 19h8"/>',
    window: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M2 9h20"/>',
    spark: '<path d="M12 3l1.9 5.8L20 11l-6.1 2.2L12 19l-1.9-5.8L4 11l6.1-2.2z"/>',
    code: '<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>',
    hex: '<path d="M12 2 21 7v10l-9 5-9-5V7z"/>',
    win: '<path d="M3 3h8v8H3zM13 3h8v8h-8zM3 13h8v8H3zM13 13h8v8h-8z"/>',
    refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5"/>',
    shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/>',
    pen: '<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>',
    bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  };
  for (const i of document.querySelectorAll('i.ic')) {
    const name = [...i.classList].find((c) => c.startsWith('ic-'))?.slice(3);
    if (ICONS[name]) {
      i.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
    }
  }

  // ---------------------------------------------------------------- stage scaling

  function fit() {
    const s = RENDER ? 1 : Math.min(innerWidth / 1920, innerHeight / 1080);
    stage.style.transform = `translate(-50%, -50%) scale(${s})`;
  }
  addEventListener('resize', fit);
  fit();

  if (!T) {
    const start = document.getElementById('start');
    start.hidden = false;
    start.querySelector('.missing').hidden = false;
    document.getElementById('play').hidden = true;
    return;
  }

  // ---------------------------------------------------------------- easing

  const clamp = (v, a = 0, b = 1) => Math.min(b, Math.max(a, v));
  const lerp = (a, b, p) => a + (b - a) * p;
  const outCubic = (p) => 1 - Math.pow(1 - p, 3);
  const inOutCubic = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
  const outBack = (p) => 1 + 2.4 * Math.pow(p - 1, 3) + 1.4 * Math.pow(p - 1, 2);
  const outElastic = (p) => (p === 0 || p === 1 ? p : Math.pow(2, -10 * p) * Math.sin((p * 10 - 0.75) * ((2 * Math.PI) / 3)) + 1);

  // ---------------------------------------------------------------- scenes and fx

  const byId = Object.fromEntries(T.scenes.map((s) => [s.id, s]));
  const scenes = [...document.querySelectorAll('.scene')].map((node) => {
    const data = byId[node.dataset.scene];
    if (!data) console.warn(`scene "${node.dataset.scene}" is not in the timeline`);
    return { node, data };
  }).filter((s) => s.data);

  function cueTime(scene, name) {
    if (!name) return 0;
    const v = scene.data.cues[name];
    if (v == null) console.warn(`cue "${name}" not found in scene "${scene.data.id}"`);
    return v ?? 0;
  }

  const DEFAULT_DUR = { type: 0, draw: 0.9, flip: 0.9, drop: 0.8, blur: 0.9 };
  for (const scene of scenes) {
    scene.fx = [...scene.node.querySelectorAll('[data-fx]')].map((node, index) => {
      const d = node.dataset;
      const at = cueTime(scene, d.cue) + parseFloat(d.delay || 0);
      let dur = parseFloat(d.dur || DEFAULT_DUR[d.fx] || 0.7);
      if (d.until) dur = Math.max(0.5, cueTime(scene, d.until) - at + 0.6);
      const fx = { node, kind: d.fx, at, dur, index, text: d.text || '' };
      if (fx.kind === 'drop') fx.tilt = parseFloat(getComputedStyle(node).getPropertyValue('--tilt')) || 0;
      if (fx.kind === 'type') {
        fx.dur = fx.dur || Math.max(0.6, fx.text.length * 0.028);
        node.textContent = '';
        fx.caret = document.createElement('span');
        fx.caret.className = 'caret';
      }
      return fx;
    });
  }

  function applyFx(fx, tl, t) {
    const raw = clamp((tl - fx.at) / fx.dur);
    const p = outCubic(raw);
    const s = fx.node.style;
    switch (fx.kind) {
      case 'fade':
        s.opacity = p;
        break;
      case 'up':
        s.opacity = p;
        s.transform = `translateY(${(1 - p) * 50}px)`;
        break;
      case 'left':
      case 'right': {
        const dir = fx.kind === 'left' ? -1 : 1;
        s.opacity = p;
        s.transform = `translateX(${dir * (1 - p) * 90}px)`;
        break;
      }
      case 'pop':
        s.opacity = clamp(raw * 2.5);
        s.transform = `scale(${lerp(0.4, 1, outBack(raw))})`;
        break;
      case 'zoom':
        s.opacity = p;
        s.transform = `scale(${lerp(0.9, 1, p)})`;
        break;
      case 'blur':
        s.opacity = p;
        s.filter = raw < 1 ? `blur(${(1 - p) * 18}px)` : 'none';
        s.transform = `translateY(${(1 - p) * 24}px) scale(${lerp(1.06, 1, p)})`;
        break;
      case 'drop': {
        const tilt = fx.tilt;
        const wobble = raw >= 1 ? Math.sin(t * 1.6 + fx.index * 1.7) * 1.2 : 0;
        s.opacity = clamp(raw * 3);
        s.transform = `translateY(${(1 - outBack(raw)) * -160}px) rotate(${lerp(tilt * 4, tilt, p) + wobble}deg)`;
        break;
      }
      case 'flip':
        s.opacity = clamp(raw * 2);
        s.transformOrigin = 'left center';
        s.transform = `rotateY(${(1 - p) * -75}deg)`;
        break;
      case 'check':
        s.opacity = clamp(raw * 3);
        s.transform = `scale(${outBack(raw)}) rotate(${(1 - p) * -40}deg)`;
        break;
      case 'draw':
        s.opacity = raw > 0 ? 1 : 0;
        s.strokeDashoffset = 1 - inOutCubic(raw);
        break;
      case 'type': {
        const n = Math.round(clamp((tl - fx.at) / fx.dur) * fx.text.length);
        const typing = tl >= fx.at - 0.4 && tl < fx.at + fx.dur + 1.2;
        const caretOn = typing && (raw < 1 || Math.floor(t * 2.5) % 2 === 0);
        s.opacity = tl >= fx.at - 0.4 ? 1 : 0;
        if (fx.shown !== n) {
          fx.node.textContent = fx.text.slice(0, n);
          fx.shown = n;
          fx.caretIn = false;
        }
        if (caretOn !== fx.caretIn) {
          if (caretOn) fx.node.appendChild(fx.caret); else fx.caret.remove();
          fx.caretIn = caretOn;
        }
        break;
      }
      default:
        s.opacity = p;
    }
  }

  // ---------------------------------------------------------------- avatar

  const avatarHost = document.getElementById('avatar');
  const avatar = window.createAvatar(avatarHost);
  const PRESETS = {
    center: { x: 960, y: 560, s: 1.9, look: { x: 0, y: -0.1 } },
    hero: { x: 450, y: 560, s: 1.55, look: { x: 0.6, y: -0.15 } },
    'hero-right': { x: 1470, y: 560, s: 1.55, look: { x: -0.6, y: -0.15 } },
    corner: { x: 190, y: 890, s: 0.62, look: { x: 0.55, y: -0.45 } },
    hidden: { x: 190, y: 1000, s: 0.001, look: { x: 0, y: 0 } },
  };
  const keys = [];
  const waves = [];
  const confetti = [];
  for (const scene of scenes) {
    const d = scene.node.dataset;
    const look = d.look ? (([x, y]) => ({ x: +x, y: +y }))(d.look.split(',')) : null;
    for (const part of (d.avatar || 'corner').split(',').map((p) => p.trim()).filter(Boolean)) {
      const [name, cue] = part.split('@');
      const preset = PRESETS[name];
      if (!preset) { console.warn(`unknown avatar pose "${name}" in scene "${scene.data.id}"`); continue; }
      keys.push({ time: scene.data.start + (cue ? cueTime(scene, cue) : 0), ...preset, look: look || preset.look });
    }
    for (const w of (d.wave || '').split(',').map((p) => p.trim()).filter(Boolean)) {
      const open = w.endsWith('+');
      const at = scene.data.start + cueTime(scene, w.replace(/\+$/, ''));
      waves.push([at - 0.2, open ? Infinity : at + 2.4]);
    }
    for (const c of (d.confetti || '').split(',').map((p) => p.trim()).filter(Boolean)) {
      confetti.push(scene.data.start + cueTime(scene, c));
    }
  }
  keys.sort((a, b) => a.time - b.time);
  const MOVE = 1.0;

  function poseAt(t) {
    let k = 0;
    while (k + 1 < keys.length && keys[k + 1].time <= t) k++;
    const cur = keys[k];
    const prev = keys[k - 1];
    if (!prev || t >= cur.time + MOVE || t < cur.time) return cur;
    const p = inOutCubic((t - cur.time) / MOVE);
    return {
      x: lerp(prev.x, cur.x, p), y: lerp(prev.y, cur.y, p), s: lerp(prev.s, cur.s, p),
      look: { x: lerp(prev.look.x, cur.look.x, p), y: lerp(prev.look.y, cur.look.y, p) },
      hop: Math.sin(p * Math.PI) * 0.35,
    };
  }

  function talkAt(t) {
    const f = t * T.fps;
    const i = Math.floor(f);
    const m = T.mouth;
    const v = (k) => (m[k] || 0) / 99;
    const smooth = (v(i - 1) + v(i) * 2 + v(i + 1)) / 4;
    const cur = lerp(v(i), v(i + 1), f - i);
    return clamp(Math.max(smooth, cur * 0.8) * 1.15);
  }

  function windowAmount(t, from, to, ramp = 0.3) {
    if (t < from || t > to) return 0;
    return clamp(Math.min((t - from) / ramp, (to - t) / ramp));
  }

  function updateAvatar(t) {
    const pose = poseAt(t);
    const pop = clamp((t - 0.25) / 1.1);
    let wave = 0;
    let hop = pose.hop || 0;
    for (const [from, to] of waves) wave = Math.max(wave, windowAmount(t, from, to));
    for (const at of confetti) {
      if (t > at && t < at + 1.05) hop = Math.max(hop, Math.max(0, Math.sin((t - at) * 6)));
    }
    avatarHost.style.transform = `translate(${pose.x}px, ${pose.y}px) scale(${pose.s * outElastic(pop)})`;
    avatarHost.style.opacity = pop > 0 ? 1 : 0;
    avatar.update({ t, talk: talkAt(t), look: pose.look, wave, hop });
    return pose;
  }

  // ---------------------------------------------------------------- background + particles

  const blobs = [...document.querySelectorAll('.blob')];
  const canvas = document.getElementById('sparks');
  const ctx = canvas.getContext('2d');
  const R = (i, k) => {
    const x = Math.sin(i * 127.1 + k * 311.7) * 43758.5453;
    return x - Math.floor(x);
  };
  const CONFETTI = ['#ff5fa8', '#c084fc', '#22d3ee', '#fbbf24', '#34d399', '#ffffff'];

  function drawBackground(t, avatarPose) {
    blobs[0].style.transform = `translate(${-150 + Math.sin(t * 0.21) * 260}px, ${-250 + Math.cos(t * 0.17) * 160}px)`;
    blobs[1].style.transform = `translate(${1250 + Math.cos(t * 0.15) * 240}px, ${420 + Math.sin(t * 0.19) * 200}px)`;
    blobs[2].style.transform = `translate(${600 + Math.sin(t * 0.12 + 2) * 400}px, ${650 + Math.cos(t * 0.14) * 120}px)`;

    ctx.clearRect(0, 0, 1920, 1080);
    for (let i = 0; i < 70; i++) {
      const x = (R(i, 1) * 1920 + t * (8 + R(i, 2) * 20)) % 1980 - 30;
      const y = (R(i, 3) * 1080 - t * (6 + R(i, 4) * 18) + 1080 * 4) % 1110 - 15;
      const tw = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * (1 + R(i, 5) * 2) + i));
      ctx.globalAlpha = 0.12 + tw * 0.3;
      ctx.fillStyle = i % 3 ? '#ffffff' : '#ff9ccb';
      ctx.beginPath();
      ctx.arc(x, y, 1 + R(i, 6) * 2.2, 0, Math.PI * 2);
      ctx.fill();
    }

    for (const at of confetti) {
      const age = t - at;
      if (age <= 0 || age >= 6) continue;
      for (let i = 0; i < 160; i++) {
        const a = -Math.PI / 2 + (R(i, 7) - 0.5) * 2.6;
        const v = 700 + R(i, 8) * 900;
        const x = avatarPose.x + Math.cos(a) * v * age * 0.9;
        const y = avatarPose.y - 120 + Math.sin(a) * v * age + 900 * age * age;
        if (y > 1100) continue;
        ctx.save();
        ctx.globalAlpha = clamp(1.4 - age / 4);
        ctx.translate(x, y);
        ctx.rotate(age * (4 + R(i, 9) * 8) + i);
        ctx.fillStyle = CONFETTI[i % CONFETTI.length];
        ctx.fillRect(-7, -4, 14, 8 * Math.abs(Math.cos(age * 6 + i)));
        ctx.restore();
      }
    }
    ctx.globalAlpha = 1;
  }

  // ---------------------------------------------------------------- scene specials

  for (const scene of scenes) {
    scene.flows = [...scene.node.querySelectorAll('path[data-flow]')].flatMap((path) => {
      const fx = scene.fx.find((f) => f.node === path);
      return [0, 0.5].map((phase) => {
        const c = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
        c.setAttribute('r', 7);
        c.setAttribute('class', 'packet');
        path.ownerSVGElement.appendChild(c);
        return { c, path, fx, phase, len: path.getTotalLength() };
      });
    });
    scene.pulses = [...scene.node.querySelectorAll('.pulse')];
  }
  const HOOKS = window.DECK_HOOKS || {};

  function specials(scene, tl, t) {
    for (const p of scene.flows) {
      const ready = !p.fx || tl > p.fx.at + p.fx.dur;
      p.c.style.opacity = ready ? 1 : 0;
      if (!ready) continue;
      const pt = p.path.getPointAtLength(((t * 0.45 + p.phase) % 1) * p.len);
      p.c.setAttribute('cx', pt.x);
      p.c.setAttribute('cy', pt.y);
    }
    for (const ring of scene.pulses) {
      const k = (t * 0.8) % 1;
      ring.style.transform = `scale(${1 + k * 0.35})`;
      ring.style.opacity = 1 - k;
    }
    HOOKS[scene.data.id]?.(scene, tl, t);
  }

  // ---------------------------------------------------------------- subtitles + progress

  const subsBox = document.querySelector('.subs-box');
  const progress = document.querySelector('#progress div');
  let subIndex = -1;
  let subWords = [];

  function drawSubs(t) {
    const S = T.subtitles;
    let i = -1;
    for (let k = 0; k < S.length && S[k].start <= t; k++) i = k;
    const cue = S[i];
    if (!cue || t > cue.end + 0.25) {
      subsBox.style.opacity = 0;
      return;
    }
    if (i !== subIndex) {
      subIndex = i;
      subsBox.innerHTML = '';
      subWords = cue.words.map(([text, start, end], n) => {
        const span = document.createElement('span');
        span.className = 'w';
        span.textContent = text;
        if (n) subsBox.appendChild(document.createTextNode(' '));
        subsBox.appendChild(span);
        return { span, start, end };
      });
    }
    const prev = S[i - 1];
    const joined = prev && cue.start - prev.end < 0.3;
    const fadeIn = joined ? 1 : clamp((t - cue.start) / 0.18);
    subsBox.style.opacity = Math.min(fadeIn, clamp((cue.end + 0.25 - t) / 0.25));
    subsBox.style.transform = `translateY(${(1 - fadeIn) * 14}px)`;
    for (const w of subWords) {
      w.span.classList.toggle('said', t >= w.start);
      w.span.classList.toggle('now', t >= w.start && t < w.end + 0.04);
    }
  }

  // ---------------------------------------------------------------- frame

  function frame(t) {
    t = clamp(t, 0, T.duration);
    for (const scene of scenes) {
      const { start, duration } = scene.data;
      const end = start + duration;
      const first = scene === scenes[0];
      const last = scene === scenes.at(-1);
      const vin = first ? 1 : clamp((t - start + 0.25) / 0.6);
      const vout = last ? 1 : clamp((end + 0.25 - t) / 0.6);
      const v = Math.min(vin, vout);
      const n = scene.node.style;
      if (v <= 0) {
        if (n.visibility !== 'hidden') n.visibility = 'hidden';
        n.opacity = 0;
        continue;
      }
      n.visibility = 'visible';
      n.opacity = v;
      const zoom = vin < 1 ? lerp(1.04, 1, outCubic(vin)) : lerp(0.97, 1, outCubic(vout));
      n.transform = `scale(${zoom})`;
      n.filter = v < 1 ? `blur(${(1 - v) * 8}px)` : 'none';
      const tl = t - start;
      for (const fx of scene.fx) applyFx(fx, tl, t);
      specials(scene, tl, t);
    }
    const pose = updateAvatar(t);
    drawBackground(t, pose);
    drawSubs(t);
    progress.style.width = `${(t / T.duration) * 100}%`;
  }

  window.seek = frame;
  window.deckReady = document.fonts.ready.then(() => {
    frame(0);
    return { duration: T.duration, fps: T.fps };
  });
  if (RENDER) return;

  // ---------------------------------------------------------------- live playback

  const audio = document.getElementById('narration');
  const overlay = document.getElementById('start');
  let playing = false;
  let pos = 0;          // where playback stands while paused
  let clock = null;     // performance.now() origin, when the audio cannot play (e.g. autoplay blocked)

  const now = () => (!playing ? pos : clock != null ? (performance.now() - clock) / 1000 : audio.currentTime);

  function loop() {
    if (!playing) return;
    const t = now();
    frame(t);
    if (t >= T.duration) return stop(true);
    requestAnimationFrame(loop);
  }

  async function play() {
    overlay.hidden = true;
    try {
      audio.currentTime = pos;
      await audio.play();
      clock = null;
    } catch {
      clock = performance.now() - pos * 1000;
    }
    playing = true;
    requestAnimationFrame(loop);
  }

  function stop(ended) {
    pos = ended ? 0 : now();
    playing = false;
    audio.pause();
    if (ended) overlay.hidden = false;
  }

  function jump(t) {
    pos = clamp(t, 0, T.duration - 0.05);
    audio.currentTime = pos;
    if (clock != null) clock = performance.now() - pos * 1000;
    frame(pos);
  }

  function sceneIndexAt(t) {
    let i = 0;
    while (i + 1 < T.scenes.length && T.scenes[i + 1].start <= t + 0.05) i++;
    return i;
  }

  document.getElementById('play').addEventListener('click', (e) => { e.stopPropagation(); play(); });
  stage.addEventListener('click', () => (playing ? stop() : play()));
  addEventListener('keydown', (e) => {
    const t = now();
    if (e.key === ' ' || e.key === 'k') { e.preventDefault(); playing ? stop() : play(); }
    else if (e.key === 'ArrowRight') jump(T.scenes[Math.min(T.scenes.length - 1, sceneIndexAt(t) + 1)].start);
    else if (e.key === 'ArrowLeft') {
      const i = sceneIndexAt(t);
      jump(t - T.scenes[i].start > 1.5 ? T.scenes[i].start : T.scenes[Math.max(0, i - 1)].start);
    }
    else if (e.key === 'Home') jump(0);
    else if (e.key === 'c') { document.body.classList.toggle('no-subs'); }
    else if (e.key === 'f') { document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen(); }
  });

  const startAt = params.has('t') ? parseFloat(params.get('t')) : (byId[location.hash.slice(1)]?.start ?? 0);
  window.deckReady.then(() => {
    overlay.hidden = false;
    jump(startAt);
  });
})();
