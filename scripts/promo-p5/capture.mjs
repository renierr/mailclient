// Frame-accurate capture of the p5 sketch via CDP (no dependencies, Node built-in WebSocket).
// Usage: node capture.mjs <port> <outdir> <start> <end>
import { spawn } from 'node:child_process';
import fs from 'node:fs';

const [port, outdir, startS, endS] = [process.argv[2], process.argv[3], +process.argv[4], +process.argv[5]];
fs.mkdirSync(outdir, { recursive: true });

class CDP {
  constructor(url) { this.ws = new WebSocket(url); this.id = 0; this.pending = new Map();
    this.ws.onmessage = ev => { const m = JSON.parse(String(ev.data)); if (m.id && this.pending.has(m.id)) { this.pending.get(m.id)(m); this.pending.delete(m.id); } };
  }
  ready() { return new Promise(res => this.ws.readyState === 1 ? res() : (this.ws.onopen = () => res())); }
  send(method, params = {}) { const id = ++this.id; this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise(res => this.pending.set(id, res)); }
  close() { this.ws.close(); }
}

const chrome = spawn('chromium', ['--headless', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage',
  '--hide-scrollbars', '--window-size=1920,1080', `--remote-debugging-port=${port}`,
  '--user-data-dir=/tmp/opencode/promo2/profile', 'about:blank'], { stdio: 'ignore' });
await new Promise(r => setTimeout(r, 2500));

const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = targets.find(t => t.type === 'page');
const cdp = new CDP(page.webSocketDebuggerUrl);
await cdp.ready();
await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
await cdp.send('Page.navigate', { url: 'http://127.0.0.1:8901/' });
for (let i = 0; i < 150; i++) {
  const r = await cdp.send('Runtime.evaluate', { expression: 'window.__ready === true', returnByValue: true });
  if (r.result?.result?.value) break;
  await new Promise(rr => setTimeout(rr, 200));
}
for (let f = startS; f <= endS; f++) {
  const name = `${outdir}/${String(f).padStart(4, '0')}.png`;
  if (fs.existsSync(name)) { if (f % 120 === 0) console.log(`skip ${f} (exists)`); continue; }
  await cdp.send('Runtime.evaluate', { expression: `renderFrame(${f})`, awaitPromise: false, returnByValue: true });
  const shot = await cdp.send('Page.captureScreenshot', { format: 'png', fromSurface: true });
  fs.writeFileSync(name, Buffer.from(shot.result.data, 'base64'));
  if (f % 60 === 0) console.log(`frame ${f}/${endS}`);
}
cdp.close();
chrome.kill();
console.log('CHUNK DONE');
