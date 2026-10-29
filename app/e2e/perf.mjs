// Performance harness of `cargo xtask perf`: measures the start, a library of 1 000 notes and the live mode on the
// production executable, a portable copy of it in a temporary folder, driven through WebView2's DevTools protocol like
// the end-to-end journeys (`run.mjs`).
//
// Usage: node app/e2e/perf.mjs <path to ardoise.exe> [--only start,library,live] [--notes 1000]
//
// Progress goes to stderr; the last line of stdout is `PERF-JSON {...}`: the raw measures (times in milliseconds, frame
// intervals one by one). `xtask/src/perf.rs` computes the statistics, judges them against SPEC §8 and prints the table.
//
// - start: wall-clock time from the spawn of the process to the `ardoise:library-listed` mark (lib/perf.ts), set once
//   the grid of a demo library is drawn. Cold = a fresh WebView2 profile (LOCALAPPDATA of the run); warm = the same
//   profile, started again.
// - library: the same for a library of 1 000 notes, plus the time until the thumbnails on screen are drawn
//   (`data-drawn-at` of each `img`, lib/perf.ts) and the frame intervals (`requestAnimationFrame`) of a scripted
//   scroll. First open = empty thumbnail cache; second open = the cache filled by the first.
// - live: the frame intervals while the simulated tablet (`--simulated-tablet`) streams handwriting at 200 samples
//   per second to the live view.
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const exe = argv[0]?.startsWith('--') ? undefined : argv[0];
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
if (!exe) {
  console.error('usage: node app/e2e/perf.mjs <ardoise.exe> [--only start,library,live] [--notes 1000]');
  process.exit(2);
}
const only = option('--only', '').split(',').filter(Boolean);
const wants = (part) => only.length === 0 || only.includes(part);
const NOTES = Number(option('--notes', '1000'));
const WARM_RUNS = 3;
/** Scripted scroll of the library: speed (a fast flick of the wheel) and duration. */
const SCROLL_PX_PER_SECOND = 3000;
const SCROLL_MS = 6000;
/** Live mode: time left to settle, then time measured. */
const LIVE_WARMUP_MS = 2000;
const LIVE_MEASURE_MS = 8000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (...parts) => console.error(...parts);
const root = mkdtempSync(join(tmpdir(), 'ardoise-perf-'));
let nextPort = Number(process.env.ARDOISE_E2E_PORT) || 9400 + 100 * Math.floor(Math.random() * 40);

/** A portable copy of the exe in its own folder, with `ardoise.toml` pointing this PC to `library`. */
function portableCopy(name, library) {
  const dir = join(root, name);
  mkdirSync(dir, { recursive: true });
  const appExe = join(dir, 'ardoise.exe');
  copyFileSync(exe, appExe);
  const machine = process.env.COMPUTERNAME ?? 'PC';
  writeFileSync(join(dir, 'ardoise.toml'), `language = "fr"\n\n[machines."${machine}"]\nlibrary = '${library}'\n`);
  return appExe;
}

/** A library of `notes` notes made by the exe itself (the 8 demo notes when `notes` is 0). */
function makeLibrary(name, notes) {
  const library = join(root, name);
  const started = Date.now();
  execFileSync(exe, ['--create-demo-library', library, ...(notes > 0 ? ['--demo-notes', String(notes)] : [])], { stdio: 'pipe' });
  log(`  library ${name}: ${notes || 8} notes made in ${((Date.now() - started) / 1000).toFixed(1)} s`);
  return library;
}

/**
 * Starts the app and connects to its main window. `spawnedAt` is the wall clock just before the spawn.
 * `local` is the LOCALAPPDATA of the run: the WebView2 profile and the thumbnail cache live in it.
 */
async function startOnce({ appExe, local, args = [] }) {
  const port = nextPort++;
  const spawnedAt = Date.now();
  const child = spawn(appExe, args, {
    env: {
      ...process.env,
      LOCALAPPDATA: local,
      // The window must not be throttled when other windows hide it: the frame rates are those of the interface.
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding`,
    },
    stdio: 'ignore',
  });
  let target;
  for (let i = 0; i < 800 && !target; i++) {
    await sleep(50);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === 'page' && !t.url.includes('view=presentation'));
    } catch {
      // Not listening yet.
    }
  }
  if (!target) {
    child.kill();
    throw new Error('the window did not open its DevTools endpoint');
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve);
    ws.addEventListener('error', reject);
  });
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', (message) => {
    const data = JSON.parse(message.data);
    pending.get(data.id)?.(data);
    pending.delete(data.id);
  });
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const app = {
    spawnedAt,
    async evaluate(expression) {
      const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (reply.result?.exceptionDetails) {
        throw new Error(`page error: ${reply.result.exceptionDetails.exception?.description ?? 'unknown'}`);
      }
      return reply.result?.result?.value;
    },
    async waitFor(expression, what, timeout = 20_000) {
      const deadline = Date.now() + timeout;
      for (;;) {
        const value = await app.evaluate(expression).catch(() => undefined);
        if (value) return value;
        if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
        await sleep(50);
      }
    },
    async click(selector) {
      const box = await app.evaluate(`(() => {
        const el = document.querySelector(${JSON.stringify(selector)});
        if (!el) return null;
        el.scrollIntoView({ block: 'center', inline: 'center' });
        const r = el.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      })()`);
      if (!box) throw new Error(`no element ${selector}`);
      for (const [type, buttons] of [['mouseMoved', 0], ['mousePressed', 1], ['mouseReleased', 0]]) {
        await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: type === 'mouseMoved' ? 'none' : 'left', buttons, clickCount: 1 });
      }
    },
    async close() {
      ws.close();
      const exited = new Promise((resolve) => child.once('exit', resolve));
      child.kill();
      await Promise.race([exited, sleep(5000)]);
      // The WebView2 processes of the run end with it; the next run must not meet them.
      await sleep(2000);
    },
  };
  await app.waitFor('document.readyState === "complete"', 'the interface');
  return app;
}

/** [`startOnce`], tried twice: on a busy PC the window may be late, or another run may hold the port. */
async function start(options) {
  try {
    return await startOnce(options);
  } catch (error) {
    log(`  start failed (${error.message}), trying again`);
    await sleep(3000);
    return startOnce(options);
  }
}

/** Milliseconds from the spawn to the `ardoise:library-listed` mark. */
async function listedAfter(app) {
  const listedAt = await app.waitFor(
    `(() => { const m = performance.getEntriesByName('ardoise:library-listed')[0]; return m ? performance.timeOrigin + m.startTime : 0; })()`,
    'the library to be listed',
    60_000,
  );
  return listedAt - app.spawnedAt;
}

/** In the page: resolves with when the cards on screen all have their thumbnail drawn (epoch ms), or null. */
const THUMBNAILS_DRAWN = `new Promise((resolve) => {
  const started = performance.now();
  const check = () => {
    const visible = [...document.querySelectorAll('.note-card')].filter((c) => {
      const r = c.getBoundingClientRect();
      return r.bottom > 0 && r.top < innerHeight;
    });
    const images = visible.map((c) => c.querySelector('.thumb img[data-drawn-at]'));
    if (visible.length > 0 && images.every(Boolean)) {
      resolve({ at: performance.timeOrigin + Math.max(...images.map((i) => Number(i.dataset.drawnAt))), visible: visible.length });
    } else if (performance.now() - started > 30000) {
      resolve(null);
    } else {
      requestAnimationFrame(check);
    }
  };
  check();
})`;

/** In the page: scrolls the library at a constant speed, noting the interval between two frames. */
const SCROLL = `new Promise((resolve) => {
  let el = document.querySelector('.note-grid');
  while (el && !(el.scrollHeight > el.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(el).overflowY))) el = el.parentElement;
  el = el ?? document.scrollingElement;
  const from = el.scrollTop;
  const intervals = [];
  let last = null;
  let first = null;
  const step = (now) => {
    first ??= now;
    if (last !== null) intervals.push(now - last);
    last = now;
    const elapsed = now - first;
    el.scrollTop = from + (${SCROLL_PX_PER_SECOND} * elapsed) / 1000;
    if (elapsed < ${SCROLL_MS}) requestAnimationFrame(step);
    else resolve({ intervals, scrolledPx: el.scrollTop - from, cards: document.querySelectorAll('.note-card').length });
  };
  requestAnimationFrame(step);
})`;

/** In the page: the interval between two frames over `ms` milliseconds. */
const FRAMES = (ms) => `new Promise((resolve) => {
  const intervals = [];
  let last = null;
  let first = null;
  const step = (now) => {
    first ??= now;
    if (last !== null) intervals.push(now - last);
    last = now;
    if (now - first < ${ms}) requestAnimationFrame(step);
    else resolve(intervals);
  };
  requestAnimationFrame(step);
})`;

/** One start on `appExe`: the time to the listed library, then (library parts) the thumbnails and the scroll. */
async function openLibrary({ appExe, local, full }) {
  const app = await start({ appExe, local });
  try {
    const result = { listedMs: await listedAfter(app) };
    if (full) {
      const drawn = await app.evaluate(THUMBNAILS_DRAWN);
      if (!drawn) throw new Error('the thumbnails on screen were not drawn in 30 s');
      result.thumbnailsMs = drawn.at - app.spawnedAt;
      result.visibleCards = drawn.visible;
      const scrolled = await app.evaluate(SCROLL);
      result.scroll = scrolled;
      // After the scroll: how long the thumbnails of the place it stopped at take.
      const stopped = Date.now();
      const settled = await app.evaluate(THUMBNAILS_DRAWN);
      result.settleMs = settled ? Math.max(0, settled.at - stopped) : null;
    }
    return result;
  } finally {
    await app.close();
  }
}

async function measureLive(local) {
  const library = makeLibrary('live-library', 0);
  const appExe = portableCopy('live', library);
  const app = await start({ appExe, local, args: ['--simulated-tablet'] });
  const j = JSON.stringify;
  const cards = (n) => `document.querySelectorAll('.note-card').length ${n}`;
  const strokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes ?? -1)`;
  const WIZARD = '.register-wizard';
  const step = (name) => `document.querySelector(${j(`${WIZARD} [data-step="${name}"]`)}) !== null`;
  try {
    await app.waitFor(cards('=== 8'), 'the demo notes');
    await app.waitFor(`document.querySelector('.tablet-status[data-connection="notRegistered"]') !== null`, 'the tablet block');
    await app.click('.tablet-status');
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
    await app.click('.tablet-panel button[data-action="register"]');
    await app.waitFor(step('warning'), 'the warning step');
    await app.click(`${WIZARD} button[data-action="continue"]`);
    await app.waitFor(step('done'), 'the end of the registration', 60_000);
    await app.click(`${WIZARD} button[data-action="finish"]`);
    await app.waitFor(`document.querySelector(${j(WIZARD)}) === null`, 'the wizard to close');
    await app.waitFor(`['connected', 'syncing'].includes(document.querySelector('.tablet-status')?.dataset.connection)`, 'the connection', 30_000);
    // The sync of the three stored pages runs first: live mode starts on a quiet app.
    await app.waitFor(cards('=== 11'), 'the synced pages', 40_000);
    await app.waitFor(`document.querySelector('.tablet-status[data-connection="connected"]') !== null`, 'the end of the sync', 30_000);
    await app.click('.tablet-panel button[data-action="live"]');
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
    await app.waitFor(`${strokes} > 0`, 'strokes on the live canvas', 15_000);
    await sleep(LIVE_WARMUP_MS);
    const before = await app.evaluate(strokes);
    const intervals = await app.evaluate(FRAMES(LIVE_MEASURE_MS));
    const after = await app.evaluate(strokes);
    return { intervals, strokesBefore: before, strokesAfter: after, measuredMs: LIVE_MEASURE_MS };
  } finally {
    await app.close();
  }
}

const result = {};
let failed = false;
try {
  const local = join(root, 'LocalAppData');
  mkdirSync(local);
  const demoExe = portableCopy('demo', makeLibrary('demo-library', 0));

  if (wants('start') || wants('library')) {
    // Cold: the WebView2 profile of `local` does not exist yet. Warm: the same profile again.
    log('start, cold');
    const cold = await openLibrary({ appExe: demoExe, local, full: false });
    if (wants('start')) {
      const warm = [];
      for (let i = 0; i < WARM_RUNS; i++) {
        log(`start, warm ${i + 1}/${WARM_RUNS}`);
        warm.push((await openLibrary({ appExe: demoExe, local, full: false })).listedMs);
      }
      result.start = { coldMs: cold.listedMs, warmMs: warm };
    }
  }
  if (wants('library')) {
    const bigExe = portableCopy('big', makeLibrary('big-library', NOTES));
    log(`library of ${NOTES} notes, first open (thumbnails not cached)`);
    const first = await openLibrary({ appExe: bigExe, local, full: true });
    log('library, second open (thumbnails cached)');
    const second = await openLibrary({ appExe: bigExe, local, full: true });
    result.library = { notes: NOTES, first, second, scrollPxPerSecond: SCROLL_PX_PER_SECOND };
  }
  if (wants('live')) {
    log('live mode');
    result.live = await measureLive(local);
  }
} catch (error) {
  failed = true;
  console.error(`PERF FAILED: ${error.message}`);
} finally {
  try {
    rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch {
    // A locked temporary file is not a measure failure.
  }
}
console.log(`PERF-JSON ${JSON.stringify(result)}`);
process.exit(failed ? 1 : 0);
