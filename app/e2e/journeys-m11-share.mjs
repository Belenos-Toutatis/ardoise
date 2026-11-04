// Milestone M11 journey "share the sheet on the local network" (contract: docs/design/lan-share.md).
//
// On the production executable with the simulated tablet (`--simulated-tablet`): live mode is open,
// the share panel starts the share, then this script plays the part of the other PC. Node speaks to the
// port with `node:http` (the page, its assets, `POST /join`, the event stream `/events`) and with raw
// `node:net` sockets for the paths a client library would normalise (`..`, backslash, `//`). The app
// listens on 127.0.0.1 only (`ARDOISE_SHARE_LOOPBACK=1`: no firewall prompt, no rule left for each
// temporary copy of the exe) on a free port written in `[share]` by the harness (`app.sharePort`).
//
// What is checked: the viewer page and its assets, the access code and its cookie, a list of paths that
// must all answer 404, the stream (snapshot first, stripped of anything personal, points arriving with a
// median delay under 50 ms from the app's own timestamps; the network and the other PC's drawing are
// what H4 judges by eye), the marks written over the presentation (set through `live_set_marks`, as the
// presentation window does: `marks` events with only what is drawn, ink on the viewer's canvas, an empty list
// when the page changes), pause and resume, the note open in the editor (`note`, then `noteStrokes`),
// and the stop (`end`, port closed). Same pattern as the other journeys (labels from the translation files).
//
// The page itself is opened too, in a real browser: headless Microsoft Edge (if this PC has it) driven over
// the DevTools protocol like the app's window. It types the code, waits for the live sheet to be drawn
// (ink on the canvas, so the WebAssembly module loaded under the page's policy), follows the switch to
// the editor's note and the end of the share, and no console message may mention the policy or WebAssembly.
//
// A second, short journey holds the share's configured port with a Node server (as another program does on a
// real PC) before the share starts: the share must take the next free port, say so in its panel, answer there,
// keep that port in this PC's table of ardoise.toml (`share_port`) and ask for it again at the next share.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// Wall-clock time in fractional milliseconds, as the app's `recv` (a `SystemTime`): `Date.now()` is whole milliseconds, rounded down, which made
// a delay of a few hundred microseconds come out negative.
const wallMs = () => performance.timeOrigin + performance.now();
const STROKES = '.editor g.ink path[data-stroke]';
const strokeCount = `document.querySelectorAll(${j(STROKES)}).length`;
/** The title given to the note of the editor: the stream must never carry it. */
const SECRET_TITLE = 'Secret E2E 4821';
const LIVE_CANVAS ='canvas.live-canvas';
const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;

// --- Pure helpers (also run in a local Node snippet, see the report) ----------------------------------

/**
 * Parser of a `text/event-stream`: `feed(chunk)` any slice of the text; `onEvent(data)` is called with the
 * `data` of each complete event (several `data:` lines joined by a newline). Comments (`: ping`) and the other
 * fields (`retry`, `event`, `id`) are ignored.
 */
export function createSseParser(onEvent) {
  let buffer = '';
  let data = [];
  const line = (text) => {
    if (text === '') {
      if (data.length > 0) onEvent(data.join('\n'));
      data = [];
    } else if (!text.startsWith(':')) {
      const colon = text.indexOf(':');
      const field = colon < 0 ? text : text.slice(0, colon);
      let value = colon < 0 ? '' : text.slice(colon + 1);
      if (value.startsWith(' ')) value = value.slice(1);
      if (field === 'data') data.push(value);
    }
  };
  return {
    feed(chunk) {
      buffer += chunk;
      // A lone CR at the very end may be the first half of a CRLF: wait for the next chunk.
      for (let m = /\r\n|\n|\r(?!$)/.exec(buffer); m; m = /\r\n|\n|\r(?!$)/.exec(buffer)) {
        const head = buffer.slice(0, m.index);
        buffer = buffer.slice(m.index + m[0].length);
        line(head);
      }
    },
  };
}

/** The `p`-quantile (0..1) of `values`, by linear interpolation. */
export function quantile(values, p) {
  if (values.length === 0) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  const at = (sorted.length - 1) * p;
  const low = Math.floor(at);
  const high = Math.ceil(at);
  return sorted[low] + (sorted[high] - sorted[low]) * (at - low);
}

/** Delays (ms) between the app's receipt of each point of the batches after index `from` and the arrival of its message. */
export function pointDelays(messages, from = 0) {
  const delays = [];
  for (const m of messages) {
    // A `reset` batch starts with a snapshot: its stroke in progress was received long ago, not a delay.
    if (m.i < from || m.msg.type !== 'live' || m.msg.reset) continue;
    for (const event of m.msg.batch?.events ?? []) {
      const points = event.type === 'points' ? event.points : event.type === 'strokeStart' ? event.stroke?.points : undefined;
      for (const point of points ?? []) if (typeof point.recv === 'number') delays.push(m.at - point.recv);
    }
  }
  return delays;
}

/** Every `src`/`href` of an HTML text, as paths of the server at `origin`; the ones that point elsewhere are returned apart. */
export function pageReferences(html, origin) {
  const paths = new Set();
  const foreign = [];
  for (const match of html.matchAll(/\b(?:src|href)\s*=\s*["']([^"']+)["']/gi)) {
    const ref = match[1];
    if (ref.startsWith('#') || ref.startsWith('data:')) continue;
    const url = new URL(ref, `${origin}/`);
    if (url.origin === origin) paths.add(url.pathname);
    else foreign.push(ref);
  }
  return { paths: [...paths], foreign };
}

/** Chunks and the WebAssembly file a script refers to (`/assets/…`), as paths of the server. */
export function scriptReferences(js) {
  const chunks = new Set();
  for (const match of js.matchAll(/(?:from|import)\s*\(?\s*["']\.\/([\w.-]+\.js)["']/g)) chunks.add(`/assets/${match[1]}`);
  const wasm = new Set();
  for (const match of js.matchAll(/assets\/(ardoise_ink[\w.-]*\.wasm)/g)) wasm.add(`/assets/${match[1]}`);
  return { chunks: [...chunks], wasm: [...wasm] };
}

// --- Network helpers ------------------------------------------------------------------------------------

/** An HTTP request to 127.0.0.1:`port`, one connection per request; resolves with `{ status, headers, body }`. */
export function request(port, { method = 'GET', path = '/', headers = {}, body, timeout = 5000 } = {}) {
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? undefined : Buffer.from(body);
    const req = http.request(
      {
        host: '127.0.0.1',
        port,
        method,
        path,
        agent: false,
        headers: { ...headers, ...(payload ? { 'Content-Length': payload.length } : {}) },
      },
      (res) => {
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
        res.on('error', reject);
      },
    );
    req.setTimeout(timeout, () => req.destroy(new Error(`${method} ${path}: no answer in ${timeout} ms`)));
    req.on('error', reject);
    req.end(payload);
  });
}

/** The status of a hand-written request (`head` is everything before the blank line): nothing is normalised on the way. */
function rawStatus(port, head, timeout = 5000) {
  return new Promise((resolve, reject) => {
    const socket = net.connect({ host: '127.0.0.1', port });
    let received = '';
    const done = () => {
      socket.destroy();
      const status = /^HTTP\/1\.[01] (\d{3})/.exec(received)?.[1];
      resolve(status === undefined ? null : Number(status));
    };
    socket.setEncoding('latin1');
    socket.setTimeout(timeout, () => {
      socket.destroy();
      reject(new Error(`no answer in ${timeout} ms to ${j(head)}`));
    });
    socket.on('connect', () => socket.write(`${head}\r\n\r\n`));
    socket.on('data', (chunk) => {
      received += chunk;
      if (received.includes('\r\n')) done();
    });
    socket.on('end', done);
    socket.on('error', reject);
  });
}

/** Polls `fn` until it returns something truthy; the message of the timeout says what was awaited. */
export async function until(fn, what, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const value = await fn();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await sleep(25);
  }
}

/** The event stream `/events` as a viewer's browser reads it; every message is kept with its arrival time. */
export class EventStream {
  constructor() {
    this.messages = [];
    this.closed = false;
    this.failure = null;
  }

  /** Opens the stream with the session cookie; rejects unless the answer is 200. */
  static open(port, cookie, timeout = 5000) {
    return new Promise((resolve, reject) => {
      const stream = new EventStream();
      const timer = setTimeout(() => {
        stream.close();
        reject(new Error(`GET /events: no answer in ${timeout} ms`));
      }, timeout);
      const req = http.request(
        { host: '127.0.0.1', port, path: '/events', method: 'GET', agent: false, headers: { Cookie: cookie, Accept: 'text/event-stream' } },
        (res) => {
          clearTimeout(timer);
          stream.res = res;
          stream.status = res.statusCode;
          stream.headers = res.headers;
          if (res.statusCode !== 200) {
            res.resume();
            reject(new Error(`GET /events with the cookie answered ${res.statusCode}, expected 200`));
            return;
          }
          res.setEncoding('utf8');
          const parser = createSseParser((data) => {
            try {
              stream.messages.push({ i: stream.messages.length, at: stream.chunkAt, raw: data, msg: JSON.parse(data) });
            } catch {
              stream.failure = `a message is not JSON: ${data.slice(0, 120)}`;
            }
          });
          res.on('data', (chunk) => {
            stream.chunkAt = wallMs();
            parser.feed(chunk);
          });
          res.on('end', () => (stream.closed = true));
          res.on('close', () => (stream.closed = true));
          res.on('error', () => (stream.closed = true));
          resolve(stream);
        },
      );
      stream.req = req;
      req.on('error', (error) => {
        clearTimeout(timer);
        stream.closed = true;
        reject(error);
      });
      req.end();
    });
  }

  /** The first message at or after index `from` for which `predicate(msg, entry)` holds, within `timeout`. */
  async next(predicate, what, { from = 0, timeout = 10_000 } = {}) {
    return until(
      () => {
        if (this.failure) throw new Error(this.failure);
        return this.messages.find((m) => m.i >= from && predicate(m.msg, m));
      },
      what,
      timeout,
    );
  }

  /** Index to give as `from` to see only what arrives from now on. */
  mark() {
    return this.messages.length;
  }

  close() {
    this.closed = true;
    this.req?.destroy();
    this.res?.destroy();
  }
}

// --- A real browser ------------------------------------------------------------------------------------

/** Microsoft Edge on this PC (it ships with Windows 11), or null. */
export function findEdge() {
  const roots = [process.env['ProgramFiles(x86)'], process.env.ProgramFiles].filter(Boolean);
  return roots.map((root) => join(root, 'Microsoft', 'Edge', 'Application', 'msedge.exe')).find((path) => existsSync(path)) ?? null;
}

/** A TCP port nobody listens on (asked of the system, then released). */
function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/** What the console said (or an exception) that is about the page's policy or about WebAssembly. */
export const BROWSER_PROBLEM = /content security policy|\bcsp\b|refused to|webassembly|\.wasm/i;

/**
 * Headless Edge on a temporary profile, one page, driven over the DevTools protocol (same calls as the app's
 * window in `run.mjs`). `close()` kills the whole process tree and removes the profile: call it in a `finally`.
 * `extraArgs`: more command-line arguments of Edge (a journey that keeps a window over the screen asks for the ones that stop Chromium from pausing a page it thinks hidden).
 * `messages`: every console message of level error or warning, log entry and uncaught exception of the page.
 */
export async function startBrowser(exe, extraArgs = []) {
  const dir = mkdtempSync(join(tmpdir(), 'ardoise-e2e-edge-'));
  const port = await freePort();
  const child = spawn(
    exe,
    ['--headless=new', '--window-size=1280,480', `--remote-debugging-port=${port}`, '--remote-allow-origins=*', `--user-data-dir=${dir}`, '--no-first-run', '--no-default-browser-check', ...extraArgs, 'about:blank'],
    { stdio: 'ignore' },
  );
  let spawnError = null;
  child.once('error', (error) => (spawnError = error));
  let ws = null;
  let closed = false;
  const messages = [];
  const browser = {
    messages,
    async close() {
      if (closed) return;
      closed = true;
      try {
        ws?.close();
      } catch {
        // Already closed.
      }
      if (child.pid !== undefined && child.exitCode === null) {
        try {
          // Edge starts a process per tab, for the GPU...: the whole tree, or the profile stays locked.
          execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
        } catch {
          child.kill();
        }
      }
      for (let attempt = 0; attempt < 20; attempt++) {
        try {
          rmSync(dir, { recursive: true, force: true });
          return;
        } catch {
          await sleep(250);
        }
      }
    },
  };
  try {
    let target;
    for (let i = 0; i < 120 && !target; i++) {
      if (spawnError) throw spawnError;
      await sleep(250);
      try {
        const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        target = list.find((t) => t.type === 'page');
      } catch {
        // Not listening yet.
      }
    }
    if (!target) throw new Error('Edge did not open its DevTools endpoint');
    ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve);
      ws.addEventListener('error', () => reject(new Error('cannot connect to the DevTools endpoint of Edge')));
    });
    let id = 0;
    const pending = new Map();
    ws.addEventListener('message', (message) => {
      const data = JSON.parse(message.data);
      if (data.id !== undefined) {
        pending.get(data.id)?.(data);
        pending.delete(data.id);
      } else if (data.method === 'Runtime.consoleAPICalled') {
        const { type, args } = data.params;
        if (type === 'error' || type === 'warning') messages.push(`console.${type}: ${args.map((a) => a.value ?? a.description ?? '').join(' ')}`);
      } else if (data.method === 'Log.entryAdded') {
        const { level, source, text, url } = data.params.entry;
        if (level === 'error' || level === 'warning') messages.push(`log ${level} (${source}): ${text}${url ? ` ${url}` : ''}`);
      } else if (data.method === 'Debugger.paused') {
        browser.paused = data.params.callFrames.slice(0, 8).map((f) => `${f.functionName || '(anonymous)'} ${f.url.split('/').at(-1)}:${f.location.lineNumber + 1}`);
      } else if (data.method === 'Runtime.exceptionThrown') {
        const details = data.params.exceptionDetails;
        messages.push(`uncaught: ${details.exception?.description ?? details.text}`);
      }
    });
    const send = (method, params = {}) =>
      new Promise((resolve) => {
        const n = ++id;
        pending.set(n, resolve);
        ws.send(JSON.stringify({ id: n, method, params }));
      });
    await send('Runtime.enable');
    await send('Log.enable');
    await send('Page.enable');

    browser.send = send;
    browser.navigate = (url) => send('Page.navigate', { url });
    /** The page's own call stack, taken by interrupting it (for a page that does not answer): the top frames, or null. */
    browser.where = async () => {
      browser.paused = null;
      await send('Debugger.enable');
      await send('Debugger.pause');
      for (let k = 0; k < 40 && !browser.paused; k++) await sleep(100);
      return browser.paused;
    };
    browser.evaluate = async (expression) => {
      const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (reply.result?.exceptionDetails) throw new Error(`page error: ${reply.result.exceptionDetails.exception?.description ?? 'unknown'}`);
      return reply.result?.result?.value;
    };
    browser.waitFor = async (expression, what, timeout = 10_000) => {
      const deadline = Date.now() + timeout;
      for (;;) {
        const value = await browser.evaluate(expression).catch(() => undefined);
        if (value) return value;
        if (Date.now() > deadline) throw new Error(`browser: timed out waiting for ${what}`);
        await sleep(100);
      }
    };
    /** A real mouse click at the centre of the first element matching `selector`. */
    browser.click = async (selector) => {
      const box = await browser.evaluate(`(() => {
        const el = document.querySelector(${j(selector)});
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      })()`);
      if (!box) throw new Error(`browser: no element ${selector}`);
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: box.x, y: box.y, button: 'none', buttons: 0 });
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: box.x, y: box.y, button: 'left', buttons: 1, clickCount: 1 });
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: box.x, y: box.y, button: 'left', buttons: 0, clickCount: 1 });
    };
    browser.type = (text) => send('Input.insertText', { text });
    /** The mouse moves (a different point each time): the page shows its controls, which hide after 2 s without it. */
    let wiggle = 0;
    browser.wake = () => send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 3 + (wiggle++ % 5), y: 3, button: 'none', buttons: 0 });
    /** A real key press (the page listens to keydown on the window). */
    browser.press = async (key) => {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', key, text: key.length === 1 ? key : undefined });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key });
    };
    return browser;
  } catch (error) {
    await browser.close();
    throw error;
  }
}

/** Throws if the console of the browser mentioned the page's policy or WebAssembly, or if the page threw. */
export function assertNoBrowserProblem(browser) {
  const problems = browser.messages.filter((m) => BROWSER_PROBLEM.test(m) || m.startsWith('uncaught:'));
  if (problems.length > 0) throw new Error(`the browser reported: ${problems.slice(0, 3).join(' | ')}`);
}

/** The canvas of the viewer's sheet; `strokes` is a condition on `c.dataset.strokes`. */
const SHEET = 'canvas.viewer-canvas';
const sheetIs = (shown, strokes) =>
  `(() => { const c = document.querySelector(${j(SHEET)}); return c !== null && c.dataset.shown === ${j(shown)} && ${strokes}; })()`;
/**
 * Dark pixels on the canvas (a sample): the ink is black on the white paper. Only the geometry module
 * (WebAssembly) draws them, `data-strokes` counts the strokes whether they are drawn or not.
 */
const DARK_PIXELS = `(() => {
  const c = document.querySelector(${j(SHEET)});
  if (!c) return -1;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  let n = 0;
  for (let i = 0; i < d.length; i += 16) if (d[i] + d[i + 1] + d[i + 2] < 150) n++;
  return n;
})()`;

/**
 * Red pixels on the canvas (a sample): the marks written over the presentation are red (216, 0, 0) on a light
 * page, and the simulated pen writes in black.
 */
const RED_PIXELS = `(() => {
  const c = document.querySelector(${j(SHEET)});
  if (!c) return -1;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  let n = 0;
  for (let i = 0; i < d.length; i += 16) if (d[i] > 150 && d[i + 1] < 80 && d[i + 2] < 80) n++;
  return n;
})()`;

/** A wavy red mark across `page`, as the presentation window reports it (`LiveMark`: the ink and the points). */
export function wavyMark(page, y, n = 14) {
  return {
    tool: 'feltTip',
    color: { r: 216, g: 0, b: 0, a: 255 },
    width: page.height / 60,
    points: Array.from({ length: n }, (_, i) => ({ x: page.width * (0.1 + (0.8 * i) / (n - 1)), y: page.height * y + 300 * Math.sin(i), p: 0.7 })),
  };
}

/** The `marks` events of a message of the stream (several in a batch are possible). */
const marksOf = (msg) => (msg.type === 'live' ? msg.batch.events.filter((e) => e.type === 'marks') : []);

// --- Steps shared with the other live journeys ------------------------------------------------------------

/** Registers the simulated tablet (it starts unregistered), opens live mode from the tablet panel and waits for the first strokes. */
async function startLive(app, check) {
  const STATUS = '.tablet-status';
  const WIZARD = '.register-wizard';
  await check('enregistrer la tablette simulée', async () => {
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
    await app.click(STATUS);
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
    await app.click('.tablet-panel button[data-action="register"]');
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="warning"]`)}) !== null`, 'the warning step');
    await app.click(`${WIZARD} button[data-action="continue"]`);
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="done"]`)}) !== null`, 'the end of the registration', 60_000);
    await app.click(`${WIZARD} button[data-action="finish"]`);
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`, 'the end of the sync', 60_000);
  });
  await check('ouvrir le mode direct (tablette simulée)', async () => {
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
    await app.click('.tablet-panel button[data-action="live"]');
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
    await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 10_000);
  });
}

export const PANEL = '.share-panel';
const viewersAre = (n) => `document.querySelector('[data-share-viewers]')?.getAttribute('data-share-viewers') === '${n}'`;

/** Opens the share panel with the toolbar button (live window or editor). */
export async function openPanel(app) {
  if (await app.evaluate(`document.querySelector(${j(PANEL)}) !== null`)) return;
  await app.click('button[data-action="share"]');
  await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the share panel');
}

/**
 * Closes the share panel if it is open, so that it does not cover the toolbar. Assumption: a close button in
 * the panel, with `data-action="share-close"` or the label `common.close` (Esc only outside live mode, where it
 * would ask whether to end the live session). Never fails: a later click that hits the panel says so.
 */
export async function closePanel(app, closeLabel) {
  if (!(await app.evaluate(`document.querySelector(${j(PANEL)}) !== null`))) return;
  const selector = await app.evaluate(`(() => {
    const panel = document.querySelector(${j(PANEL)});
    const button =
      panel.querySelector('[data-action="share-close"]') ??
      [...panel.querySelectorAll('button')].find((b) => b.textContent.trim() === ${j(closeLabel)} || b.getAttribute('aria-label') === ${j(closeLabel)});
    if (!button) return null;
    button.setAttribute('data-e2e-close', '');
    return ${j(`${PANEL} [data-e2e-close]`)};
  })()`);
  if (selector !== null) await app.click(selector);
  else if (await app.evaluate(`document.querySelector('.live-view') === null`)) await app.key('Escape');
  try {
    await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close', 2000);
  } catch {
    console.log('    (le panneau de partage reste ouvert : il pourrait recouvrir la suite)');
  }
}

// --- The journey ----------------------------------------------------------------------------------------

export const shareJourneys = [
  {
    name: 'partage sur le réseau : feuille en direct, note de l’éditeur, pause, arrêt',
    args: ['--simulated-tablet'],
    // The server listens on 127.0.0.1 only, on a port of ours, so that a run never collides with another
    // and the firewall never asks anything.
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const port = app.sharePort;
      const origin = `http://127.0.0.1:${port}`;
      let code = '';
      let cookie = '';
      let stream;
      let browser = null;

      try {
        await startLive(app, check);

        await check('« Partager sur le réseau » : le panneau montre le code, l’adresse et 0 écran', async () => {
          await app.click('button[data-action="share"]');
          await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the share panel', 15_000);
          await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
          const shown = await app.evaluate(`(() => {
            const el = document.querySelector('[data-share-code]');
            return [el.textContent, el.getAttribute('data-share-code') ?? ''];
          })()`);
          const pattern = /^[A-HJKMNP-Z2-9]{3} [A-HJKMNP-Z2-9]{3}$/;
          code = shown.map((s) => s.replace(/[\s  ]+/g, ' ').trim()).find((s) => pattern.test(s));
          if (code === undefined) throw new Error(`the code ${j(shown)} is not shown as "K7M Q2X" (3 + 3 unambiguous characters)`);
          if (!(await app.evaluate(viewersAre(0)))) throw new Error('the panel does not show 0 screens yet');
          const address = await app.evaluate(`document.querySelector('[data-share-address]')?.textContent ?? null`);
          if (address === null) {
            console.log('    (aucune adresse réseau affichée : ce PC est-il relié à un réseau ?)');
          } else if (!address.includes(`:${port}`) || !address.includes('http://')) {
            throw new Error(`the address ${j(address)} does not give http://…:${port}`);
          }
        });

        await check('GET / : la page du visionneur (200, text/html, politique CSP)', async () => {
          const page = await request(port, { path: '/' });
          if (page.status !== 200) throw new Error(`GET / answered ${page.status}`);
          if (!String(page.headers['content-type']).startsWith('text/html')) throw new Error(`content type ${page.headers['content-type']}`);
          const csp = String(page.headers['content-security-policy'] ?? '');
          if (!csp.includes("default-src 'self'")) throw new Error(`missing or unexpected Content-Security-Policy: ${j(csp)}`);
          if (page.headers['x-content-type-options'] !== 'nosniff') throw new Error('missing X-Content-Type-Options: nosniff');
          if (page.body.length === 0) throw new Error('the page is empty');
        });

        await check('les fichiers cités par la page (script, feuille de style) : 200 avec le bon type', async () => {
          const html = (await request(port, { path: '/' })).body.toString('utf8');
          const { paths, foreign } = pageReferences(html, origin);
          if (foreign.length > 0) throw new Error(`the page refers to resources elsewhere: ${foreign.join(', ')}`);
          if (!paths.some((p) => p.endsWith('.js'))) throw new Error(`the page refers to no script: ${paths}`);
          const scripts = new Map();
          for (const path of paths) {
            const asset = await request(port, { path });
            if (asset.status !== 200) throw new Error(`GET ${path} answered ${asset.status}`);
            const type = String(asset.headers['content-type']);
            if (path.endsWith('.js')) {
              if (!/javascript/.test(type)) throw new Error(`${path}: content type ${type}`);
              scripts.set(path, asset.body.toString('utf8'));
            } else if (path.endsWith('.css') && !/text\/css/.test(type)) {
              throw new Error(`${path}: content type ${type}`);
            }
          }
          // The chunks the scripts import and the WebAssembly file (the stroke outlines) are part of the viewer too
          // (a Map visits the chunks added while it is walked).
          const wasm = new Set();
          for (const [path, js] of scripts) {
            const refs = scriptReferences(js);
            for (const chunk of refs.chunks) {
              if (scripts.has(chunk)) continue;
              const asset = await request(port, { path: chunk });
              if (asset.status !== 200) throw new Error(`${path} imports ${chunk}, which answers ${asset.status}`);
              if (!/javascript/.test(String(asset.headers['content-type']))) throw new Error(`${chunk}: content type ${asset.headers['content-type']}`);
              scripts.set(chunk, asset.body.toString('utf8'));
            }
            for (const file of refs.wasm) wasm.add(file);
          }
          if (wasm.size === 0) throw new Error('no script refers to assets/ardoise_ink-….wasm');
          for (const file of wasm) {
            const asset = await request(port, { path: file });
            if (asset.status !== 200) throw new Error(`GET ${file} answered ${asset.status}`);
            if (!String(asset.headers['content-type']).startsWith('application/wasm')) {
              throw new Error(`${file}: content type ${asset.headers['content-type']}, expected application/wasm`);
            }
            if (asset.body.subarray(0, 4).toString('latin1') !== '\0asm') throw new Error(`${file} is not a WebAssembly module`);
          }
        });

        await check('GET /events sans cookie : 401', async () => {
          const reply = await request(port, { path: '/events' });
          if (reply.status !== 401) throw new Error(`answered ${reply.status}, expected 401`);
        });

        await check('POST /join avec un mauvais code : 403 et le nombre d’essais restants', async () => {
          const wrong = code.replace(/\s/g, '') === 'ZZZZZZ' ? 'AAAAAA' : 'ZZZZZZ';
          const reply = await request(port, {
            method: 'POST',
            path: '/join',
            headers: { 'Content-Type': 'application/json' },
            body: j({ code: wrong }),
          });
          if (reply.status !== 403) throw new Error(`answered ${reply.status}, expected 403`);
          const body = JSON.parse(reply.body.toString('utf8'));
          if (body.error !== 'wrongCode' || !Number.isInteger(body.remaining) || body.remaining < 0 || body.remaining > 5) {
            throw new Error(`unexpected body ${j(body)}`);
          }
          if (reply.headers['set-cookie']) throw new Error('a wrong code received a cookie');
        });

        await check('POST /join avec le bon code (en minuscules, sans espace) : 200 et un cookie HttpOnly', async () => {
          const reply = await request(port, {
            method: 'POST',
            path: '/join',
            headers: { 'Content-Type': 'application/json' },
            body: j({ code: code.replace(/\s/g, '').toLowerCase() }),
          });
          if (reply.status !== 200) throw new Error(`answered ${reply.status}, expected 200 (${reply.body.toString('utf8').slice(0, 120)})`);
          const set = [reply.headers['set-cookie'] ?? []].flat().join('\n');
          if (!/HttpOnly/i.test(set)) throw new Error(`the cookie is not HttpOnly: ${j(set)}`);
          cookie = set.split(';')[0].trim();
          if (!/^[\w-]+=\S+$/.test(cookie)) throw new Error(`unexpected cookie ${j(set)}`);
        });

        await check('tout autre chemin répond 404 (jamais 200) : fichiers, bibliothèque, remontées, autres méthodes', async () => {
          // `lenient`: a request target that a strict parser may refuse as malformed (400) rather than not find.
          const paths = [
            { m: 'GET', p: '/index.html' },
            { m: 'GET', p: '/viewer.html' },
            { m: 'GET', p: '/ardoise.toml' },
            { m: 'GET', p: '/assets/../ardoise.toml' },
            { m: 'GET', p: '/assets/%2e%2e/ardoise.toml' },
            { m: 'GET', p: '/..%2fardoise.toml' },
            { m: 'GET', p: '//assets/x' },
            { m: 'GET', p: '/assets\\x', lenient: true },
            { m: 'GET', p: '/ASSETS/x' },
            { m: 'GET', p: '/assets/' },
            { m: 'GET', p: '/assets' },
            { m: 'GET', p: '/events/' },
            { m: 'GET', p: '/_ardoise/device.json' },
            { m: 'GET', p: '/Biblioth%C3%A8que' },
            { m: 'GET', p: '/Biblioth%C3%A8que/' },
            { m: 'GET', p: '/library' },
            { m: 'GET', p: '/join' },
            { m: 'POST', p: '/' },
          ];
          const wrong = [];
          for (const { m, p, lenient } of paths) {
            const body = m === 'POST' ? '\r\nContent-Length: 0' : '';
            const status = await rawStatus(port, `${m} ${p} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close${body}`);
            if (status !== 404 && !(lenient && status === 400)) wrong.push(`${m} ${p} → ${status}`);
          }
          if (wrong.length > 0) throw new Error(`expected 404: ${wrong.join('; ')}`);
        });

        await check('le flux /events avec le cookie : d’abord un instantané du direct, sans rien de personnel', async () => {
          stream = await EventStream.open(port, cookie);
          if (!String(stream.headers['content-type']).startsWith('text/event-stream')) throw new Error(`content type ${stream.headers['content-type']}`);
          const first = await stream.next(() => true, 'the first message', { timeout: 5000 });
          const { msg } = first;
          if (msg.type !== 'live' || msg.reset !== true) throw new Error(`the first message is ${j(msg).slice(0, 160)}, expected {type:'live', reset:true, …}`);
          const snapshot = msg.batch?.events?.[0];
          if (snapshot?.type !== 'snapshot') throw new Error(`the first batch starts with ${snapshot?.type}, expected a snapshot`);
          const { saved, presentation, page } = snapshot.snapshot;
          if (!Array.isArray(saved) || saved.length !== 0) throw new Error(`the snapshot lists saved notes: ${j(saved)}`);
          if (presentation !== null) throw new Error(`the snapshot names the projection screen: ${j(presentation)}`);
          if (page?.title !== '') throw new Error(`the page of the snapshot has a title: ${j(page?.title)}`);
          if (!Array.isArray(page.strokes)) throw new Error('the page of the snapshot has no strokes');
        });

        await check('le panneau compte 1 écran connecté', async () => {
          await app.waitFor(viewersAre(1), 'the panel to show 1 screen', 5000);
        });

        await check('le tracé de la tablette arrive : délai médian < 50 ms (date de réception dans l’app → arrivée dans Node)', async () => {
          await stream.next(
            (msg) => msg.type === 'live' && !msg.reset && msg.batch.events.some((e) => e.type === 'points' || e.type === 'strokeStart'),
            'points of the simulated pen',
            { timeout: 10_000 },
          );
          const delays = await until(() => {
            const all = pointDelays(stream.messages);
            return all.length >= 60 ? all : null;
          }, 'at least 60 points (is the simulated pen writing?)', 30_000);
          const median = quantile(delays, 0.5);
          const p95 = quantile(delays, 0.95);
          console.log(`    délai de ${delays.length} points : médiane ${median.toFixed(1)} ms, p95 ${p95.toFixed(1)} ms`);
          if (!(median < 50)) throw new Error(`median delay ${median.toFixed(1)} ms (p95 ${p95.toFixed(1)} ms), expected less than 50 ms`);
        });

        await check('vrai navigateur (Edge sans fenêtre) : la page demande le code, puis dessine le direct sans erreur de politique ni de WebAssembly', async () => {
          const edge = findEdge();
          if (!edge) {
            console.log('    skipped: Edge not found (C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe)');
            return;
          }
          browser = await startBrowser(edge);
          await browser.navigate(`${origin}/`);
          await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'code'`, 'the code form', 30_000);
          await browser.click('#viewer-code');
          await browser.type(code);
          await browser.waitFor(`document.querySelector('form button[type="submit"]')?.disabled === false`, 'the join button');
          await browser.click('form button[type="submit"]');
          await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'live'`, 'the live sheet', 20_000);
          await browser.waitFor(sheetIs('live', 'Number(c.dataset.strokes) > 0'), 'strokes on the canvas of the viewer', 20_000);
          const strokes = Number(await browser.evaluate(`document.querySelector(${j(SHEET)}).dataset.strokes`));
          const before = await browser.evaluate(DARK_PIXELS);
          // The simulated pen keeps writing: one more stroke, and more ink drawn.
          await browser.waitFor(`Number(document.querySelector(${j(SHEET)})?.dataset.strokes) > ${strokes}`, 'one more stroke', 30_000);
          await browser.waitFor(`${DARK_PIXELS} > ${before}`, 'more ink on the canvas (is the geometry module loaded?)', 10_000);
          assertNoBrowserProblem(browser);
        });

        // The viewer's own display (docs/design/lan-share.md, "The viewer"): purely local, the sheet drawn is the one of the PC.
        const geometry = () =>
          browser.evaluate(`(() => {
            const c = document.querySelector(${j(SHEET)});
            return c ? { fit: c.dataset.fit, zoom: Number(c.dataset.zoom), scale: Number(c.dataset.scale), width: c.clientWidth } : null;
          })()`);
        const fitIs = (fit) => `document.querySelector(${j(SHEET)})?.dataset.fit === ${j(fit)}`;
        /** Clicks a control of the viewer's bar, which hides with the cursor: wake it first. */
        const control = async (action) => {
          await browser.wake();
          await browser.waitFor(`document.querySelector('.controls [data-action="${action}"]') !== null`, `the "${action}" control`, 5000);
          await browser.click(`.controls [data-action="${action}"]`);
        };

        await check('vrai navigateur : pleine largeur, zoom, page entière, puis « Suivre le PC » rend la vue du PC (direct)', async () => {
          if (!browser) return;
          const pageWidth = stream.messages[0].msg.batch.events[0].snapshot.page.page.width;
          if (!(pageWidth > 0)) throw new Error('no page width in the first snapshot');
          const followed = await geometry();
          if (followed.fit !== 'follow') throw new Error(`the viewer starts with fit "${followed.fit}", expected "follow"`);
          await browser.wake();
          if (await browser.evaluate(`document.querySelector('.controls [data-action="follow"]') !== null`)) throw new Error('« Suivre le PC » is shown before any local change');

          // Full width: the page across the window (the view of the PC is left).
          await control('fit-width');
          await browser.waitFor(fitIs('width'), 'the full width fit');
          const wide = await geometry();
          if (Math.abs(wide.scale * pageWidth - wide.width) > 1.5) throw new Error(`full width: the page is ${wide.scale * pageWidth} px wide in a ${wide.width} px window`);
          // Zoom in: the page is drawn larger, as its own view.
          await control('zoom-in');
          await browser.waitFor(fitIs('free'), 'a free zoom');
          const zoomed = await geometry();
          if (!(zoomed.scale > wide.scale * 1.1)) throw new Error(`zoom in: scale ${wide.scale} then ${zoomed.scale}`);
          // Whole page: all of it inside the window.
          await control('fit-page');
          await browser.waitFor(fitIs('page'), 'the whole page fit');
          const whole = await geometry();
          if (!(whole.scale * pageWidth <= whole.width + 1.5) || !(whole.scale <= wide.scale + 1e-9)) throw new Error(`whole page: scale ${whole.scale}, window ${whole.width} px`);
          // The window is much wider than tall (see startBrowser): the whole page leaves room at the sides.
          if (!(wide.scale > whole.scale * 1.05)) throw new Error(`full width (${wide.scale}) is not larger than the whole page (${whole.scale})`);
          // The same by keyboard.
          await browser.press('w');
          await browser.waitFor(fitIs('width'), 'the full width fit by the W key');
          await browser.press('0');
          await browser.waitFor(fitIs('page'), 'the whole page by the 0 key');

          // The PC's view again.
          await control('follow');
          await browser.waitFor(fitIs('follow'), 'the PC view followed again');
          const back = await geometry();
          if (Math.abs(back.zoom - followed.zoom) > 1e-3 || Math.abs(back.scale - followed.scale) > followed.scale * 1e-3) {
            throw new Error(`« Suivre le PC »: zoom ${followed.zoom} then ${back.zoom}, scale ${followed.scale} then ${back.scale}`);
          }
          await browser.wake();
          if (await browser.evaluate(`document.querySelector('.controls [data-action="follow"]') !== null`)) throw new Error('« Suivre le PC » stays after following the PC again');
          assertNoBrowserProblem(browser);
        });

        let page = { width: 29700, height: 21600 };
        let redBefore = 0;
        // What the presentation window does when the teacher writes over the page, without a projector: it reports the marks.
        const invoke = (command, args) => app.evaluate(`window.__TAURI_INTERNALS__.invoke(${j(command)}, ${j(args)}).then(() => true)`);
        const livePage = async () => Number((await app.evaluate(canvasData('page'))).split('/')[0]) - 1;

        await check('marques sur la présentation : le flux reçoit la liste entière, avec seulement ce qui se dessine', async () => {
          const size = stream.messages[0].msg.batch.events[0].snapshot.page.page;
          if (!(size?.width > 0 && size?.height > 0)) throw new Error(`no page size in the first snapshot: ${j(size)}`);
          page = size;
          redBefore = browser ? await browser.evaluate(RED_PIXELS) : 0;
          const mark = stream.mark();
          const pageIndex = await livePage();
          await invoke('live_set_marks', { pageIndex, marks: [wavyMark(page, 0.3)] });
          const first = await stream.next((msg) => marksOf(msg).some((e) => e.marks.length === 1), 'a marks event with one mark', { from: mark, timeout: 5000 });
          const event = marksOf(first.msg).find((e) => e.marks.length === 1);
          const keys = (o) => Object.keys(o).sort().join();
          if (keys(event) !== 'marks,type') throw new Error(`the marks event has the keys ${keys(event)}`);
          if (keys(event.marks[0]) !== 'color,points,tool,width') throw new Error(`a mark has the keys ${keys(event.marks[0])}`);
          if (keys(event.marks[0].points[0]) !== 'p,x,y') throw new Error(`a mark point has the keys ${keys(event.marks[0].points[0])}`);
          if (event.marks[0].points.length !== 14 || event.marks[0].tool !== 'feltTip') throw new Error(`unexpected mark ${j(event.marks[0]).slice(0, 160)}`);
          // The whole list each time: a second mark comes with the first.
          const marksAt = stream.mark();
          await invoke('live_set_marks', { pageIndex, marks: [wavyMark(page, 0.3), wavyMark(page, 0.6)] });
          await stream.next((msg) => marksOf(msg).some((e) => e.marks.length === 2), 'a marks event with two marks', { from: marksAt, timeout: 5000 });
          // A viewer arriving now starts from them: its first message is the snapshot, then the marks.
          const late = await EventStream.open(port, cookie);
          try {
            const joined = await late.next((msg) => msg.type === 'live' && msg.reset === true, 'the first message of a late viewer', { timeout: 5000 });
            const events = joined.msg.batch.events;
            if (events[0]?.type !== 'snapshot') throw new Error(`the late viewer's batch starts with ${events[0]?.type}`);
            if (!marksOf(joined.msg).some((e) => e.marks.length === 2)) throw new Error('the late viewer does not get the two marks with its snapshot');
          } finally {
            late.close();
          }
        });

        await check('vrai navigateur : les marques ajoutent de l’encre rouge sur la feuille du visionneur', async () => {
          if (!browser) return;
          await browser.waitFor(`${RED_PIXELS} > ${redBefore}`, 'red ink on the canvas of the viewer (the marks)', 10_000);
          assertNoBrowserProblem(browser);
        });

        await check('changement de page : le flux reçoit une liste vide, et plus aucune marque de la page quittée', async () => {
          const pageIndex = await livePage();
          const mark = stream.mark();
          await invoke('live_command', { command: { kind: 'nextPage' } });
          const cleared = await stream.next((msg) => marksOf(msg).some((e) => e.marks.length === 0), 'an empty marks event after the page change', { from: mark, timeout: 5000 });
          // It follows the snapshot of the new page, in the same message or after it.
          const snapshot = stream.messages.find(
            (m) => m.i >= mark && m.msg.type === 'live' && m.msg.batch.events.some((e) => e.type === 'snapshot' && e.snapshot.pageIndex === pageIndex + 1),
          );
          if (!snapshot || snapshot.i > cleared.i) throw new Error(`the snapshot of page ${pageIndex + 2} did not come before the empty marks (message ${cleared.i})`);
          // Marks sent for the page just left are refused.
          const after = stream.mark();
          await invoke('live_set_marks', { pageIndex, marks: [wavyMark(page, 0.4)] });
          await sleep(500);
          if (stream.messages.some((m) => m.i >= after && marksOf(m.msg).some((e) => e.marks.length > 0))) {
            throw new Error('marks written for the page just left reached the viewers');
          }
          if (browser) {
            await browser.waitFor(`${RED_PIXELS} <= ${redBefore}`, 'the red ink to go with the page', 10_000);
          }
          // Back to the first page, as the later steps found it.
          await invoke('live_command', { command: { kind: 'previousPage' } });
          await app.waitFor(`${canvasData('page')}.startsWith(${j(`${pageIndex + 1}/`)})`, 'the first page again');
        });

        await check('Pause : l’image figée (instantané) puis « pause », puis plus aucun point pendant 1 s', async () => {
          const mark = stream.mark();
          await app.click('button[data-action="share-pause"]');
          const paused = await stream.next((msg) => msg.type === 'pause' && msg.paused === true, 'the pause message', { from: mark, timeout: 5000 });
          const frozen = stream.messages.find((m) => m.i >= mark && m.i < paused.i && m.msg.type === 'live' && m.msg.reset === true);
          if (!frozen) throw new Error('no frozen batch ({type:"live", reset:true}) before the pause message');
          if (frozen.msg.batch.events[0]?.type !== 'snapshot') throw new Error('the frozen batch does not start with a snapshot');
          await sleep(1000);
          const moving = stream.messages.filter(
            (m) => m.i > paused.i && m.msg.type === 'live' && m.msg.batch.events.some((e) => e.type === 'points' || e.type === 'strokeStart'),
          );
          if (moving.length > 0) throw new Error(`${moving.length} messages with points arrived during the pause`);
        });

        await check('Reprendre : « pause » false, puis un nouvel instantané et le tracé qui reprend', async () => {
          const mark = stream.mark();
          await app.click('button[data-action="share-pause"]');
          const resumed = await stream.next((msg) => msg.type === 'pause' && msg.paused === false, 'the resume message', { from: mark, timeout: 5000 });
          const again = await stream.next((msg) => msg.type === 'live' && msg.reset === true, 'a new snapshot after the resume', { from: resumed.i, timeout: 5000 });
          if (again.msg.batch.events[0]?.type !== 'snapshot') throw new Error('the batch after the resume does not start with a snapshot');
          await stream.next(
            (msg) => msg.type === 'live' && !msg.reset && msg.batch.events.some((e) => e.type === 'points' || e.type === 'strokeStart'),
            'points after the resume',
            { from: again.i + 1, timeout: 15_000 },
          );
        });

        let title = '';
        let noteStrokes = 0;
        let editorMark = 0;
        await check('quitter le mode direct : le flux annonce l’attente (bibliothèque), puis la note ouverte dans l’éditeur', async () => {
          await closePanel(app, l('common.close'));
          editorMark = stream.mark();
          await app.click('button[data-action="stop-live"]');
          await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 15_000);
          await stream.next((msg) => msg.type === 'waiting', 'the waiting message (library)', { from: editorMark, timeout: 5000 });
          await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the notes of the library');
          const waitingAt = stream.mark();
          await app.click('.note-card .card-open');
          await app.waitFor(`${strokeCount} > 0`, 'the strokes of the note');
          // A title worth searching for in the stream (the demo note may have none): typed in the editor's field, which also renames the file.
          await app.click('.title-field input');
          await app.key('a', { ctrl: true });
          await app.type(SECRET_TITLE);
          await app.key('Enter');
          await app.waitFor(`document.querySelector('.editor').dataset.path.includes(${j(SECRET_TITLE)})`, 'the renamed note');
          await app.waitFor(`${strokeCount} > 0`, 'the strokes of the renamed note');
          // The page is fitted once the editor has measured its stage.
          await sleep(300);
          noteStrokes = await app.evaluate(strokeCount);
          title = (await app.evaluate(`document.querySelector('.title-field input')?.value ?? ''`)).trim();
          if (title !== SECRET_TITLE) throw new Error(`the title field holds ${j(title)}, expected ${j(SECRET_TITLE)}`);
          const note = await stream.next((msg) => msg.type === 'note', 'the note message', { from: waitingAt, timeout: 5000 });
          const shared = note.msg.page;
          if (!shared || !Array.isArray(shared.strokes)) throw new Error(`the note message has no page: ${note.raw.slice(0, 160)}`);
          if (shared.strokes.length !== noteStrokes) throw new Error(`the note message has ${shared.strokes.length} strokes, the editor shows ${noteStrokes}`);
          if (note.msg.background !== null && !String(note.msg.background).startsWith('data:image/png')) {
            throw new Error(`the background is neither null nor a PNG data URL: ${String(note.msg.background).slice(0, 40)}`);
          }
        });

        await check('tracer un trait dans l’éditeur : un message « noteStrokes » avec un trait de plus, en moins de 3 s', async () => {
          const mark = stream.mark();
          const box = await app.evaluate(`(() => { const r = document.querySelector('.editor svg.canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
          const points = Array.from({ length: 25 }, (_, i) => ({
            x: box.x + box.w * (0.3 + 0.4 * (i / 24)),
            y: box.y + box.h * (0.5 + 0.08 * Math.sin(i / 3)),
          }));
          await app.drag(points);
          const released = wallMs();
          await app.waitFor(`${strokeCount} === ${noteStrokes + 1}`, 'the new stroke in the editor');
          const update = await stream.next(
            (msg) =>
              (msg.type === 'noteStrokes' && msg.strokes.length === 1) || (msg.type === 'note' && msg.page.strokes.length === noteStrokes + 1),
            'a message with the new stroke (noteStrokes with one stroke, or note with one more stroke)',
            { from: mark, timeout: 3000 },
          );
          console.log(`    trait reçu ${Math.round(update.at - released)} ms après le relâchement (${update.msg.type})`);
          const added = update.msg.type === 'noteStrokes' ? update.msg.strokes[0] : update.msg.page.strokes.at(-1);
          if (!Array.isArray(added.points) || added.points.length < 2) throw new Error('the new stroke has no points');
        });

        await check('vrai navigateur : la note de l’éditeur remplace le direct (ses traits, avec le trait de plus)', async () => {
          if (!browser) return;
          await browser.waitFor(sheetIs('note', `Number(c.dataset.strokes) === ${noteStrokes + 1}`), `the note with ${noteStrokes + 1} strokes`, 15_000);
          if ((await browser.evaluate(`document.querySelector('main.viewer').dataset.paused`)) !== 'false') {
            throw new Error('the viewer shows the pause badge');
          }
          assertNoBrowserProblem(browser);
        });

        await check('vrai navigateur : la note de l’éditeur s’affiche en page entière, pleine largeur et page entière au choix, sans « Suivre le PC »', async () => {
          if (!browser) return;
          await browser.waitFor(fitIs('page'), 'the whole page by default for a note');
          const noteWidth = await geometry();
          await control('fit-width');
          await browser.waitFor(fitIs('width'), 'the full width fit of the note');
          const wide = await geometry();
          if (!(wide.scale > noteWidth.scale * 1.05)) throw new Error(`note: scale ${noteWidth.scale} (whole page) then ${wide.scale} (full width)`);
          if (await browser.evaluate(`document.querySelector('.controls [data-action="follow"]') !== null`)) throw new Error('« Suivre le PC » is shown for a note');
          await control('fit-page');
          await browser.waitFor(fitIs('page'), 'the whole page of the note again');
          const again = await geometry();
          if (Math.abs(again.scale - noteWidth.scale) > noteWidth.scale * 1e-3) throw new Error(`note: scale ${noteWidth.scale} then ${again.scale} at the whole page`);
          assertNoBrowserProblem(browser);
        });

        await check('rien de personnel dans le flux : ni dossier de la bibliothèque, ni titre de la note, ni événement « saved »', async () => {
          if (stream.failure) throw new Error(stream.failure);
          const secrets = [
            app.library,
            app.library.replaceAll('\\', '\\\\'),
            app.library.replaceAll('\\', '/'),
            basename(app.library),
            basename(app.dir),
            app.dir,
          ];
          if (title.length >= 3) secrets.push(title, j(title).slice(1, -1));
          const all = stream.messages.map((m) => m.raw).join('\n');
          const leaked = secrets.filter((s) => s.length > 0 && all.includes(s));
          if (leaked.length > 0) throw new Error(`the stream contains ${j(leaked)}`);
          for (const { msg, i } of stream.messages) {
            for (const event of msg.type === 'live' ? msg.batch.events : []) {
              if (event.type === 'saved' || event.type === 'saveFailed') throw new Error(`message ${i} carries a "${event.type}" event`);
              if (event.type === 'snapshot' && (event.snapshot.saved.length !== 0 || event.snapshot.presentation !== null || event.snapshot.page.title !== '')) {
                throw new Error(`message ${i}: a snapshot carries saved notes, the screen or a title`);
              }
            }
          }
          if (title.length < 3) console.log(`    (titre « ${title} » trop court pour être cherché dans le flux)`);
        });

        await check('Arrêter le partage : le flux reçoit « end » et se ferme', async () => {
          await openPanel(app);
          const mark = stream.mark();
          await app.click('button[data-action="share-stop"]');
          await stream.next((msg) => msg.type === 'end', 'the end message', { from: mark, timeout: 5000 });
          await until(() => stream.closed, 'the stream to close', 5000);
          if (browser) {
            await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'ended'`, 'the viewer to say that sharing ended', 10_000);
          }
        });

        await check('le port est fermé : connexion refusée, GET / échoue', async () => {
          const code = await until(
            () =>
              new Promise((resolve) => {
                const socket = net.connect({ host: '127.0.0.1', port });
                socket.once('connect', () => {
                  socket.destroy();
                  resolve(null);
                });
                socket.once('error', (error) => resolve(error.code ?? String(error)));
              }),
            'the port to refuse connections',
            // Windows retries a refused connection for about 2 s before reporting it.
            10_000,
          );
          if (code !== 'ECONNREFUSED') throw new Error(`connecting to the port failed with ${code}, expected ECONNREFUSED`);
          const reached = await request(port, { path: '/', timeout: 2000 }).then(
            (reply) => `answered ${reply.status}`,
            () => null,
          );
          if (reached !== null) throw new Error(`GET / ${reached} after the share stopped`);
        });
      } finally {
        stream?.close();
        // Edge and its profile, also when a step failed.
        await browser?.close();
      }
    },
  },
  {
    name: 'partage sur le réseau : port déjà pris, le partage prend le suivant et le retient',
    args: ['--simulated-tablet'],
    // The port of `[share]` is held by a Node server (as the Python helper of the user's PC held 8765), on 127.0.0.1
    // like the app's listener here: Windows refuses the second socket on that address and port.
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const wanted = app.sharePort;
      const settingsFile = join(app.dir, 'ardoise.toml');
      let held = null;
      const release = () =>
        new Promise((resolve) => {
          const server = held;
          held = null;
          if (server) server.close(() => resolve());
          else resolve();
        });
      /** The `share_port` written in the table of this machine in ardoise.toml, or null. The key is
       * bare when the machine's name allows it (`[machines.PC]`), quoted otherwise. */
      const keptPort = () => {
        const file = readFileSync(settingsFile, 'utf8');
        const table = /\[machines\.(?:"[^"]+"|[\w-]+)\]\r?\n((?:[^[\r\n].*\r?\n?)*)/.exec(file);
        const match = table && /^share_port\s*=\s*(\d+)\s*$/m.exec(table[1]);
        return match ? Number(match[1]) : null;
      };

      try {
        await startLive(app, check);

        await check('un autre programme tient le port du partage (serveur Node sur 127.0.0.1)', async () => {
          held = await new Promise((resolve, reject) => {
            const server = net.createServer((socket) => socket.destroy());
            server.once('error', (error) => reject(new Error(`cannot hold port ${wanted}: ${error.message}`)));
            server.listen(wanted, '127.0.0.1', () => resolve(server));
          });
        });

        let found = 0;
        await check('« Partager sur le réseau » : le partage démarre sur un autre port et le dit', async () => {
          await app.click('button[data-action="share"]');
          await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the share panel', 15_000);
          await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
          if (await app.evaluate(`document.querySelector('[data-share-error]') !== null`)) {
            throw new Error('the panel shows an error instead of taking another port');
          }
          found = await app.waitFor(shownPortExpression(), 'an address in the panel', 15_000);
          if (found === wanted) throw new Error(`the share took port ${wanted}, which another program holds`);
          if (found < wanted || found > wanted + 20) throw new Error(`port ${found} is not one of the 20 following ${wanted}`);
          const notice = await app.evaluate(`document.querySelector('[data-share-port-moved]')?.textContent?.trim() ?? null`);
          const expected = l('share.portMoved').replace('{requested}', String(wanted)).replace('{port}', String(found));
          if (notice !== expected) throw new Error(`the notice is ${j(notice)}, expected ${j(expected)}`);
        });

        await check('GET / répond sur le port trouvé', async () => {
          const page = await request(found, { path: '/' });
          if (page.status !== 200) throw new Error(`GET / on port ${found} answered ${page.status}`);
          if (!String(page.headers['content-type']).startsWith('text/html')) throw new Error(`content type ${page.headers['content-type']}`);
        });

        await check('ardoise.toml retient ce port dans la section de ce PC, sans toucher à [share]', async () => {
          await until(() => keptPort() === found, `share_port = ${found} in the machine's table of ardoise.toml`, 5000);
          const file = readFileSync(settingsFile, 'utf8');
          if (!new RegExp(`\\[share\\]\\r?\\nport\\s*=\\s*${wanted}\\b`).test(file)) {
            throw new Error(`[share] port is no longer ${wanted}:\n${file}`);
          }
        });

        await check('Arrêter le partage puis le relancer : même port, plus d’avis', async () => {
          await app.click('button[data-action="share-stop"]');
          await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the panel to close', 5000);
          await until(
            () =>
              new Promise((resolve) => {
                const socket = net.connect({ host: '127.0.0.1', port: found });
                socket.once('connect', () => {
                  socket.destroy();
                  resolve(false);
                });
                socket.once('error', () => resolve(true));
              }),
            `port ${found} to close`,
            10_000,
          );
          await app.click('button[data-action="share"]');
          await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
          const again = await app.waitFor(shownPortExpression(), 'an address in the panel', 15_000);
          if (again !== found) throw new Error(`the second share is on port ${again}, expected the port kept, ${found}`);
          if (await app.evaluate(`document.querySelector('[data-share-port-moved]') !== null`)) {
            throw new Error('the notice is still shown although the port asked for is the one kept');
          }
          const page = await request(found, { path: '/' });
          if (page.status !== 200) throw new Error(`GET / on port ${found} answered ${page.status}`);
        });

        await check('Arrêter le partage : le port trouvé est fermé, l’autre programme garde le sien', async () => {
          await app.click('button[data-action="share-stop"]');
          await until(
            () =>
              new Promise((resolve) => {
                const socket = net.connect({ host: '127.0.0.1', port: found });
                socket.once('connect', () => {
                  socket.destroy();
                  resolve(false);
                });
                socket.once('error', () => resolve(true));
              }),
            `port ${found} to close`,
            10_000,
          );
          if (held === null || !held.listening) throw new Error(`the other program lost port ${wanted}`);
        });
      } finally {
        await release();
      }
    },
  },
];

/** Page expression: the port of the panel's first address, or null (for `app.waitFor`). */
function shownPortExpression() {
  return `(() => {
    const match = /:(\\d+)\\/?$/.exec(document.querySelector('[data-share-address]')?.textContent ?? '');
    return match ? Number(match[1]) : null;
  })()`;
}
