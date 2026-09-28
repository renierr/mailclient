// explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
//
// The fluffy pink narrator, drawn as SVG so it stays crisp at any size.
// Everything that moves is set from update(state) — no CSS animations — so a
// given moment always renders the same frame (the video renderer depends on it).

(function () {
  const NS = 'http://www.w3.org/2000/svg';

  // Small seeded PRNG so the fur looks the same on every load.
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;

  const FUR = ['#f06aa8', '#ec5c9f', '#f47fb5', '#e24d93', '#f590c0', '#d9418a', '#f7a3cb'];

  function el(name, attrs = {}, parent) {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
    if (parent) parent.appendChild(node);
    return node;
  }

  function strands(parent, count, rMin, rMax, width, colors, opacity) {
    const g = el('g', { 'stroke-linecap': 'round', fill: 'none', opacity }, parent);
    for (let i = 0; i < count; i++) {
      const a = rnd() * Math.PI * 2;
      const r0 = rMin + rnd() * 10;
      const r1 = rMax - rnd() * 22;
      const bend = (rnd() - 0.5) * 0.5;
      const x0 = Math.cos(a) * r0;
      const y0 = Math.sin(a) * r0 * 0.94;
      const x1 = Math.cos(a + bend) * r1;
      const y1 = Math.sin(a + bend) * r1 * 0.94;
      const cx = Math.cos(a + bend * 0.4) * (r0 + r1) * 0.55;
      const cy = Math.sin(a + bend * 0.4) * (r0 + r1) * 0.52;
      el('path', {
        d: `M${x0.toFixed(1)},${y0.toFixed(1)} Q${cx.toFixed(1)},${cy.toFixed(1)} ${x1.toFixed(1)},${y1.toFixed(1)}`,
        stroke: colors[Math.floor(rnd() * colors.length)],
        'stroke-width': (width * (0.7 + rnd() * 0.6)).toFixed(1),
      }, g);
    }
    return g;
  }

  // Short strokes scattered over the body, mostly pointing outwards: the fur's surface.
  function texture(parent, count, radius, colors, opacity) {
    const g = el('g', { 'stroke-linecap': 'round', fill: 'none', opacity }, parent);
    for (let i = 0; i < count; i++) {
      const r = Math.sqrt(rnd()) * radius;
      const a = rnd() * Math.PI * 2;
      const x = Math.cos(a) * r;
      const y = Math.sin(a) * r * 0.94;
      const dir = a + (rnd() - 0.5) * 1.4;
      const len = 7 + rnd() * 14;
      el('path', {
        d: `M${x.toFixed(1)},${y.toFixed(1)} q${(Math.cos(dir + 0.5) * len * 0.5).toFixed(1)},${(Math.sin(dir + 0.5) * len * 0.5).toFixed(1)} ${(Math.cos(dir) * len).toFixed(1)},${(Math.sin(dir) * len).toFixed(1)}`,
        stroke: colors[Math.floor(rnd() * colors.length)],
        'stroke-width': (2.5 + rnd() * 3).toFixed(1),
      }, g);
    }
    return g;
  }

  function tuft(parent, cx, cy, r) {
    const g = el('g', { transform: `translate(${cx},${cy})` }, parent);
    el('ellipse', { rx: r, ry: r * 0.8, fill: 'url(#av-body)' }, g);
    strands(g, 46, 2, r + 16, 4.5, FUR, 1);
    return g;
  }

  function build(host) {
    const svg = el('svg', { viewBox: '-170 -170 340 340', class: 'avatar-svg', 'aria-hidden': 'true' });
    const defs = el('defs', {}, svg);
    const body = el('radialGradient', { id: 'av-body', cx: '38%', cy: '30%', r: '75%' }, defs);
    el('stop', { offset: '0%', 'stop-color': '#ffb3d6' }, body);
    el('stop', { offset: '45%', 'stop-color': '#f064a7' }, body);
    el('stop', { offset: '100%', 'stop-color': '#b3246a' }, body);
    const shade = el('radialGradient', { id: 'av-shade', cx: '50%', cy: '35%', r: '65%' }, defs);
    el('stop', { offset: '60%', 'stop-color': '#000', 'stop-opacity': '0' }, shade);
    el('stop', { offset: '100%', 'stop-color': '#5a0a33', 'stop-opacity': '.55' }, shade);
    const eyeGrad = el('radialGradient', { id: 'av-eye', cx: '40%', cy: '35%', r: '70%' }, defs);
    el('stop', { offset: '0%', 'stop-color': '#ffffff' }, eyeGrad);
    el('stop', { offset: '100%', 'stop-color': '#e9dbe4' }, eyeGrad);
    const fuzz = el('filter', { id: 'av-fuzz', x: '-20%', y: '-20%', width: '140%', height: '140%' }, defs);
    el('feTurbulence', { type: 'fractalNoise', baseFrequency: '0.75', numOctaves: '2', seed: '3', result: 'n' }, fuzz);
    el('feDisplacementMap', { in: 'SourceGraphic', in2: 'n', scale: '7', xChannelSelector: 'R', yChannelSelector: 'G' }, fuzz);

    // Soft ground shadow, outside the bobbing group.
    const shadow = el('ellipse', { cx: 0, cy: 150, rx: 92, ry: 12, fill: '#000', opacity: '.28', class: 'av-shadow' }, svg);

    const root = el('g', {}, svg);        // bob + squash
    const armL = el('g', {}, root);
    const armR = el('g', {}, root);
    tuft(armL, 0, 0, 22);
    tuft(armR, 0, 0, 22);

    const furry = el('g', { filter: 'url(#av-fuzz)' }, root);
    strands(furry, 200, 58, 136, 10, ['#d9418a', '#e24d93', '#c43580'], 1);
    strands(furry, 320, 66, 130, 8, FUR, 1);
    el('ellipse', { rx: 110, ry: 104, fill: 'url(#av-body)' }, furry);
    texture(furry, 900, 104, ['#f590c0', '#f47fb5', '#ec5c9f', '#e24d93', '#f7a3cb'], 0.75);
    texture(furry, 260, 60, ['#ffc2de', '#ffd6ea', '#f7a3cb'], 0.5);
    el('ellipse', { rx: 112, ry: 106, fill: 'url(#av-shade)' }, furry);

    const face = el('g', {}, root);
    const eyes = [];
    for (const side of [-1, 1]) {
      const eye = el('g', { transform: `translate(${side * 36},-26)` }, face);
      const lid = el('g', {}, eye);
      el('ellipse', { rx: 31, ry: 35, fill: 'url(#av-eye)', stroke: '#8a1d52', 'stroke-width': '2.5' }, lid);
      const pupil = el('g', {}, lid);
      el('circle', { r: 15.5, fill: '#1b0b14' }, pupil);
      el('circle', { cx: -5.5, cy: -6, r: 5.2, fill: '#fff' }, pupil);
      el('circle', { cx: 5, cy: 5.5, r: 2.2, fill: '#fff', opacity: '.8' }, pupil);
      eyes.push({ lid, pupil });
    }
    for (const side of [-1, 1]) {
      el('ellipse', { cx: side * 70, cy: 20, rx: 17, ry: 9, fill: '#ff4f8b', opacity: '.45' }, face);
    }
    const mouth = el('g', { transform: 'translate(0,36)' }, face);
    const smile = el('path', { d: 'M-15,-2 Q0,11 15,-2', stroke: '#5b0e33', 'stroke-width': '5', fill: 'none', 'stroke-linecap': 'round' }, mouth);
    const open = el('g', {}, mouth);
    const lips = el('path', { fill: '#4a0a2a', stroke: '#5b0e33', 'stroke-width': '2', 'stroke-linejoin': 'round' }, open);
    const tongue = el('ellipse', { rx: 8, ry: 4.5, fill: '#ff7aa8' }, open);

    host.appendChild(svg);
    return { svg, shadow, root, armL, armR, eyes, face, smile, open, lips, tongue };
  }

  function blinkAmount(t) {
    // Blink roughly every 3–5 s, at moments fixed by t alone.
    const period = 4.1;
    const k = Math.floor(t / period);
    const at = k * period + ((k * 7919) % 13) / 13 * 2.2;
    const d = t - at;
    if (d < 0 || d > 0.18) return 0;
    return Math.sin((d / 0.18) * Math.PI);
  }

  window.createAvatar = function (host) {
    const a = build(host);
    return {
      /**
       * t: seconds, talk: 0..1 mouth opening, look: {x,y} −1..1,
       * wave: 0..1 how much the right arm waves, hop: 0..1 happy jump.
       */
      update({ t, talk = 0, look = { x: 0.3, y: -0.2 }, wave = 0, hop = 0 }) {
        const bob = Math.sin(t * 2.3) * 4 - hop * 38;
        const breathe = 1 + Math.sin(t * 1.7) * 0.012;
        const sq = 1 + talk * 0.035 - hop * 0.05;
        a.root.setAttribute('transform', `translate(0,${bob.toFixed(2)}) scale(${(breathe / Math.sqrt(sq)).toFixed(4)},${(breathe * sq).toFixed(4)})`);
        a.shadow.setAttribute('rx', (92 - hop * 22 + Math.sin(t * 2.3) * 2).toFixed(1));
        a.shadow.setAttribute('opacity', (0.28 - hop * 0.12).toFixed(3));

        a.armL.setAttribute('transform', `translate(-104,52) rotate(${(Math.sin(t * 2.3 + 1) * 6).toFixed(1)})`);
        const waveAngle = wave * (-70 + Math.sin(t * 13) * 28);
        a.armR.setAttribute('transform', `translate(${104 + wave * 12},${52 - wave * 58}) rotate(${(waveAngle + Math.sin(t * 2.3) * 6).toFixed(1)})`);

        const b = blinkAmount(t);
        for (const e of a.eyes) {
          e.lid.setAttribute('transform', `scale(1,${(1 - b * 0.92).toFixed(3)})`);
          e.pupil.setAttribute('transform', `translate(${(look.x * 11).toFixed(1)},${(look.y * 12).toFixed(1)})`);
        }
        a.face.setAttribute('transform', `translate(${(look.x * 7).toFixed(1)},${(look.y * 5).toFixed(1)})`);

        const o = Math.max(0, Math.min(1, talk));
        a.smile.setAttribute('opacity', o < 0.08 ? 1 : 0);
        a.open.setAttribute('opacity', o < 0.08 ? 0 : 1);
        // A happy "D": slightly smiling top edge, round bottom that drops as it opens.
        const w = 17 - o * 3;
        const h = 5 + o * 17;
        a.lips.setAttribute('d', `M${-w},-3 Q0,3 ${w},-3 Q${w * 0.95},${h} 0,${h} Q${-w * 0.95},${h} ${-w},-3Z`);
        a.tongue.setAttribute('cy', (h - 4).toFixed(1));
        a.tongue.setAttribute('opacity', o > 0.35 ? 1 : 0);
      },
    };
  };
})();
