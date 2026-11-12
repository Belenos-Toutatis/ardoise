// Performance harness of `cargo xtask perf`: measures the start, a library of 1 000 notes and the live mode on the
// production executable, a portable copy of it in a temporary folder, driven through WebView2's DevTools protocol like
// the end-to-end journeys (`run.mjs`).
//
// Usage: node app/e2e/perf.mjs <path to ardoise.exe> [--only start,library,live,touchup,pen,paper,projection] [--notes 1000]
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
// - touchup (M12): the same on a loaded page (a resumed sheet of 4 000 strokes) while touch-ups arrive through the real
//   interface: a lasso, a colour, Delete, Ctrl+Z, Ctrl+Z, Ctrl+Y, Ctrl+Z, again and again. Also the time of one edit: the
//   longest frame just after each edit (what one edit costs on screen), and, on the same page, after a full redraw of
//   the cache (`repaintFromScratch` of the canvas: what an edit would cost without the repaint of an area).
// - pen (M14): as live, with the presentation window open, a layer « Correction » added and the Pen tool on, while the
//   script writes with the computer's pen through the DevTools protocol (a move every 8 ms, strokes of 1.5 s, 0.3 s
//   apart, at the foot of the page): the frame intervals of both windows, the strokes of both pens, and the latency of
//   the computer's points to the projector (`window.__ardoiseLatency.computer` of the presentation window).
// - paper (M17): as live, with the presentation window open and the page on screen given the Seyès template and a page of
//   a two-page PDF under it: the frame intervals of both windows while the simulated tablet streams.
// - projection (M17): a note of 1 000 strokes open in the editor, projected (« Projeter » of its Projection tab): the
//   projector's frame intervals while the script writes in the editor with the pen (as `pen`), the latency of the
//   editor's points to the projector (`window.__ardoiseLatency.computer` of the projection window), then the projector's
//   frame intervals during the replay of the note.
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const exe = argv[0]?.startsWith('--') ? undefined : argv[0];
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
if (!exe) {
  console.error('usage: node app/e2e/perf.mjs <ardoise.exe> [--only start,library,live,touchup,pen,paper,projection] [--notes 1000]');
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
/** Touch-up run: the loaded page (strokes in a grid of rows and columns) and the time measured. */
const LOADED_ROWS = 50;
const LOADED_COLUMNS = 80;
const TOUCHUP_MEASURE_MS = 10_000;
const TOUCHUP_IDLE_MS = 4000;
/** Strokes of a touch-up's lasso: 5 rows of 20 columns of the grid. */
const LASSO_ROWS = 5;
const LASSO_COLUMNS = 20;
/** An edit is followed for this long: its frame comes within it, and the next edit does not (the script waits 250 ms). */
const EDIT_WINDOW_MS = 230;
/** Full redraws of the cache timed on the page once the touch-ups are over. */
const FULL_REDRAWS = 10;
/** The computer's pen (M14): a move every this many ms, strokes this long, this far apart. */
const PEN_MOVE_MS = 8;
const PEN_STROKE_MS = 1500;
const PEN_GAP_MS = 300;
/** Where its strokes go: the foot of the page, below the lines of the simulated pen (its last baseline is at 19 600). */
const PEN_Y = 20_700;
/** The PDF under the live page (M17, part `paper`). */
const PAPER_PDF = 'fiche-perf.pdf';
/** The note of the editor's projection (M17): its strokes, this far apart in time (a replay of some 12 s), and the time of the replay measured. */
const REPLAY_STROKES = 1000;
const REPLAY_STROKE_GAP_MS = 12;
const REPLAY_MEASURE_MS = 8000;

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
      target = list.find((t) => t.type === 'page' && !t.url.includes('view=presentation') && !t.url.includes('view=projection'));
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
    /** The remote debugging port: the presentation window is another target of it. */
    port,
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
    /** A real mouse event (page pixels). */
    async mouse(type, x, y, { buttons = 0 } = {}) {
      await send('Input.dispatchMouseEvent', {
        type,
        x,
        y,
        button: type === 'mouseMoved' && buttons === 0 ? 'none' : 'left',
        buttons,
        clickCount: type === 'mouseMoved' ? 0 : 1,
      });
    },
    /** A pen event (`journeys-m8-input.mjs`): `pointerType: 'pen'`, the tip held with `buttons: 1`. */
    async pen(type, x, y, { buttons = 0, force = 0.5 } = {}) {
      await send('Input.dispatchMouseEvent', {
        type,
        x,
        y,
        pointerType: 'pen',
        button: type === 'mouseMoved' && buttons === 0 ? 'none' : 'left',
        buttons,
        force,
        clickCount: type === 'mouseMoved' ? 0 : 1,
      });
    },
    /** Types text into the focused element. */
    async type(text) {
      await send('Input.insertText', { text });
    },
    /** A key press: `key('z', { ctrl: true })`, `key('Delete')`. */
    async key(key, { ctrl = false } = {}) {
      const windowsVirtualKeyCode = key.length === 1 ? key.toUpperCase().charCodeAt(0) : { Delete: 46, Escape: 27, Enter: 13 }[key];
      const base = { key, code: key.length === 1 ? `Key${key.toUpperCase()}` : key, modifiers: ctrl ? 2 : 0, windowsVirtualKeyCode };
      await send('Input.dispatchKeyEvent', { type: 'keyDown', ...base, text: key.length === 1 && !ctrl ? key : undefined });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
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

/** In the page: what the canvas counted of the redraws of its cache. */
const REDRAWS = `(() => {
  const data = document.querySelector('canvas.live-canvas').dataset;
  return { full: Number(data.fullRedraws ?? 0), partial: Number(data.partialRedraws ?? 0) };
})()`;

/** In the page: the same, each interval with the time it ended. */
const STAMPED_FRAMES = (ms) => `new Promise((resolve) => {
  const intervals = [];
  const ends = [];
  let last = null;
  let first = null;
  const step = (now) => {
    first ??= now;
    if (last !== null) {
      intervals.push(now - last);
      ends.push(now);
    }
    last = now;
    if (now - first < ${ms}) requestAnimationFrame(step);
    else resolve({ intervals, ends });
  };
  requestAnimationFrame(step);
})`;

/**
 * In the page: the longest frame after each of `FULL_REDRAWS` full redraws of the cache (a frame apart from the next, and
 * time left in between), and how long the calls took. Null on an executable without the hook.
 */
const FULL_REDRAW_FRAMES = `new Promise((resolve) => {
  const canvas = document.querySelector('canvas.live-canvas');
  if (!canvas.repaintFromScratch) {
    resolve(null);
    return;
  }
  const frames = [];
  const calls = [];
  let left = ${FULL_REDRAWS};
  let wait = 20;
  let longest = 0;
  let last = null;
  let following = false;
  const step = (now) => {
    if (last !== null && following) longest = Math.max(longest, now - last);
    last = now;
    if (--wait > 0) {
      requestAnimationFrame(step);
      return;
    }
    if (following) frames.push(longest);
    if (left === 0) {
      resolve({ frames, calls });
      return;
    }
    left -= 1;
    const started = performance.now();
    canvas.repaintFromScratch();
    calls.push(performance.now() - started);
    following = true;
    longest = 0;
    wait = 14;
    requestAnimationFrame(step);
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

/** Registers the simulated tablet and waits for its three pages to be synced (`cards`: the notes shown once they are). */
async function registerAndSync(app, cards) {
  const j = JSON.stringify;
  const WIZARD = '.register-wizard';
  const step = (name) => `document.querySelector(${j(`${WIZARD} [data-step="${name}"]`)}) !== null`;
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
  await app.waitFor(`document.querySelectorAll('.note-card').length === ${cards}`, 'the synced pages', 40_000);
  await app.waitFor(`document.querySelector('.tablet-status[data-connection="connected"]') !== null`, 'the end of the sync', 30_000);
}

async function measureLive(local) {
  const library = makeLibrary('live-library', 0);
  const appExe = portableCopy('live', library);
  const app = await start({ appExe, local, args: ['--simulated-tablet'] });
  const strokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes ?? -1)`;
  try {
    await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
    await registerAndSync(app, 11);
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

/**
 * Live mode on a loaded page while the page is touched up (M12). The sheet is a note of `LOADED_ROWS` rows of
 * `LOADED_COLUMNS` short strokes, put in the library before the start and resumed from its card's menu; the simulated
 * tablet writes on it. While the frame intervals are recorded in the page, this script plays a teacher through the real
 * interface (mouse and keys of the DevTools protocol): Select tool, a lasso around 100 strokes, a colour swatch, Delete,
 * Ctrl+Z twice (the deletion, the colour), Ctrl+Y and Ctrl+Z again, then the next place on the page. The script notes the
 * page's clock before each edit, to find the frames that follow it.
 */
async function measureLiveTouchUp(local) {
  const { sheet } = await import('./journeys-m12-resume.mjs');
  const library = makeLibrary('touchup-library', 0);
  const ink = [];
  for (let row = 0; row < LOADED_ROWS; row++) {
    for (let column = 0; column < LOADED_COLUMNS; column++) {
      const x0 = 1500 + column * 340;
      const y0 = 800 + row * 400;
      const start = (row * LOADED_COLUMNS + column) * 400;
      ink.push({ points: Array.from({ length: 30 }, (_, i) => ({ x: x0 + i * 10, y: y0 + 120 * Math.sin(i / 3), p: 0.6, t: start + i * 10 })) });
    }
  }
  const title = 'Page chargée perf';
  const file = `${title}.ardoise`;
  writeFileSync(join(library, file), sheet({ title, index: 9, image: false, ink }).bytes);
  const appExe = portableCopy('touchup', library);
  const app = await start({ appExe, local, args: ['--simulated-tablet'] });
  const j = JSON.stringify;
  const loaded = LOADED_ROWS * LOADED_COLUMNS;
  const strokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes ?? -1)`;
  const barCount = `Number(document.querySelector('[data-slot="select-bar"] .select-count')?.dataset.count ?? 0)`;
  try {
    await app.waitFor(`document.querySelectorAll('.note-card').length === 9`, 'the demo notes and the loaded sheet');
    await registerAndSync(app, 12);
    // The tablet panel stays open beside the grid and would cover the cards of its last column.
    await app.evaluate(`[...document.querySelectorAll('.tablet-panel button')].find((b) => b.textContent.trim() === 'Fermer')?.click()`);
    await app.waitFor(`document.querySelector('.tablet-panel') === null`, 'the tablet panel to close');
    // « Reprendre en mode direct » in the menu of the sheet's card.
    const card = `.note-card[data-path*=${j(title)}]`;
    await app.click(`${card} .card-menu button[aria-haspopup="menu"]`);
    await app.waitFor(`document.querySelector('[role="menu"] [role="menuitem"]') !== null`, 'the card menu');
    await app.evaluate(`[...document.querySelectorAll('[role="menu"] [role="menuitem"]')].find((e) => e.textContent.includes('Reprendre en mode direct'))?.scrollIntoView()`);
    const item = await app.evaluate(`(() => {
      const e = [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].find((x) => x.textContent.includes('Reprendre en mode direct'));
      if (!e) return null;
      const r = e.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    })()`);
    if (!item) throw new Error('no « Reprendre en mode direct » in the card menu');
    await app.mouse('mouseMoved', item.x, item.y);
    await app.mouse('mousePressed', item.x, item.y, { buttons: 1 });
    await app.mouse('mouseReleased', item.x, item.y);
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode on the loaded sheet', 30_000);
    await app.waitFor(`${strokes} >= ${loaded}`, 'the strokes of the loaded sheet', 60_000);
    // The first paint of 4 000 outlines is not what is measured.
    await sleep(LIVE_WARMUP_MS + 2000);
    // The same page without a touch-up: what the page costs by itself (the tablet writes on it).
    const idleIntervals = await app.evaluate(FRAMES(TOUCHUP_IDLE_MS));
    await app.click('button[data-action="select"]');
    await app.waitFor(`document.querySelector('canvas.live-canvas.selecting') !== null`, 'the Select tool');

    const geometry = await app.evaluate(`(() => {
      const c = document.querySelector('canvas.live-canvas');
      const r = c.getBoundingClientRect();
      const sw = document.querySelector('.swatches .swatch:nth-child(5)').getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height, swatch: { x: sw.left + sw.width / 2, y: sw.top + sw.height / 2 } };
    })()`);
    const page = { width: 29700, height: 21600 };
    const margin = 12;
    const scale = Math.min((geometry.width - 2 * margin) / page.width, (geometry.height - 2 * margin) / page.height);
    const at = (x, y) => ({
      x: geometry.left + (geometry.width - page.width * scale) / 2 + x * scale,
      y: geometry.top + (geometry.height - page.height * scale) / 2 + y * scale,
    });
    // Four places of the page, each around LASSO_ROWS x LASSO_COLUMNS strokes of the grid.
    const places = [
      [0, 0],
      [LOADED_COLUMNS / 2, 10],
      [10, LOADED_ROWS / 2],
      [LOADED_COLUMNS / 2, LOADED_ROWS - LASSO_ROWS - 2],
    ].map(([column, row]) => ({
      x0: 1500 + column * 340 - 100,
      x1: 1500 + (column + LASSO_COLUMNS) * 340 - 100,
      y0: 800 + row * 400 - 300,
      y1: 800 + (row + LASSO_ROWS) * 400 - 150,
    }));
    const lasso = async (rect) => {
      const corners = [at(rect.x0, rect.y0), at(rect.x1, rect.y0), at(rect.x1, rect.y1), at(rect.x0, rect.y1), at(rect.x0, rect.y0)];
      const points = [];
      for (let k = 1; k < corners.length; k++) {
        const steps = Math.max(2, Math.ceil(Math.hypot(corners[k].x - corners[k - 1].x, corners[k].y - corners[k - 1].y) / 12));
        for (let n = 0; n < steps; n++) {
          const t = n / steps;
          points.push({ x: corners[k - 1].x + (corners[k].x - corners[k - 1].x) * t, y: corners[k - 1].y + (corners[k].y - corners[k - 1].y) * t });
        }
      }
      await app.mouse('mouseMoved', points[0].x, points[0].y);
      await app.mouse('mousePressed', points[0].x, points[0].y, { buttons: 1 });
      for (const q of points.slice(1)) {
        await app.mouse('mouseMoved', q.x, q.y, { buttons: 1 });
        await sleep(8);
      }
      await app.mouse('mouseReleased', corners[0].x, corners[0].y);
    };

    const before = await app.evaluate(strokes);
    await app.evaluate(`performance.clearMeasures(); 0`);
    const redrawsBefore = await app.evaluate(REDRAWS);
    let finished = false;
    const frames = app.evaluate(STAMPED_FRAMES(TOUCHUP_MEASURE_MS)).then((measured) => {
      finished = true;
      return measured;
    });
    const removed = [];
    let iterations = 0;
    /** The page's clock just before each gesture of the script, and what it was (`kind`). */
    const gestures = [];
    const note = async (kind) => gestures.push({ kind, at: await app.evaluate(`performance.now()`) });
    const noteEdit = (kind) => note(`edit ${kind}`);
    // A whole iteration is always finished, so that the page ends as it began (apart from what the tablet wrote).
    while (!finished || iterations === 0) {
      const rect = places[iterations % places.length];
      await note('lasso');
      await lasso(rect);
      await note('lasso released');
      await app.waitFor(`${barCount} > 0`, 'the lasso to choose strokes', 10_000);
      const picked = await app.evaluate(barCount);
      await sleep(120);
      await app.mouse('mouseMoved', geometry.swatch.x, geometry.swatch.y);
      await noteEdit('colour');
      await app.mouse('mousePressed', geometry.swatch.x, geometry.swatch.y, { buttons: 1 });
      await app.mouse('mouseReleased', geometry.swatch.x, geometry.swatch.y);
      await sleep(250);
      const full = await app.evaluate(strokes);
      await noteEdit('delete');
      await app.key('Delete');
      // The deletion must show: the count falls by the strokes chosen (the tablet adds one now and then).
      await app.waitFor(`${strokes} <= ${full} - ${Math.max(1, picked - 5)}`, 'the deletion to reach the canvas', 10_000);
      removed.push(full - (await app.evaluate(strokes)));
      await sleep(250);
      for (const [key, what] of [['z', 'undo delete'], ['z', 'undo colour'], ['y', 'redo colour'], ['z', 'undo colour']]) {
        await noteEdit(what);
        await app.key(key, { ctrl: true });
        await sleep(250);
      }
      iterations++;
    }
    const { intervals, ends } = await frames;
    const after = await app.evaluate(strokes);
    // The longest frame after each edit of the measure; the edits made once it was over have no frames.
    const editFrames = gestures
      .filter(({ kind, at }) => kind.startsWith('edit') && at + EDIT_WINDOW_MS <= ends[ends.length - 1])
      .map(({ at }) => Math.max(0, ...intervals.filter((_, k) => ends[k] > at && ends[k] <= at + EDIT_WINDOW_MS)));
    // Which gesture each late frame came after: where to look when frames are missed.
    intervals.forEach((ms, k) => {
      if (ms <= 25) return;
      const gesture = gestures.findLast(({ at }) => at < ends[k]);
      log(`  late frame: ${ms.toFixed(1)} ms, after « ${gesture?.kind ?? 'nothing'} » by ${gesture ? (ends[k] - gesture.at).toFixed(0) : '-'} ms`);
    });
    const redrawsAfter = await app.evaluate(REDRAWS);
    const editCalls = await app.evaluate(`performance.getEntriesByName('ardoise:live-partial-redraw').map((m) => m.duration)`);
    const fullRedraw = await app.evaluate(FULL_REDRAW_FRAMES);
    return {
      intervals,
      editFrames,
      editCalls,
      fullRedrawFrames: fullRedraw?.frames ?? [],
      fullRedrawCalls: fullRedraw?.calls ?? [],
      partialRedraws: redrawsAfter.partial - redrawsBefore.partial,
      fullRedraws: redrawsAfter.full - redrawsBefore.full,
      idleIntervals,
      strokesBefore: before,
      strokesAfter: after,
      measuredMs: TOUCHUP_MEASURE_MS,
      loaded,
      iterations,
      // Six edits per iteration: the colour, the deletion, two undos, a redo and an undo.
      edits: iterations * 6,
      removedPerDelete: removed.length ? removed.reduce((a, b) => a + b, 0) / removed.length : 0,
    };
  } finally {
    await app.close();
  }
}

/**
 * Live mode with the tablet and the computer's pen together (M14, docs/design/tablet-live-classroom.md, "Performance (C)").
 * As `measureLive` (the simulated tablet streams), with the presentation window open (the browser flags of `startOnce`
 * keep the covered control window painting), a layer « Correction » added from the layers zone and the Pen tool on.
 * While the frame intervals are recorded in the control window and in the presentation window, this script writes with
 * the pen through the DevTools protocol (`pointerType: 'pen'`): a move every `PEN_MOVE_MS`, strokes of `PEN_STROKE_MS`,
 * `PEN_GAP_MS` apart, at the foot of the page, below the simulated pen's lines. The latency of the computer's points on
 * the projector is the presentation's `__ardoiseLatency.computer`, the samples of the measure only.
 */
async function measureLivePen(local) {
  const { attach } = await import('./journeys-m12-resume.mjs');
  const library = makeLibrary('pen-library', 0);
  const appExe = portableCopy('pen', library);
  const app = await start({ appExe, local, args: ['--simulated-tablet'] });
  const j = JSON.stringify;
  const strokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes ?? -1)`;
  const zone = 'section.layers-zone[data-where="live"]';
  const computerSamples = `window.__ardoiseLatency.computer.samples().length`;
  let presentation = null;
  try {
    // The computer's default ink (red felt tip): nothing remembered in the browser's storage.
    await app.evaluate(`(() => { try { localStorage.removeItem('ardoise.live.computerStyle'); } catch {} return true; })()`);
    await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
    await registerAndSync(app, 11);
    await app.click('.tablet-panel button[data-action="live"]');
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
    await app.waitFor(`${strokes} > 0`, 'strokes on the live canvas', 15_000);

    await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
    presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window', 30_000);
    await presentation.waitFor(`typeof window.__ardoiseLatency?.computer?.samples === 'function'`, 'the latency of the computer’s points in the presentation', 20_000);

    // A layer « Correction » for the computer (« Ajouter un calque » chooses it), then the Pen tool.
    await app.click(`${zone} button[data-action="add-layer"]`);
    await app.waitFor(`document.querySelector(${j(`${zone} li.layer[data-layer="1"][data-chosen="true"]`)}) !== null`, 'the new layer, chosen');
    await app.click(`${zone} li.layer[data-layer="1"] button[data-action="rename-layer"]`);
    await app.waitFor(`document.activeElement?.matches?.('input.layer-rename') === true`, 'the name field');
    await app.evaluate(`(() => { document.activeElement.select(); return true; })()`);
    await app.type('Correction');
    await app.key('Enter');
    await app.waitFor(`(document.querySelector(${j(`${zone} li.layer[data-layer="1"] .layer-name`)})?.textContent ?? '').trim() === 'Correction'`, 'the name « Correction »');
    await app.click('[data-slot="touch-up"] button[data-action="pen"]');
    await app.waitFor(`document.querySelector('canvas.live-canvas[data-tool="pen"]') !== null`, 'the Pen tool');

    const geometry = await app.evaluate(`(() => {
      const r = document.querySelector('canvas.live-canvas').getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    })()`);
    // The live page of the simulated Bamboo Slate, held landscape (the tablet's area: SLATE_LARGE_WIDTH x HEIGHT).
    const page = { width: 29700, height: 21600 };
    const margin = 12;
    const scale = Math.min((geometry.width - 2 * margin) / page.width, (geometry.height - 2 * margin) / page.height);
    const at = (x, y) => ({
      x: geometry.left + (geometry.width - page.width * scale) / 2 + x * scale,
      y: geometry.top + (geometry.height - page.height * scale) / 2 + y * scale,
    });
    const moves = Math.round(PEN_STROKE_MS / PEN_MOVE_MS);
    /** Stroke `n`: a wave of 5 000 units, one of four places along the foot of the page. */
    const strokeAt = (n) => {
      const x0 = 3000 + (n % 4) * 6000;
      return Array.from({ length: moves + 1 }, (_, i) => at(x0 + (5000 * i) / moves, PEN_Y + 250 * Math.sin((i / moves) * 4 * Math.PI)));
    };
    /** One stroke of the pen, its moves on a fixed beat (a slow DevTools call is caught up, not added). */
    const write = async (points) => {
      await app.pen('mouseMoved', points[0].x, points[0].y);
      await app.pen('mousePressed', points[0].x, points[0].y, { buttons: 1 });
      const t0 = Date.now();
      for (let i = 1; i < points.length; i++) {
        const wait = t0 + i * PEN_MOVE_MS - Date.now();
        if (wait > 0) await sleep(wait);
        await app.pen('mouseMoved', points[i].x, points[i].y, { buttons: 1, force: 0.6 });
      }
      const last = points[points.length - 1];
      await app.pen('mouseReleased', last.x, last.y);
    };

    // A first stroke out of the measure: the first outlines and the first IPC calls are not what is measured.
    await write(strokeAt(3));
    await sleep(LIVE_WARMUP_MS);
    const before = await app.evaluate(strokes);
    const samplesBefore = await presentation.evaluate(computerSamples);
    let finished = 0;
    const measured = (promise) =>
      promise.then((intervals) => {
        finished += 1;
        return intervals;
      });
    const control = measured(app.evaluate(FRAMES(LIVE_MEASURE_MS)));
    const projected = measured(presentation.evaluate(FRAMES(LIVE_MEASURE_MS)));
    let penStrokes = 0;
    // Whole strokes only: the last one ends after the frames, as the touch-ups' last iteration does.
    while (finished < 2) {
      await write(strokeAt(penStrokes));
      penStrokes += 1;
      if (finished < 2) await sleep(PEN_GAP_MS);
    }
    const [controlIntervals, presentationIntervals] = await Promise.all([control, projected]);
    // The last stroke's end reaches both windows.
    await sleep(300);
    const after = await app.evaluate(strokes);
    // The recorder keeps the last 2 000 samples: a warm-up stroke and a measure of some 1 000 points fit in it.
    const latency = await presentation.evaluate(`window.__ardoiseLatency.computer.samples().slice(${samplesBefore})`);
    return {
      controlIntervals,
      presentationIntervals,
      strokesBefore: before,
      strokesAfter: after,
      penStrokes,
      computerLatency: latency ?? [],
      measuredMs: LIVE_MEASURE_MS,
    };
  } finally {
    presentation?.close();
    await app.close();
  }
}

/**
 * Live mode on a page of a PDF (M17, docs/design/paper-projection-narration.md §8, part `paper`). As `measureLive`, with
 * the presentation window open, the page on screen given the Seyès template and the first page of a two-page PDF under
 * it, through live mode's own commands (`live_page_paper`, `stage_background_image`, `live_page_image`: what the Page and
 * Fond tabs call); once both windows draw the PDF, the frame intervals of both while the simulated tablet streams.
 */
async function measurePaper(local) {
  const { attach } = await import('./journeys-m12-resume.mjs');
  const { twoPagePdf } = await import('./journeys-m7-background.mjs');
  const j = JSON.stringify;
  const library = makeLibrary('paper-library', 0);
  const appExe = portableCopy('paper', library);
  const pdf = join(root, PAPER_PDF);
  writeFileSync(pdf, twoPagePdf());
  const app = await start({ appExe, local, args: ['--simulated-tablet'] });
  const strokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes ?? -1)`;
  const showsPdf = `(() => {
    const d = document.querySelector('canvas.live-canvas')?.dataset;
    return d?.image === ${j(PAPER_PDF)} && d.template === 'seyes' && d.paperReady === 'true';
  })()`;
  const invoke = (command, args) => app.evaluate(`window.__TAURI_INTERNALS__.invoke(${j(command)}, ${j(args)})`);
  let presentation = null;
  try {
    await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
    await registerAndSync(app, 11);
    await app.click('.tablet-panel button[data-action="live"]');
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
    await app.waitFor(`${strokes} > 0`, 'strokes on the live canvas', 15_000);
    await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
    presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window', 30_000);
    await presentation.waitFor(`document.querySelector('canvas.live-canvas') !== null`, 'the presentation canvas', 20_000);

    const pageIndex = await app.evaluate(`Number((document.querySelector('canvas.live-canvas')?.dataset.page ?? '1/1').split('/')[0]) - 1`);
    await invoke('live_page_paper', { pageIndex, background: { paper: 'white', template: 'seyes', texture: false } });
    const staged = await invoke('stage_background_image', { path: pdf, pdfPage: 1 });
    // Contained on the tablet's landscape page, as « Ajuster » places it.
    const page = { width: 29700, height: 21600 };
    const scale = Math.min(page.width / staged.width, page.height / staged.height);
    const rect = {
      x: (page.width - staged.width * scale) / 2,
      y: (page.height - staged.height * scale) / 2,
      width: staged.width * scale,
      height: staged.height * scale,
    };
    await invoke('live_page_image', { pageIndex, op: { kind: 'set', key: staged.key, image: { name: staged.name, pdfPage: 1, rect, quarterTurns: 0, opacity: 1 } } });
    await app.waitFor(showsPdf, 'the PDF under the live page', 60_000);
    await presentation.waitFor(showsPdf, 'the PDF under the projected page', 60_000);

    await sleep(LIVE_WARMUP_MS);
    const before = await app.evaluate(strokes);
    const [controlIntervals, presentationIntervals] = await Promise.all([
      app.evaluate(FRAMES(LIVE_MEASURE_MS)),
      presentation.evaluate(FRAMES(LIVE_MEASURE_MS)),
    ]);
    const after = await app.evaluate(strokes);
    // The tablet stayed on that page (the next one has no image): the frames were those of a page of a PDF.
    const imageShown = (await app.evaluate(showsPdf)) === true && (await presentation.evaluate(showsPdf)) === true;
    return { controlIntervals, presentationIntervals, strokesBefore: before, strokesAfter: after, imageShown, measuredMs: LIVE_MEASURE_MS };
  } finally {
    presentation?.close();
    await app.close();
  }
}

/**
 * The editor's projection (M17, docs/design/paper-projection-narration.md §2.8 and §8, part `projection`). A note of
 * `REPLAY_STROKES` strokes is put in the library and opened in the editor, the Projection tab's « Projeter » opens the
 * projection window (the browser flags of `startOnce` keep the covered editor painting). While the projector's frame
 * intervals are recorded, this script writes in the editor with the pen through the DevTools protocol as `measureLivePen`
 * does (a move every `PEN_MOVE_MS`, strokes of `PEN_STROKE_MS`, `PEN_GAP_MS` apart, at the foot of the page): the latency
 * of the editor's points to the projector is its `__ardoiseLatency.computer`, the samples of the measure only. Then the
 * replay (R) of the note, projected: the projector's frame intervals while it plays (the late frames tell whether the
 * replay's partial stroke must go as a stroke in progress, R2).
 */
async function measureProjection(local) {
  const { attach, strokesBin, writeZip } = await import('./journeys-m12-resume.mjs');
  const j = JSON.stringify;
  const library = makeLibrary('projection-library', 0);
  const page = { width: 29700, height: 21600 };
  const ink = Array.from({ length: REPLAY_STROKES }, (_, i) => {
    const x0 = 1200 + (i % 40) * 680;
    const y0 = 1500 + Math.floor(i / 40) * 720;
    const start = i * REPLAY_STROKE_GAP_MS;
    return { points: Array.from({ length: 12 }, (_, k) => ({ x: x0 + k * 40, y: y0 + 150 * Math.sin(k / 2), p: 0.6, t: start + k * 4 })) };
  });
  const title = 'Note projetée perf';
  const created = Date.UTC(2026, 10, 1, 9, 0, 0);
  const noteJson = {
    format: { version: 1, minor: 2, writer: 'Ardoise 0.1.0' },
    background: { paper: 'white', template: 'blank', texture: false },
    created,
    id: '00000000-0000-4000-8000-000000000917',
    layers: [{ name: 'Calque 1', visible: true }],
    modified: created,
    origin: { kind: 'ardoise' },
    page,
    tags: [],
    timing: 'recorded',
    title,
  };
  writeFileSync(
    join(library, `${title}.ardoise`),
    writeZip([
      ['note.json', Buffer.from(j(noteJson, null, 2))],
      ['strokes.bin', strokesBin(ink)],
    ]),
  );
  const appExe = portableCopy('projection', library);
  const app = await start({ appExe, local });
  const editorStrokes = `document.querySelectorAll('.editor g.ink path[data-stroke]').length`;
  const drawn = `Number(document.querySelector('canvas.live-canvas')?.dataset.drawn ?? -1)`;
  const computerSamples = `window.__ardoiseLatency.computer.samples().length`;
  const tab = (name) => `[data-side-panel] .side-rail [role="tab"][data-tab="${name}"]`;
  let projection = null;
  try {
    await app.waitFor(`document.querySelectorAll('.note-card').length === 9`, 'the demo notes and the note of the measure');
    await app.click(`.note-card[data-path*=${j(title)}] .card-open`);
    await app.waitFor(`${editorStrokes} === ${REPLAY_STROKES}`, `the ${REPLAY_STROKES} strokes in the editor`, 30_000);
    // The page is fitted once the editor has measured its stage.
    await sleep(500);
    await app.click(tab('projection'));
    await app.waitFor(`document.querySelector('[data-tab-panel="projection"] button[data-action="project"]:not([disabled])') !== null`, 'the Project button');
    await app.click('[data-tab-panel="projection"] button[data-action="project"]');
    projection = await attach(app.port, (t) => t.url.includes('view=projection'), 'the projection window', 30_000);
    await projection.waitFor(`${drawn} === ${REPLAY_STROKES}`, 'the note on the projector', 30_000);
    await projection.waitFor(`typeof window.__ardoiseLatency?.computer?.samples === 'function'`, 'the latency of the editor’s points in the projection', 20_000);
    await app.click(tab('layers'));
    await app.click('.toolbar button[title="Stylo"]');

    const geometry = await app.evaluate(`(() => {
      const r = document.querySelector('.editor svg.canvas').getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    })()`);
    const moves = Math.round(PEN_STROKE_MS / PEN_MOVE_MS);
    /** Stroke `n`: a wave across a seventh of the canvas, one of four places along its foot. */
    const strokeAt = (n) => {
      const x0 = geometry.left + geometry.width * (0.15 + (n % 4) * 0.18);
      const y0 = geometry.top + geometry.height * 0.82;
      return Array.from({ length: moves + 1 }, (_, i) => ({
        x: x0 + (geometry.width * 0.15 * i) / moves,
        y: y0 + geometry.height * 0.02 * Math.sin((i / moves) * 4 * Math.PI),
      }));
    };
    /** One stroke of the pen, its moves on a fixed beat (a slow DevTools call is caught up, not added). */
    const write = async (points) => {
      await app.pen('mouseMoved', points[0].x, points[0].y);
      await app.pen('mousePressed', points[0].x, points[0].y, { buttons: 1 });
      const t0 = Date.now();
      for (let i = 1; i < points.length; i++) {
        const wait = t0 + i * PEN_MOVE_MS - Date.now();
        if (wait > 0) await sleep(wait);
        await app.pen('mouseMoved', points[i].x, points[i].y, { buttons: 1, force: 0.6 });
      }
      const last = points[points.length - 1];
      await app.pen('mouseReleased', last.x, last.y);
    };

    // A first stroke out of the measure: the first outlines and the first IPC calls are not what is measured.
    await write(strokeAt(3));
    await sleep(LIVE_WARMUP_MS);
    const drawnBefore = await projection.evaluate(drawn);
    const samplesBefore = await projection.evaluate(computerSamples);
    let finished = false;
    const frames = projection.evaluate(FRAMES(LIVE_MEASURE_MS)).then((intervals) => {
      finished = true;
      return intervals;
    });
    let penStrokes = 0;
    // Whole strokes only: the last one ends after the frames, as in `measureLivePen`.
    while (!finished) {
      await write(strokeAt(penStrokes));
      penStrokes += 1;
      if (!finished) await sleep(PEN_GAP_MS);
    }
    const projectorIntervals = await frames;
    await sleep(500);
    const drawnAfter = await projection.evaluate(drawn);
    const latency = await projection.evaluate(`window.__ardoiseLatency.computer.samples().slice(${samplesBefore})`);

    // The replay of the whole note, projected.
    const replayStrokes = await app.evaluate(editorStrokes);
    await app.key('r');
    await app.waitFor(`document.querySelector('button[data-action="replay-toggle"]')?.getAttribute('aria-pressed') === 'true'`, 'the replay');
    const [replayIntervals, replayEditorIntervals] = await Promise.all([
      projection.evaluate(FRAMES(REPLAY_MEASURE_MS)),
      app.evaluate(FRAMES(REPLAY_MEASURE_MS)),
    ]);
    // The projector followed the replay: fewer strokes than the note's while it plays.
    const replayDrawn = await projection.evaluate(drawn);
    return {
      projectorIntervals,
      penStrokes,
      drawnBefore,
      drawnAfter,
      computerLatency: latency ?? [],
      replayIntervals,
      replayEditorIntervals,
      replayStrokes,
      replayFollowed: replayDrawn >= 0 && replayDrawn < replayStrokes,
      measuredMs: LIVE_MEASURE_MS,
      replayMeasuredMs: REPLAY_MEASURE_MS,
    };
  } finally {
    projection?.close();
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
  if (wants('touchup')) {
    log('live mode, loaded page, touch-ups');
    result.liveTouchUp = await measureLiveTouchUp(local);
  }
  if (wants('pen')) {
    log('live mode, the tablet and the computer’s pen together');
    result.livePen = await measureLivePen(local);
  }
  if (wants('paper')) {
    log('live mode on a page of a PDF');
    result.livePaper = await measurePaper(local);
  }
  if (wants('projection')) {
    log('the editor’s projection: the pen, then the replay');
    result.editorProjection = await measureProjection(local);
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
