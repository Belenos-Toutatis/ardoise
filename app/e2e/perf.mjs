// Performance harness of `cargo xtask perf`: measures the start, a library of 1 000 notes and the live mode on the
// production executable, a portable copy of it in a temporary folder, driven through WebView2's DevTools protocol like
// the end-to-end journeys (`run.mjs`).
//
// Usage: node app/e2e/perf.mjs <path to ardoise.exe> [--only start,library,live,touchup] [--notes 1000]
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
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const exe = argv[0]?.startsWith('--') ? undefined : argv[0];
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
if (!exe) {
  console.error('usage: node app/e2e/perf.mjs <ardoise.exe> [--only start,library,live,touchup] [--notes 1000]');
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
    /** A key press: `key('z', { ctrl: true })`, `key('Delete')`. */
    async key(key, { ctrl = false } = {}) {
      const windowsVirtualKeyCode = key.length === 1 ? key.toUpperCase().charCodeAt(0) : { Delete: 46, Escape: 27 }[key];
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
