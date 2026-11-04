// Milestone M12 journeys "touch up during live mode" (SPEC §4.2, docs/questions.md Q9,
// docs/design/tablet-live-classroom.md, "M12": « Touch up, as built » and « the selection »).
//
// On the production executable with the simulated tablet (`--simulated-tablet`), in French and in English. The
// simulated pen writes the whole time (cursive fragments, a stroke every second or so, 10 minutes of script), so
// every edit below is made while the tablet is adding strokes to the page. Three places are compared:
//   - the control window (the preview canvas of the app's own window),
//   - the presentation window (a second DevTools target of the same process),
//   - the share: a real browser (headless Microsoft Edge, if this PC has it) that joined with the code, and a Node
//     client of the event stream, which builds the list of strokes from the messages (`StreamModel`) and says what
//     every `strokesEdited` event named. The model also checks, after each step, that nothing the tablet wrote was
//     lost or doubled and that a stroke which came back is at its old place in the list.
//
// 1. The first journey: select with the toolbar button and the V key, lasso a group of strokes with real mouse events
//    (the lasso is a rectangle, so the strokes it must pick are known from the geometry the stream carries), click
//    another stroke, Shift + click, Ctrl + click: the bar shows the count. The highlight is drawn in the control
//    window only (pixels of the three canvases), and nothing about it goes through the stream. A swatch recolours
//    the selection in the three places and leaves the pen's colour alone (the next stroke of the tablet is black).
//    Delete, by the button and by the key. Ctrl+Z brings the strokes back, Ctrl+Y removes them again, with the
//    tablet writing between; undoing down to the first edit then redoing up again; « Effacer la page » then Ctrl+Z.
//    The three places hold the same strokes at the end. After « Terminer » the note in the library holds what was on
//    screen (strokes and colours, in the file and in the editor).
// 2. The second journey: on a resumed sheet with an image background, delete a stroke of the old note and recolour
//    another one. After « Terminer » the note keeps its image, its original and every other member, with the edits.
//
// Limits: the narration is not recorded (no microphone); the pen's back, the finger and the touch gestures are
// covered by the interface tests (the lasso here is the mouse's); how the highlight looks is for the eye (H5).
import { readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { assertNoBrowserProblem, closePanel, EventStream, findEdge, request, startBrowser, until } from './journeys-m11-share.mjs';
import {
  attach,
  canon,
  canvasData,
  closeEditor,
  dropSheet,
  EDITOR_STROKES,
  IMAGE_NAME,
  LIVE_CANVAS,
  notesOf,
  openInEditor,
  readZip,
  registerTablet,
  resumeFromCard,
  STATUS,
  stopLive,
  strokesOf,
  text,
  titled,
} from './journeys-m12-resume.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const VIEWER_CANVAS = 'canvas.viewer-canvas';
const PREVIEW_MARGIN = 12;
const BAR = '[data-slot="select-bar"]';
const SELECT_BUTTON = 'button[data-action="select"]';
const RED = { r: 216, g: 0, b: 0, a: 255 };
const GREEN = { r: 132, g: 189, b: 0, a: 255 };
const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const sameColor = (a, b) => !!a && !!b && a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
/**
 * A key press that is made again when nothing came of it: the control window handles one edit at a time and ignores a
 * key pressed while the previous edit is still under way (`lib/live-touch-up.ts`), and a teacher presses again.
 * `done()` says whether the press had its effect.
 */
async function pressUntil(app, name, options, done, what, { presses = 4, wait = 2500 } = {}) {
  for (let k = 0; k < presses; k++) {
    await press(app, name, options);
    const end = Date.now() + wait;
    while (Date.now() < end) {
      if (done()) return;
      await sleep(25);
    }
  }
  throw new Error(`timed out waiting for ${what} (${presses} key presses)`);
}

/** Where the journey is, for the message of a step that does not end (a DevTools call that never answers). */
let lastPoint = '';
const at = (point) => {
  lastPoint = point;
};
const countOf = (selector) => `Number(document.querySelector(${j(selector)})?.dataset.strokes ?? -1)`;

// --- What the stream says -------------------------------------------------------------------------------------

/** A stroke's identity in these journeys: the time of its first point (milliseconds since the note was created). */
const printOf = (stroke) => String(stroke.points[0].t);

/**
 * The page as a viewer of the share builds it from the messages of `/events` (the same rules as `LiveState`):
 * finished strokes, the stroke in progress, and the list of what every `strokesEdited` event named. `registry` gives
 * each stroke its order of first appearance; `deleted` holds the strokes taken out by an edit (or a clearing) and not
 * yet back. `check()` throws unless the list is that registry less the deleted ones, in the registry's order.
 */
class StreamModel {
  constructor(stream) {
    this.stream = stream;
    this.read = 0;
    this.page = null;
    this.pageIndex = -1;
    this.strokes = [];
    this.current = null;
    this.registry = new Map();
    this.deleted = new Set();
    /** The prints of the strokes the tablet ended, in order. */
    this.tablet = [];
    /** The `strokesEdited` events, with the prints they name. */
    this.edits = [];
    /** Strokes that ended by the next one starting, not by a `strokeEnd` (message index, print). */
    this.unended = [];
  }

  register(stroke) {
    const print = printOf(stroke);
    if (!this.registry.has(print)) this.registry.set(print, this.registry.size);
    return print;
  }

  count() {
    return this.strokes.length + (this.current ? 1 : 0);
  }

  /** How long ago the last message of the stream came (ms), and whether the app closed the stream. */
  health() {
    const last = this.stream.messages.at(-1);
    return `last message ${last ? Math.round(performance.timeOrigin + performance.now() - last.at) : '-'} ms ago, ${this.stream.closed ? 'closed' : 'open'}`;
  }

  /** Applies the messages that arrived since the last call. */
  sync() {
    if (this.stream.failure) throw new Error(this.stream.failure);
    if (this.stream.closed && this.read >= this.stream.messages.length) throw new Error('the share closed the event stream of this client');
    for (; this.read < this.stream.messages.length; this.read++) {
      const entry = this.stream.messages[this.read];
      if (entry.msg.type !== 'live') continue;
      for (const event of entry.msg.batch.events) this.apply(event, entry.i);
    }
    return this;
  }

  apply(event, i) {
    switch (event.type) {
      case 'snapshot': {
        const { snapshot } = event;
        const before = new Set(this.strokes.map(printOf));
        this.page = snapshot.page.page;
        this.pageIndex = snapshot.pageIndex;
        this.strokes = snapshot.page.strokes;
        const after = new Set(this.strokes.map((stroke) => this.register(stroke)));
        for (const print of before) if (!after.has(print)) this.deleted.add(print);
        for (const print of after) this.deleted.delete(print);
        this.current = null;
        break;
      }
      case 'strokeStart':
        if (event.pageIndex !== this.pageIndex) break;
        // As `LiveState` does: a stroke that starts ends the one in progress (it should have had its `strokeEnd` first).
        if (this.current) {
          this.unended.push({ i, print: this.current.points[0] ? printOf(this.current) : '?' });
          if (this.current.points.length > 0) {
            this.tablet.push(this.register(this.current));
            this.strokes.push(this.current);
          }
        }
        this.current = { ...event.stroke, points: [...event.stroke.points] };
        break;
      case 'points':
        if (event.pageIndex !== this.pageIndex) break;
        if (!this.current) this.current = { color: null, points: [] };
        this.current.points.push(...event.points);
        break;
      case 'strokeEnd':
        if (event.pageIndex === this.pageIndex && this.current && this.current.points.length > 0) {
          this.tablet.push(this.register(this.current));
          this.strokes.push(this.current);
        }
        this.current = null;
        break;
      case 'strokesEdited': {
        if (event.pageIndex !== this.pageIndex) break;
        const removed = event.removed.map((index) => printOf(this.strokes[index]));
        const gone = new Set(event.removed);
        this.strokes = this.strokes.filter((_, index) => !gone.has(index));
        for (const { index, stroke } of event.inserted) this.strokes.splice(index, 0, stroke);
        const inserted = event.inserted.map(({ stroke }) => this.register(stroke));
        const recoloured = event.recoloured.map(({ color, indices }) => ({ color, prints: indices.map((index) => printOf(this.strokes[index])) }));
        for (const { color, indices } of event.recoloured) for (const index of indices) this.strokes[index].color = color;
        for (const print of removed) this.deleted.add(print);
        for (const print of inserted) this.deleted.delete(print);
        this.edits.push({ i, keys: Object.keys(event).sort().join(), removed, inserted, recoloured, removedIndices: event.removed, insertedIndices: event.inserted.map((p) => p.index) });
        break;
      }
      default:
        break;
    }
  }

  /** Throws unless the list is what the registry and the deleted strokes say. */
  check(what) {
    this.sync();
    const prints = this.strokes.map(printOf);
    for (let k = 1; k < prints.length; k++) {
      if (!(this.registry.get(prints[k]) > this.registry.get(prints[k - 1]))) {
        throw new Error(`${what}: the strokes are not in their order of writing at place ${k} (${prints[k - 1]} then ${prints[k]})`);
      }
    }
    const expected = [...this.registry.keys()].filter((print) => !this.deleted.has(print));
    if (expected.length !== prints.length || expected.some((print, k) => print !== prints[k])) {
      throw new Error(`${what}: the list holds ${prints.length} strokes, the registry less the deleted ones ${expected.length}`);
    }
  }

  /** The edits since message `from`. */
  editsSince(from) {
    this.sync();
    return this.edits.filter((edit) => edit.i >= from);
  }

  /** The finished strokes with their print, colour and index. */
  listing() {
    return this.strokes.map((stroke, index) => ({ index, print: printOf(stroke), color: stroke.color }));
  }
}

/** What the stream's edits were, one word each, to say what happened when a step ends oddly. */
const describeEdits = (model) =>
  model.edits.map((e) => `${e.removed.length ? `-${e.removed.length}` : ''}${e.inserted.length ? `+${e.inserted.length}` : ''}${e.recoloured.map((r) => `~${r.prints.length}${colorText(r.color)}`).join('')}`).join(' ');

const sameSet = (a, b) => a.length === b.length && [...a].sort().join() === [...b].sort().join();

// --- Geometry: where to press ---------------------------------------------------------------------------------

const pointInRect = (p, r) => p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1;

/**
 * The box of a stroke's samples, kept with it. These plans run in the journey's own process, which also reads the share's
 * stream: a plan that takes seconds on a page of hundreds of strokes stops the reading, the share drops the client
 * ("a slow viewer is disconnected") and the journey fails for what it did itself. Boxes keep a plan to milliseconds.
 */
const boxes = new WeakMap();
function boxOf(stroke) {
  let box = boxes.get(stroke);
  if (!box || box.n !== stroke.points.length) {
    box = { n: stroke.points.length, x0: Infinity, x1: -Infinity, y0: Infinity, y1: -Infinity };
    for (const p of stroke.points) {
      box.x0 = Math.min(box.x0, p.x);
      box.x1 = Math.max(box.x1, p.x);
      box.y0 = Math.min(box.y0, p.y);
      box.y1 = Math.max(box.y1, p.y);
    }
    boxes.set(stroke, box);
  }
  return box;
}
const boxesMeet = (a, b) => a.x0 <= b.x1 && a.x1 >= b.x0 && a.y0 <= b.y1 && a.y1 >= b.y0;

/** The strokes a rectangular lasso picks (half of their samples inside), and whether one is too close to call. */
function lassoOutcome(strokes, rect) {
  const hits = [];
  let doubtful = false;
  strokes.forEach((stroke, index) => {
    if (!boxesMeet(boxOf(stroke), rect)) return;
    let inside = 0;
    for (const p of stroke.points) if (pointInRect(p, rect)) inside++;
    if (inside === 0) return;
    const share = inside / stroke.points.length;
    if (share >= 0.5) hits.push(index);
    if (share > 0.3 && share < 0.7) doubtful = true;
  });
  return { hits, doubtful };
}

/**
 * A rectangle that surrounds the strokes `start` to `start + length - 1` and picks a known set of strokes (none too
 * close to call), for the first of `starts` that gives one that `accept(rect, hits)` allows: `{ rect, hits }`, rect in
 * page units.
 */
function planLasso(strokes, page, { starts, length = 3, accept = () => true }) {
  for (const from of starts.slice(0, 60)) {
    for (let size = length; size >= 1; size--) {
      for (const margin of [150, 450, 900, 1600]) {
        const targets = strokes.slice(from, from + size);
        if (targets.length < size) continue;
        const xs = targets.flatMap((st) => st.points.map((p) => p.x));
        const ys = targets.flatMap((st) => st.points.map((p) => p.y));
        const rect = {
          x0: Math.max(100, Math.min(...xs) - margin),
          x1: Math.min(page.width - 100, Math.max(...xs) + margin),
          y0: Math.max(100, Math.min(...ys) - margin),
          y1: Math.min(page.height - 100, Math.max(...ys) + margin),
        };
        const { hits, doubtful } = lassoOutcome(strokes, rect);
        if (!doubtful && hits.length >= size && accept(rect, hits)) return { rect, hits };
      }
    }
  }
  throw new Error(`no lasso picks strokes the way the step needs (${strokes.length} strokes, ${starts.length} places tried)`);
}

function distanceToSegment(p, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const t = dx === 0 && dy === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy)));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** The topmost stroke whose ink passes within `tolerance` of `p`, as the click of the selection tool finds it. */
function topmostAt(strokes, p, tolerance) {
  for (let index = strokes.length - 1; index >= 0; index--) {
    const stroke = strokes[index];
    const reach = tolerance + stroke.width / 2;
    if (!boxesMeet(boxOf(stroke), { x0: p.x - reach, x1: p.x + reach, y0: p.y - reach, y1: p.y + reach })) continue;
    const pts = stroke.points;
    let best = Infinity;
    for (let k = 0; k < pts.length; k++) best = Math.min(best, k === 0 ? Math.hypot(p.x - pts[0].x, p.y - pts[0].y) : distanceToSegment(p, pts[k - 1], pts[k]));
    if (best <= reach) return index;
  }
  return null;
}

/** A point on stroke `index` where a click picks that stroke and no other (`scale`: screen pixels per page unit). */
function planClick(strokes, index, scale, { lowFirst = false } = {}) {
  const pts = strokes[index].points;
  const order = pts.map((_, k) => k).sort((a, b) => (lowFirst ? pts[b].y - pts[a].y : Math.abs(a - pts.length / 2) - Math.abs(b - pts.length / 2)));
  for (const k of order) {
    const p = { x: pts[k].x, y: pts[k].y };
    // Every stroke within the click's reach (not only the topmost) must be this one.
    const reach = 6 / scale + 40;
    const around = { x0: p.x - reach * 3, x1: p.x + reach * 3, y0: p.y - reach * 3, y1: p.y + reach * 3 };
    const near = strokes.filter((s) => boxesMeet(boxOf(s), around) && s.points.some((q) => Math.hypot(q.x - p.x, q.y - p.y) < reach * 3));
    if (near.length === 1 && topmostAt(strokes, p, 6 / scale) === index) return p;
  }
  for (const k of order) {
    const p = { x: pts[k].x, y: pts[k].y };
    if (topmostAt(strokes, p, 6 / scale) === index) return p;
  }
  throw new Error(`no point of stroke ${index} picks it`);
}

/** Maps page points to window pixels on the preview (the page fitted in the canvas with its margin, centred). */
async function previewMap(app, page) {
  at('measuring the preview');
  const box = await app.evaluate(`(() => {
    const c = document.querySelector(${j(LIVE_CANVAS)});
    c.scrollIntoView({ block: 'nearest' });
    const r = c.getBoundingClientRect();
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  })()`);
  const scale = Math.min((box.width - 2 * PREVIEW_MARGIN) / page.width, (box.height - 2 * PREVIEW_MARGIN) / page.height);
  const x0 = box.left + (box.width - page.width * scale) / 2;
  const y0 = box.top + (box.height - page.height * scale) / 2;
  return { scale, at: (p) => ({ x: x0 + p.x * scale, y: y0 + p.y * scale }) };
}

// --- Pixels ---------------------------------------------------------------------------------------------------

/**
 * Page expression: colours on a canvas (a sample, every other pixel) and the area of its paper (the box of its white
 * pixels). Red, green and blue are chromatic tints (ink or highlight, antialiased edges included); black ink stays out.
 */
const inkStats = (selector) => `(() => {
  const c = document.querySelector(${j(selector)});
  if (!c || c.width === 0) return null;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  // A small canvas (the share's page in a small window) is read pixel by pixel: its thin strokes are mostly antialiased.
  const step = c.width * c.height > 1500000 ? 2 : 1;
  let x0 = c.width, x1 = -1, y0 = c.height, y1 = -1, red = 0, green = 0, blue = 0, dark = 0;
  for (let y = 0; y < c.height; y += step) {
    for (let x = 0; x < c.width; x += step) {
      const i = (y * c.width + x) * 4;
      const r = d[i], g = d[i + 1], b = d[i + 2];
      if (r >= 253 && g >= 253 && b >= 253) {
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      } else if (Math.max(r, g, b) - Math.min(r, g, b) >= 8) {
        // A tint, antialiased edges included: the dominant channel says which (grey ink and its edges have none).
        if (r >= g && r >= b) red++;
        else if (g >= b) green++;
        else blue++;
      } else if (r + g + b < 300) dark++;
    }
  }
  const area = x1 < 0 ? 0 : ((x1 - x0) / step + 1) * ((y1 - y0) / step + 1);
  return { red, green, blue, dark, area };
})()`;

const share = (stats, tint) => (stats && stats.area > 0 ? stats[tint] / stats.area : 0);

/** `promise`, or an error naming the window that did not answer within `ms` (a DevTools call has no timeout of its own). */
function within(promise, ms, what) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} did not answer within ${ms / 1000} s`)), ms);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

/** The three places side by side: the control window, the presentation window, the page of the real browser, the stream. */
class Places {
  constructor({ app, model, presentation, browser }) {
    this.app = app;
    this.model = model;
    this.presentation = presentation;
    this.browser = browser;
  }

  async counts() {
    at('reading the strokes of the three places');
    this.model.sync();
    const [control, presented, viewed, badge] = await Promise.all([
      within(this.app.evaluate(countOf(LIVE_CANVAS)), 10_000, 'the control window'),
      this.presentation ? within(this.presentation.evaluate(countOf(LIVE_CANVAS)), 10_000, 'the presentation window') : null,
      this.browser ? within(this.browser.evaluate(countOf(VIEWER_CANVAS)), 10_000, 'the browser of the share') : null,
      within(this.app.evaluate(`document.querySelector('.share-indicator')?.getAttribute('data-viewers') ?? null`), 10_000, 'the control window'),
    ]);
    this.model.sync();
    if (badge !== this.badge) {
      console.log(`    écrans connectés (compteur du partage) : ${this.badge ?? '-'} -> ${badge} ; flux Node ${this.model.health()}`);
      this.badge = badge;
    }
    return { control, presented, viewed, stream: this.model.count() };
  }

  /** Waits until the places all hold the same number of strokes (the tablet adds one now and then: read it again). */
  async agree(what, timeout = 20_000) {
    const end = Date.now() + timeout;
    for (;;) {
      const counts = await this.counts();
      const values = Object.values(counts).filter((v) => v !== null);
      if (values[0] >= 0 && values.every((v) => v === values[0])) return counts;
      if (Date.now() > end) throw new Error(`${what}: the places do not hold the same number of strokes (${j(counts)}; stream: ${this.model.health()})`);
      await sleep(150);
    }
  }

  async stats() {
    at('reading the pixels of the three places');
    const [control, presented, viewed] = await Promise.all([
      within(this.app.evaluate(inkStats(LIVE_CANVAS)), 15_000, 'the control window'),
      this.presentation ? within(this.presentation.evaluate(inkStats(LIVE_CANVAS)), 15_000, 'the presentation window') : null,
      this.browser ? within(this.browser.send('Page.bringToFront').then(() => this.browser.evaluate(inkStats(VIEWER_CANVAS))), 15_000, 'the browser of the share') : null,
    ]);
    return { control, presented, viewed };
  }

  /** Waits until `predicate(stats)` holds (a canvas is redrawn a frame or two after the event); returns the stats. */
  async until(predicate, what, timeout = 30_000) {
    const end = Date.now() + timeout;
    for (;;) {
      const stats = await this.stats();
      if (predicate(stats)) return stats;
      if (Date.now() > end) {
        const saved = await this.save().catch(() => '');
        throw new Error(`${what} (${j(stats)})${saved}`);
      }
      await sleep(400);
    }
  }

  /** Saves what each canvas shows as a PNG in the run's folder (to look at when a step fails); the names. */
  async save() {
    const grab = (selector) => `document.querySelector(${j(selector)})?.toDataURL('image/png') ?? null`;
    const shots = {
      control: await within(this.app.evaluate(grab(LIVE_CANVAS)), 5000, 'the control window').catch(() => null),
      presentation: this.presentation ? await within(this.presentation.evaluate(grab(LIVE_CANVAS)), 5000, 'the presentation window').catch(() => null) : null,
      viewer: this.browser ? await within(this.browser.evaluate(grab(VIEWER_CANVAS)), 5000, 'the browser').catch(() => null) : null,
    };
    const names = [];
    for (const [name, url] of Object.entries(shots)) {
      if (!url) continue;
      const path = join(this.app.dir, `canvas-${name}.png`);
      writeFileSync(path, Buffer.from(url.split(',')[1], 'base64'));
      names.push(path);
    }
    const alive = this.browser
      ? await within(this.browser.evaluate(`({ frames: window.__frames, visibility: document.visibilityState, hidden: document.hidden, now: Math.round(performance.now()) })`), 5000, 'the browser').catch((e) => String(e))
      : null;
    const viewer = this.browser
      ? await within(this.browser.evaluate(`new Promise((resolve) => {
          const c = document.querySelector(${j(VIEWER_CANVAS)});
          let frames = 0;
          const t0 = performance.now();
          const tick = () => (performance.now() - t0 < 500 ? (frames++, requestAnimationFrame(tick)) : resolve({ strokes: c?.dataset.strokes, shown: c?.dataset.shown, hidden: document.hidden, frames, size: [c?.width, c?.height] }));
          requestAnimationFrame(tick);
        })`), 5000, 'the browser').catch((e) => String(e))
      : null;
    writeFileSync(join(this.app.dir, 'stream-messages.jsonl'), this.model.stream.messages.map((m) => j({ i: m.i, at: m.at, raw: m.raw })).join(String.fromCharCode(10)));
    const where = this.browser && viewer && String(viewer).startsWith('Error') ? await within(this.browser.where(), 8000, 'the browser').catch((e) => String(e)) : null;
    this.model.sync();
    const kinds = this.model.stream.messages
      .slice(-40)
      .flatMap((m) => (m.msg.type === 'live' ? m.msg.batch.events.map((e) => e.type) : [m.msg.type]))
      .join(',');
    const tally = {};
    for (const m of this.model.stream.messages) if (m.msg.type === 'live') for (const e of m.msg.batch.events) tally[e.type] = (tally[e.type] ?? 0) + 1;
    return ` [stream events ${j(tally)}; in progress: ${this.model.current ? this.model.current.points.length : 'none'} points; last events: ${kinds}; viewer stack: ${j(where)}; viewer frames: ${j(alive)}; canvases: ${names.join(', ')}; viewer ${j(viewer)}; stream ${this.model.count()} (closed: ${this.model.stream.closed}, ${this.model.stream.messages.length} messages, last ${this.model.stream.messages.at(-1)?.msg.type}); control ${await within(this.app.evaluate(countOf(LIVE_CANVAS)), 5000, 'the control window').catch(() => '?')}]`;
  }
}

/** Every tint of every present canvas satisfies `test(share)`. */
const everywhere = (stats, tint, test) => Object.values(stats).every((s) => s === null || test(share(s, tint)));

// --- The window's own controls ---------------------------------------------------------------------------------

/** A key press on the control window (`app.key` knows ctrl and shift). */
const press = (app, name, options) => {
  at(`key ${name}${options?.ctrl ? ' with Ctrl' : ''}`);
  return app.key(name, options);
};

/** Page expression: the label of the pen's colour (the pressed swatch). */
const PEN_COLOR = `[...document.querySelectorAll('.swatches .swatch')].filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.getAttribute('aria-label')).join('|')`;

const barCount = `Number(document.querySelector(${j(`${BAR} .select-count`)})?.dataset.count ?? 0)`;
const selectionIs = (n) => (n === 0 ? `document.querySelector(${j(BAR)}) === null` : `${barCount} === ${n}`);

/** A lasso with real mouse events: the rectangle's outline, from its top left corner, closed. */
async function drawLasso(app, map, rect, { shift = false } = {}) {
  at('drawing a lasso');
  const corners = [
    { x: rect.x0, y: rect.y0 },
    { x: rect.x1, y: rect.y0 },
    { x: rect.x1, y: rect.y1 },
    { x: rect.x0, y: rect.y1 },
    { x: rect.x0, y: rect.y0 },
  ].map(map.at);
  const points = [];
  for (let k = 1; k < corners.length; k++) {
    const steps = Math.max(2, Math.ceil(Math.hypot(corners[k].x - corners[k - 1].x, corners[k].y - corners[k - 1].y) / 12));
    for (let s = 0; s < steps; s++) {
      const t = s / steps;
      points.push({ x: corners[k - 1].x + (corners[k].x - corners[k - 1].x) * t, y: corners[k - 1].y + (corners[k].y - corners[k - 1].y) * t });
    }
  }
  points.push(corners[corners.length - 1]);
  if (!shift) return app.drag(points);
  // The same with Shift held (CDP modifiers: Shift is 8).
  const [first, ...rest] = points;
  const mouse = (type, p, buttons) => app.send('Input.dispatchMouseEvent', { type, x: p.x, y: p.y, button: type === 'mouseMoved' && !buttons ? 'none' : 'left', buttons, clickCount: type === 'mouseMoved' ? 0 : 1, modifiers: 8 });
  await mouse('mouseMoved', first, 0);
  await mouse('mousePressed', first, 1);
  for (const p of rest) {
    await mouse('mouseMoved', p, 1);
    await sleep(8);
  }
  await mouse('mouseReleased', rest[rest.length - 1], 0);
}

/** A click with the mouse on the preview; `modifiers`: 8 = Shift, 2 = Ctrl. */
async function clickAt(app, where, modifiers = 0) {
  at('a click on the preview');
  const send = (type, buttons) => app.send('Input.dispatchMouseEvent', { type, x: where.x, y: where.y, button: type === 'mouseMoved' ? 'none' : 'left', buttons, clickCount: type === 'mouseMoved' ? 0 : 1, modifiers });
  await send('mouseMoved', 0);
  await send('mousePressed', 1);
  await send('mouseReleased', 0);
}

/** Joins the share with the code, as a browser would: the cookie for `/events`. */
async function joinStream(app, code) {
  const reply = await request(app.sharePort, { method: 'POST', path: '/join', headers: { 'Content-Type': 'application/json' }, body: j({ code }) });
  if (reply.status !== 200) throw new Error(`POST /join answered ${reply.status}`);
  const cookie = [reply.headers['set-cookie'] ?? []].flat().join('\n').split(';')[0].trim();
  const stream = await EventStream.open(app.sharePort, cookie);
  await stream.next((msg) => msg.type === 'live' && msg.reset === true, 'the first message of the stream', { timeout: 10_000 });
  return stream;
}

/** Starts the share from the toolbar; returns its access code (spaces out). */
async function startShare(app) {
  await app.click('button[data-action="share"]');
  await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
  return app.evaluate(`document.querySelector('[data-share-code]').textContent.replace(/[\\s\\u00a0\\u202f]+/g, '')`);
}

/** Opens live mode from the tablet panel (the registration closed it) and waits for the first strokes. */
async function openLive(app) {
  await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
  await app.click('.tablet-panel button[data-action="live"]');
  await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
  await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 15_000);
}

// --- A note's strokes.bin -----------------------------------------------------------------------------------------

/** The strokes of a `strokes.bin` (docs/file-format.md §3): `{ tool, color, width, layer, points: [{ x, y, p, t }] }`. */
export function decodeStrokes(bytes) {
  if (bytes.subarray(0, 4).toString('latin1') !== 'ASTK') throw new Error('not a strokes.bin');
  let at = 5;
  const varint = () => {
    let value = 0;
    for (let scale = 1; ; scale *= 0x80) {
      const byte = bytes[at++];
      value += (byte & 0x7f) * scale;
      if (!(byte & 0x80)) return value;
    }
  };
  const svarint = () => {
    const z = varint();
    return z % 2 === 0 ? z / 2 : -(z + 1) / 2;
  };
  const count = varint();
  const strokes = [];
  const last = { x: 0, y: 0, p: 0, t: 0 };
  for (let n = 0; n < count; n++) {
    const tool = bytes[at++];
    const color = { r: bytes[at++], g: bytes[at++], b: bytes[at++], a: bytes[at++] };
    const width = bytes.readFloatLE(at);
    at += 4;
    const layer = varint();
    const points = [];
    for (let k = varint(); k > 0; k--) {
      last.x += svarint();
      last.y += svarint();
      last.p += svarint();
      last.t += svarint();
      points.push({ x: last.x / 100, y: last.y / 100, p: last.p / 65535, t: last.t });
    }
    strokes.push({ tool, color, width, layer, points });
  }
  return strokes;
}

/** `rgba(216, 0, 0, 1.000)` (or `rgb(…)`) as `{ r, g, b, a }`. */
const parseFill = (fill) => {
  const m = /rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+))?/.exec(fill ?? '');
  return m ? { r: +m[1], g: +m[2], b: +m[3], a: m[4] === undefined ? 255 : Math.round(+m[4] * 255) } : null;
};

const colorText = (c) => (c ? `${c.r},${c.g},${c.b},${c.a}` : 'none');

// --- The journeys --------------------------------------------------------------------------------------------------

/**
 * `check` with a limit on each step: a window that stops answering (a DevTools call has no timeout of its own) must end
 * as a failed step, not as a journey that never ends while the tablet keeps writing.
 */
function limited(check, ms = 150_000) {
  return (description, action) =>
    check(description, async () => {
      let timer;
      const late = new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`the step did not end within ${ms / 1000} s: ${description} (last point reached: ${lastPoint})`)), ms);
      });
      try {
        return await Promise.race([action(), late]);
      } finally {
        clearTimeout(timer);
      }
    });
}

const BROWSER_ARGS = '--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding';

export const touchUpJourneys = [
  {
    name: 'retouches en direct : sélection au lasso et au clic, couleur, suppression, annulation, effacement, pendant que la tablette écrit',
    args: ['--simulated-tablet'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    // The presentation window covers the control window: without these flags the control window would stop painting.
    browserArgs: BROWSER_ARGS,
    async run(app, rawCheck) {
      const check = limited(rawCheck);
      const l = (key) => text(app.language, key);
      let browser = null;
      let presentation = null;
      let stream = null;
      let model = null;
      let places = null;
      let map = null;
      let notesBefore = [];
      let baseline = null;
      let sheet = null;
      /** Strokes the user picks: prints and the plan of the lasso. */
      const edited = { a: null, b: null, bc: null };

      const replan = async () => {
        map = await previewMap(app, model.page);
        return map;
      };
      const pressSwatch = (name) => app.click(`.swatches .swatch[aria-label=${j(l(`editor.colors.${name}`))}]`);
      const strokesNow = () => model.sync().strokes;
      // The simulated pen writes line after line down the page (a line every 1 600 units, 12 lines) and starts again at
      // the top: new ink falls over old strokes there. A lasso or a click must be where the pen will not be for a while:
      // `ahead(y)` is how far down the page (cyclically) the pen has to go to reach `y`.
      const CYCLE = 19_200;
      const penY = () => {
        model.sync();
        const point = model.current?.points.at(-1) ?? model.strokes.at(-1)?.points.at(-1);
        return point ? point.y : 0;
      };
      const aheadOf = () => {
        const pen = penY();
        return (y) => (((y - pen) % CYCLE) + CYCLE) % CYCLE;
      };
      /** Places to start a group of strokes at, the ones the pen reaches last first (from stroke `from` on). */
      const starts = (strokes, from = 0) => {
        const ahead = aheadOf();
        const mid = (st) => st.points.reduce((sum, p) => sum + p.y, 0) / st.points.length;
        return strokes.map((_, i) => i).filter((i) => i >= from && i < strokes.length - 1).sort((a, b) => ahead(mid(strokes[b])) - ahead(mid(strokes[a])));
      };
      /** A rectangle is safe when the pen is at least two lines from it, either way, and not about to wrap into it. */
      const safe = () => {
        const ahead = aheadOf();
        return (rect) => {
          const top = ahead(rect.y0);
          const bottom = ahead(rect.y1);
          return top >= 2400 && bottom <= 18_800 && bottom >= top;
        };
      };
      /** The lasso of strokes already chosen (their prints): the same group, around where it is now. */
      const lassoOf = (prints) => {
        const strokes = strokesNow();
        const found = prints.map((print) => strokes.findIndex((st) => printOf(st) === print));
        if (found.some((i) => i < 0)) throw new Error(`strokes ${prints} are gone`);
        const exact = (_, hits) => sameSet(hits.map((i) => printOf(strokes[i])), prints);
        return planLasso(strokes, model.page, { starts: [Math.min(...found)], length: prints.length, accept: exact });
      };

      try {
        await registerTablet(app, check);
        await check('le dossier avant le direct : les pages synchronisées sont déjà des notes', async () => {
          notesBefore = notesOf(app.library).map((n) => n.path);
        });

        await check('ouvrir le mode direct (tablette simulée)', async () => {
          await openLive(app);
        });

        await check('fenêtre de présentation ouverte', async () => {
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
          presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window');
          await presentation.waitFor(`Number(${canvasData('strokes')}) >= 1`, 'the strokes in the presentation');
        });

        await check('partage : un vrai navigateur (Edge sans fenêtre) et un client Node du flux ont rejoint', async () => {
          const code = await startShare(app);
          const edge = findEdge();
          if (edge) {
            browser = await startBrowser(edge, BROWSER_ARGS.split(' '));
            await browser.navigate(`http://127.0.0.1:${app.sharePort}/`);
            await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'code'`, 'the code form', 30_000);
            await browser.click('#viewer-code');
            await browser.type(code);
            await browser.waitFor(`document.querySelector('form button[type="submit"]')?.disabled === false`, 'the join button');
            await browser.click('form button[type="submit"]');
            await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'live'`, 'the live sheet', 20_000);
            await browser.waitFor(`Number(document.querySelector(${j(VIEWER_CANVAS)})?.dataset.strokes) > 0`, 'strokes on the page', 20_000);
            // The page must stay visible (it is shown to nobody: Windows may count it hidden under the presentation window, and a
            // hidden page does not paint, which is its right).
            await browser.send('Emulation.setFocusEmulationEnabled', { enabled: true });
            await browser.send('Page.bringToFront');
            // A frame counter of the page, to tell a page that stops painting from one that does not answer.
            await browser.evaluate(`window.__frames = 0; (function tick() { window.__frames++; requestAnimationFrame(tick); })(); true`);
          } else {
            console.log('    skipped: Edge not found, the share is followed by the stream only');
          }
          stream = await joinStream(app, code);
          model = new StreamModel(stream).sync();
          places = new Places({ app, model, presentation, browser });
          await closePanel(app, l('common.close'));
          if (await app.evaluate(`document.querySelector('.share-panel') !== null`)) throw new Error('the share panel stays open and would take the keys');
        });

        await check('la tablette écrit : au moins 12 traits finis dans la liste du flux', async () => {
          await until(() => model.sync().strokes.length >= 12, 'twelve finished strokes in the stream (is the simulated pen writing?)', 60_000);
          model.check('before touching up');
          await places.agree('before touching up');
          baseline = await places.stats();
        });

        // --- 2. Select ----------------------------------------------------------------------------------------------

        await check('« Sélection » (bouton de la barre d’outils) : l’outil est actif ; la touche V le coupe puis le rallume', async () => {
          await app.click(SELECT_BUTTON);
          await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the Select tool on');
          await app.waitFor(`document.querySelector(${j(`${LIVE_CANVAS}.selecting`)}) !== null`, 'the canvas to take the selection cursor');
          await press(app, 'v');
          await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the V key to turn it off');
          await press(app, 'v');
          await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the V key to turn it on again');
          await replan();
          baseline = await places.stats();
        });

        let selectMark = 0;
        await check('lasso à la souris autour d’un groupe de traits : la barre annonce le nombre de traits pris', async () => {
          selectMark = stream.mark();
          // Away from the pen: when the page holds too few lines yet, a moment later it does.
          let strokes;
          const plan = await until(() => {
            strokes = [...strokesNow()];
            try {
              return planLasso(strokes, model.page, { starts: starts(strokes), length: 3, accept: safe() });
            } catch {
              return null;
            }
          }, 'a group of strokes the pen is far from', 40_000);
          edited.a = { ...plan, prints: plan.hits.map((i) => printOf(strokes[i])) };
          await replan();
          await drawLasso(app, map, plan.rect);
          await app.waitFor(selectionIs(plan.hits.length), `the bar to show ${plan.hits.length} strokes`);
          console.log(`    lasso : ${plan.hits.length} traits pris (indices ${plan.hits.join(', ')})`);
        });

        await check('rien de la sélection ailleurs : le halo n’est que dans la fenêtre de commande, le flux n’en dit rien', async () => {
          const stats = await places.until(
            (s) => s.control && s.control.blue > baseline.control.blue + 150,
            'the highlight is not drawn in the control window',
          );
          console.log(`    pixels bleus (fenêtre de commande) : ${baseline.control.blue} -> ${stats.control.blue} ; présentation : ${stats.presented?.blue} ; navigateur : ${stats.viewed?.blue ?? '-'}`);
          for (const [name, s] of Object.entries({ presentation: stats.presented, browser: stats.viewed })) {
            if (s && s.blue > Math.max(5, (baseline[name === 'browser' ? 'viewed' : 'presented']?.blue ?? 0) + 5)) {
              throw new Error(`the ${name} shows blue pixels (${s.blue}): the selection leaks`);
            }
          }
          await sleep(400);
          model.sync();
          const seen = stream.messages.filter((m) => m.i >= selectMark);
          if (model.editsSince(selectMark).length > 0) throw new Error('the stream carries an edit while only a selection exists');
          if (seen.some((m) => /select|lasso|highlight/i.test(m.raw))) throw new Error('a message of the stream mentions the selection');
          if (seen.some((m) => m.msg.type === 'live' && m.msg.batch.events.some((e) => e.type === 'snapshot'))) throw new Error('a snapshot went to the share when only a selection changed');
          console.log(`    flux : ${seen.length} messages depuis la sélection, que des traits de la tablette`);
        });

        await check('clic sur un autre trait : un seul trait ; Maj + clic en ajoute un, Ctrl + clic le retire ; Échap désélectionne', async () => {
          await replan();
          let strokes;
          // Two other strokes the pen is far from, each with a point where a click picks it and nothing else.
          const picked = await until(() => {
            strokes = [...strokesNow()];
            const inside = safe();
            const found = [];
            for (const i of starts(strokes)) {
              if (edited.a.prints.includes(printOf(strokes[i]))) continue;
              if (!inside({ y0: Math.min(...strokes[i].points.map((p) => p.y)), y1: Math.max(...strokes[i].points.map((p) => p.y)) })) continue;
              try {
                found.push({ index: i, point: planClick(strokes, i, map.scale) });
              } catch {
                continue;
              }
              if (found.length === 2) return found;
            }
            return null;
          }, 'two other strokes the pen is far from, that a click can pick');
          const [b, c] = picked.map((p) => p.index);
          const [pointB, pointC] = picked.map((p) => p.point);
          await clickAt(app, map.at(pointB));
          await app.waitFor(selectionIs(1), 'one stroke selected by the click');
          await clickAt(app, map.at(pointC), 8);
          await app.waitFor(selectionIs(2), 'two strokes with Shift + click');
          await clickAt(app, map.at(pointC), 2);
          await app.waitFor(selectionIs(1), 'one stroke after Ctrl + click');
          edited.bc = { indices: [b, c], prints: [printOf(strokes[b]), printOf(strokes[c])], points: [pointB, pointC] };
          await press(app, 'Escape');
          await app.waitFor(selectionIs(0), 'Esc to deselect');
          if (!(await app.evaluate(`document.querySelector('.live-view') !== null`))) throw new Error('Esc closed live mode');
          model.check('after the clicks');
        });

        // --- 3. Recolour and delete ----------------------------------------------------------------------------------

        let penBefore = '';
        await check('un échantillon de couleur recolore la sélection (Rouge) dans les trois endroits ; la couleur du stylo ne change pas', async () => {
          penBefore = await app.evaluate(PEN_COLOR);
          if (penBefore !== l('editor.colors.black')) throw new Error(`the pen starts with ${j(penBefore)}, expected black`);
          const plan = lassoOf(edited.a.prints);
          await replan();
          const mark = stream.mark();
          await drawLasso(app, map, plan.rect);
          await app.waitFor(selectionIs(plan.hits.length), `the bar to show ${plan.hits.length} strokes`);
          await pressSwatch('red');
          await until(() => model.editsSince(mark).length > 0, 'the strokesEdited event of the recolouring');
          const [edit] = model.editsSince(mark);
          // Nothing but what the viewers need (docs/design/lan-share.md): no revision, no undo state.
          if (edit.keys !== 'inserted,pageIndex,recoloured,removed,type') throw new Error(`the event carries the keys ${edit.keys}`);
          if (edit.removed.length > 0 || edit.inserted.length > 0) throw new Error('a recolouring removed or inserted strokes');
          if (edit.recoloured.length !== 1 || !sameColor(edit.recoloured[0].color, RED)) throw new Error(`recoloured: ${j(edit.recoloured)}`);
          if (!sameSet(edit.recoloured[0].prints, edited.a.prints)) throw new Error(`the event recolours ${edit.recoloured[0].prints} instead of ${edited.a.prints}`);
          model.check('after the recolouring');
          const penAfter = await app.evaluate(PEN_COLOR);
          if (penAfter !== penBefore) throw new Error(`the pen's colour changed: ${j(penBefore)} then ${j(penAfter)}`);
          // The selection stays (a colour does not change the strokes' places)...
          await app.waitFor(selectionIs(plan.hits.length), 'the selection to stay after the recolouring');
          // ...and its highlight hides the new colour in the control window: deselect to see it.
          await press(app, 'Escape');
          await app.waitFor(selectionIs(0), 'Esc to deselect');
          const stats = await places.until((s) => everywhere(s, 'red', (r) => r > 0.00004), 'red ink in the three places');
          console.log(`    part de rouge : commande ${share(stats.control, 'red').toExponential(2)}, présentation ${share(stats.presented, 'red').toExponential(2)}, navigateur ${stats.viewed ? share(stats.viewed, 'red').toExponential(2) : '-'}`);
          // What the tablet writes next keeps the pen's colour (black), not the swatch.
          const known = model.sync().tablet.length;
          await until(() => model.sync().tablet.length >= known + 2, 'two strokes written by the tablet after the recolouring', 30_000);
          for (const print of model.tablet.slice(known)) {
            const stroke = model.strokes.find((s) => printOf(s) === print);
            if (stroke && !sameColor(stroke.color, BLACK)) throw new Error(`the tablet writes in ${colorText(stroke.color)} after the recolouring`);
          }
        });

        await check('un second groupe (clic, Maj + clic) devient Vert ; la sélection se défait avec Échap', async () => {
          const strokes = strokesNow();
          const [b, c] = edited.bc.indices;
          // The strokes may have been shifted by nothing: only the tablet appended since.
          if (printOf(strokes[b]) !== edited.bc.prints[0] || printOf(strokes[c]) !== edited.bc.prints[1]) throw new Error('the strokes moved');
          await replan();
          const mark = stream.mark();
          await clickAt(app, map.at(edited.bc.points[0]));
          await clickAt(app, map.at(edited.bc.points[1]), 8);
          await app.waitFor(selectionIs(2), 'two strokes selected');
          await pressSwatch('green');
          await until(() => model.editsSince(mark).length > 0, 'the recolouring to green');
          const [edit] = model.editsSince(mark);
          if (!sameColor(edit.recoloured[0]?.color, GREEN) || !sameSet(edit.recoloured[0].prints, edited.bc.prints)) throw new Error(`recoloured: ${j(edit.recoloured)}`);
          model.check('after the green');
          await press(app, 'Escape');
          await app.waitFor(selectionIs(0), 'Esc to deselect');
          await places.until((s) => everywhere(s, 'green', (r) => r > 0.00004), 'green ink in the three places');
        });

        await check('« Supprimer » (bouton) : le premier groupe (rouge) disparaît des trois endroits, les trois comptent le même nombre de traits', async () => {
          const plan = lassoOf(edited.a.prints);
          await replan();
          const mark = stream.mark();
          await drawLasso(app, map, plan.rect);
          await app.waitFor(selectionIs(plan.hits.length), 'the red group selected');
          await app.click(`${BAR} button[data-action="delete-selection"]`);
          await until(() => model.editsSince(mark).length > 0, 'the strokesEdited event of the delete');
          const [edit] = model.editsSince(mark);
          if (!sameSet(edit.removed, edited.a.prints) || edit.inserted.length > 0 || edit.recoloured.length > 0) throw new Error(`the delete removed ${edit.removed} (expected ${edited.a.prints})`);
          model.check('after the delete');
          await app.waitFor(selectionIs(0), 'the bar to go with the strokes');
          const counts = await places.agree('after the delete by the button');
          const stats = await places.until((s) => everywhere(s, 'red', (r) => r < 0.0001), 'the red strokes to leave the three places');
          console.log(`    après la suppression : ${j(counts)} ; vert ${j(Object.values(stats).map((s) => s && share(s, 'green').toExponential(1)))}`);
          if (!everywhere(stats, 'green', (r) => r > 0.00004)) throw new Error('the green strokes were lost with the delete');
        });

        await check('Suppr (touche) : un autre groupe disparaît à son tour ; les trois endroits sont d’accord', async () => {
          const greens = new Set(edited.bc.prints);
          // Three strokes the pen is far from, none of the green ones in the lasso (a moment later, there may be more room).
          let strokes;
          const plan = await until(() => {
            strokes = [...strokesNow()];
            const inside = safe();
            const noGreen = (rect, hits) => inside(rect) && !hits.some((h) => greens.has(printOf(strokes[h])));
            try {
              return planLasso(strokes, model.page, { starts: starts(strokes), length: 3, accept: noGreen });
            } catch {
              return null;
            }
          }, 'a second group of strokes the pen is far from');
          edited.b = { ...plan, prints: plan.hits.map((i) => printOf(strokes[i])) };
          await replan();
          const mark = stream.mark();
          await drawLasso(app, map, plan.rect);
          await app.waitFor(selectionIs(plan.hits.length), 'the second group selected');
          await press(app, 'Delete');
          await until(() => model.editsSince(mark).length > 0, 'the strokesEdited event of the key');
          const [edit] = model.editsSince(mark);
          if (!sameSet(edit.removed, edited.b.prints)) throw new Error(`the key removed ${edit.removed} (expected ${edited.b.prints})`);
          model.check('after the Delete key');
          await app.waitFor(selectionIs(0), 'the bar to go');
          await places.agree('after the Delete key');
        });

        // --- 4. Undo and redo, the tablet writing meanwhile ------------------------------------------------------------

        const undoable = (state) => `document.querySelector('button[data-action=${state}]').disabled === false`;

        await check('Ctrl+Z rend le second groupe, Ctrl+Y le supprime de nouveau ; un trait de la tablette écrit entre les deux reste', async () => {
          await app.waitFor(undoable('"undo-edit"'), 'the Undo button to be available');
          const kept = model.sync().tablet.length;
          const mark = stream.mark();
          await pressUntil(app, 'z', { ctrl: true }, () => model.editsSince(mark).length >= 1, 'the undo event');
          let [edit] = model.editsSince(mark);
          if (!sameSet(edit.inserted, edited.b.prints) || edit.removed.length > 0) throw new Error(`the undo inserted ${edit.inserted} (expected ${edited.b.prints})`);
          model.check('after Ctrl+Z');
          await places.agree('after Ctrl+Z');
          // The tablet writes a stroke between the undo and the redo: the redo must leave it alone.
          await until(() => model.sync().tablet.length > kept + 1, 'a stroke written by the tablet after the undo', 20_000);
          const written = model.tablet.slice(kept);
          const redoMark = stream.mark();
          await pressUntil(app, 'y', { ctrl: true }, () => model.editsSince(redoMark).length >= 1, 'the redo event');
          [edit] = model.editsSince(redoMark);
          if (!sameSet(edit.removed, edited.b.prints)) throw new Error(`the redo removed ${edit.removed} (expected ${edited.b.prints})`);
          model.check('after Ctrl+Y');
          if (written.some((print) => !model.strokes.some((s) => printOf(s) === print))) throw new Error('a stroke written by the tablet was lost by the redo');
          await places.agree('after Ctrl+Y');
        });

        await check('annuler tout (Ctrl+Z jusqu’au bout) : les traits rouges et verts redeviennent noirs, rien de la tablette ne disparaît ; rétablir tout', async () => {
          const written = [...model.sync().tablet];
          const before = model.strokes.length;
          let undone = 0;
          const steps = 4; // the red, the green, the two deletions
          for (; undone < steps; undone++) {
            const mark = stream.mark();
            await pressUntil(app, 'z', { ctrl: true }, () => model.editsSince(mark).length >= 1, `the event of undo ${undone + 1} (edits so far: ${describeEdits(model)})`);
          }
          await app.waitFor(`document.querySelector('button[data-action="undo-edit"]').disabled`, `Undo to be out of steps after ${steps} undos (edits so far: ${describeEdits(model)})`);
          model.check('after undoing everything');
          if (model.deleted.size !== 0) throw new Error(`${model.deleted.size} strokes are still deleted after undoing everything`);
          if (written.some((print) => !model.strokes.some((s) => printOf(s) === print))) throw new Error('Ctrl+Z took away a stroke the tablet wrote');
          if (model.strokes.length < before + edited.a.prints.length + edited.b.prints.length) throw new Error('the deleted strokes are not all back');
          const stats = await places.until((s) => everywhere(s, 'red', (r) => r < 0.0001) && everywhere(s, 'green', (r) => r < 0.0001), 'all ink black again in the three places');
          await places.agree('after undoing everything');
          // The Undo button is out of steps, and Ctrl+Z once more does nothing (the tablet's strokes are not steps).
          if (await app.evaluate(undoable('"undo-edit"'))) throw new Error('Undo is still offered after the last step');
          const mark = stream.mark();
          await press(app, 'z', { ctrl: true });
          await sleep(500);
          if (model.editsSince(mark).length > 0) throw new Error('Ctrl+Z with nothing to undo changed the page');
          // And all the way back.
          let redone = 0;
          for (; redone < steps; redone++) {
            const redoMark = stream.mark();
            await pressUntil(app, 'y', { ctrl: true }, () => model.editsSince(redoMark).length >= 1, `the event of redo ${redone + 1} (edits so far: ${describeEdits(model)})`);
          }
          await app.waitFor(`document.querySelector('button[data-action="redo-edit"]').disabled`, `Redo to be out of steps after ${steps} redos (edits so far: ${describeEdits(model)})`);
          model.check('after redoing everything');
          if (!sameSet([...model.deleted], [...edited.a.prints, ...edited.b.prints])) throw new Error(`deleted after the redo: ${[...model.deleted]}`);
          await places.until((s) => everywhere(s, 'green', (r) => r > 0.00004) && everywhere(s, 'red', (r) => r < 0.0001), 'the green strokes back and the red ones gone');
          console.log(`    ${undone} annulations, ${redone} rétablissements ; couleur noire rendue : ${j(Object.values(stats).map((s) => s && share(s, 'red').toExponential(1)))}`);
        });

        // --- 5. Clear the page --------------------------------------------------------------------------------------------

        await check('« Effacer la page » vide les trois endroits, puis Ctrl+Z rend la page (et garde ce que la tablette a écrit depuis)', async () => {
          model.check('before the clearing');
          const before = new Set(model.strokes.map(printOf));
          const mark = stream.mark();
          await app.click('button[data-action="clear-page"]');
          await until(() => model.sync() && [...before].every((p) => model.deleted.has(p)), 'the page to be empty in the stream');
          const cleared = await places.agree('after clearing the page');
          console.log(`    page effacée : ${j(cleared)}`);
          if (cleared.control > 3) throw new Error(`the page still holds ${cleared.control} strokes after the clearing`);
          await until(() => model.sync().strokes.length >= 1, 'the tablet to write on the cleared page', 20_000);
          const since = model.strokes.map(printOf).filter((p) => !before.has(p));
          await pressUntil(app, 'z', { ctrl: true }, () => model.editsSince(mark).some((e) => e.inserted.length > 0), 'the undo of the clearing');
          model.check('after undoing the clearing');
          const back = model.strokes.map(printOf);
          if ([...before].some((p) => !back.includes(p))) throw new Error('the page did not come back whole');
          if (since.some((p) => !back.includes(p))) throw new Error('a stroke written after the clearing was lost');
          if (!sameSet([...model.deleted], [...edited.a.prints, ...edited.b.prints])) throw new Error(`still deleted: ${[...model.deleted]}`);
          await places.agree('after undoing the clearing');
          await places.until((s) => everywhere(s, 'green', (r) => r > 0.00004), 'the green strokes back with the page');
        });

        await check('les trois endroits à la fin : mêmes traits (compte), même part de vert, aucun rouge', async () => {
          model.check('at the end');
          const counts = await places.agree('at the end');
          const stats = await places.stats();
          const greens = Object.entries(stats).filter(([, s]) => s).map(([name, s]) => [name, share(s, 'green')]);
          console.log(`    ${j(counts)} ; vert ${j(greens.map(([n, v]) => `${n} ${v.toExponential(2)}`))}`);
          const top = Math.max(...greens.map(([, v]) => v));
          const low = Math.min(...greens.map(([, v]) => v));
          if (!(low > 0) || top / low > 3) throw new Error(`the green ink differs between the places: ${j(greens)}`);
          if (!everywhere(stats, 'red', (r) => r < 0.0001)) throw new Error('red ink is left');
          if (!(await app.evaluate(`document.querySelector('.live-view') !== null`))) throw new Error('live mode closed');
          if (browser) assertNoBrowserProblem(browser);
          if (model.unended.length > 0) console.log(`    ${model.unended.length} traits sans strokeEnd dans le flux (le suivant a commencé avant) : ${j(model.unended.slice(0, 5))}`);
        });

        // --- 6. Stop, then the note ---------------------------------------------------------------------------------------

        let finalList = [];
        await check('Terminer : la note de la bibliothèque contient ce qui était à l’écran (nombre de traits et couleurs)', async () => {
          presentation.close();
          presentation = null;
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('close_presentation').then(() => true)`);
          model.sync();
          await stopLive(app);
          await sleep(300);
          model.sync();
          finalList = model.strokes.map((s) => ({ print: printOf(s), color: s.color }));
          const added = notesOf(app.library).filter((n) => !notesBefore.includes(n.path));
          if (added.length !== 1) throw new Error(`${added.length} new notes after the live session: ${added.map((n) => n.path).join(', ')}`);
          sheet = added[0];
          const file = decodeStrokes(sheet.members.get('strokes.bin'));
          console.log(`    flux : ${finalList.length} traits ; fichier : ${file.length} traits`);
          // The file holds the strokes of the screen, in order; the stroke in progress at the stop may be one more.
          if (file.length < finalList.length || file.length > finalList.length + 1) throw new Error(`the file holds ${file.length} strokes, the screen ${finalList.length}`);
          finalList.forEach((shown, k) => {
            if (String(file[k].points[0].t) !== shown.print) throw new Error(`stroke ${k}: the file has t=${file[k].points[0].t}, the screen ${shown.print}`);
            if (!sameColor(file[k].color, shown.color)) throw new Error(`stroke ${k}: the file has ${colorText(file[k].color)}, the screen ${colorText(shown.color)}`);
          });
          const colors = new Set(file.map((s) => colorText(s.color)));
          if (!colors.has(colorText(GREEN)) || !colors.has(colorText(BLACK))) throw new Error(`the file holds the colours ${[...colors].join(' / ')}`);
          if (colors.has(colorText(RED))) throw new Error('a red stroke (deleted) is in the file');
          for (const [p, color] of [[edited.a.prints, 'deleted'], [edited.b.prints, 'deleted']]) {
            if (p.some((print) => file.some((s) => String(s.points[0].t) === print))) throw new Error(`a stroke ${color} on screen is in the file`);
          }
        });

        await check('rouvrir la note dans l’éditeur : le même nombre de traits, les mêmes couleurs', async () => {
          const file = decodeStrokes(sheet.members.get('strokes.bin'));
          const name = sheet.path.split(/[\\/]/).at(-1);
          await app.click(`.note-card[data-path*=${j(name)}] .card-open`);
          await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length >= ${file.length}`, 'the strokes of the note');
          const fills = await app.evaluate(`[...document.querySelectorAll(${j(EDITOR_STROKES)})].sort((a, b) => a.dataset.stroke - b.dataset.stroke).map((p) => p.getAttribute('fill'))`);
          if (fills.length !== file.length) throw new Error(`the editor draws ${fills.length} strokes, the file holds ${file.length}`);
          fills.forEach((fill, k) => {
            const color = parseFill(fill);
            if (!sameColor(color, file[k].color)) throw new Error(`stroke ${k}: the editor draws ${j(fill)}, the file holds ${colorText(file[k].color)}`);
          });
          await closeEditor(app);
        });
      } finally {
        presentation?.close();
        stream?.close();
        await browser?.close();
      }
    },
  },

  {
    name: 'retouches en direct : sur une feuille reprise avec une image de fond, supprimer et recolorer un trait de l’ancienne note',
    args: ['--simulated-tablet'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    async run(app, rawCheck) {
      const check = limited(rawCheck);
      const l = (key) => text(app.language, key);
      const title = 'Fiche retouchée E2E';
      const file = '2026-10-28 Fiche retouchée E2E.ardoise';
      let made;
      let stream = null;
      let model = null;
      let map = null;
      const strokesNow = () => model.sync().strokes;

      try {
        await registerTablet(app, check);

        await check('une fiche (image de fond, trois traits, première version, texte, narration) est posée dans le classeur « Reprise »', async () => {
          made = await dropSheet(app, { binder: 'Reprise', file, title, index: 5 });
        });

        await check('reprendre la fiche : ses trois traits sont dans le flux du partage', async () => {
          await resumeFromCard(app, file);
          await app.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'the three strokes of the sheet', 20_000);
          await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'the tablet to stream', 30_000);
          const code = await startShare(app);
          stream = await joinStream(app, code);
          model = new StreamModel(stream).sync();
          await closePanel(app, l('common.close'));
          if (model.strokes.length < 3) throw new Error(`${model.strokes.length} strokes in the stream, expected the 3 of the sheet`);
        });

        await check('Sélection : un clic sur le trait du bas et « Supprimer » le retirent (un trait de l’ancienne note)', async () => {
          await app.click(SELECT_BUTTON);
          await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the Select tool on');
          map = await previewMap(app, model.page);
          const strokes = strokesNow();
          const order = strokes.slice(0, 3).map(printOf);
          if (order.join() !== ['0', '1500', '3000'].join()) throw new Error(`the sheet's strokes in the stream start at ${order}`);
          const mark = stream.mark();
          await clickAt(app, map.at(planClick(strokes, 2, map.scale, { lowFirst: true })));
          await app.waitFor(selectionIs(1), 'the base line selected');
          await app.click(`${BAR} button[data-action="delete-selection"]`);
          await until(() => model.editsSince(mark).length > 0, 'the strokesEdited event of the delete');
          const [edit] = model.editsSince(mark);
          if (!sameSet(edit.removed, ['3000']) || !sameSet(edit.removedIndices.map(String), ['2'])) throw new Error(`the delete removed ${edit.removed} at ${edit.removedIndices}`);
          model.check('after deleting an old stroke');
        });

        await check('un clic sur la première diagonale et l’échantillon Rouge la recolorent', async () => {
          const strokes = strokesNow();
          const mark = stream.mark();
          await clickAt(app, map.at(planClick(strokes, 0, map.scale, { lowFirst: true })));
          await app.waitFor(selectionIs(1), 'the diagonal selected');
          await app.click(`.swatches .swatch[aria-label=${j(l('editor.colors.red'))}]`);
          await until(() => model.editsSince(mark).length > 0, 'the strokesEdited event of the recolouring');
          const [edit] = model.editsSince(mark);
          if (!sameColor(edit.recoloured[0]?.color, RED) || !sameSet(edit.recoloured[0].prints, ['0'])) throw new Error(`recoloured: ${j(edit.recoloured)}`);
          model.check('after recolouring an old stroke');
          await press(app, 'Escape');
          // The tablet kept writing meanwhile: the sheet is still the page on screen.
          await app.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'the page on screen');
        });

        let after;
        await check('Terminer : la même note (même chemin), son image, son original et tous ses membres, avec les retouches dedans', async () => {
          model.sync();
          const written = model.tablet.length;
          await stopLive(app);
          const same = titled(app.library, title);
          if (same.length !== 1 || same[0].path !== made.path) throw new Error(`${same.length} notes titled ${title}: ${same.map((n) => n.path).join(', ')}`);
          after = same[0];
          for (const [name, bytes] of made.members) {
            if (name === 'note.json' || name === 'strokes.bin') continue;
            if (!after.members.has(name)) throw new Error(`the member ${name} is gone`);
            if (!after.members.get(name).equals(bytes)) throw new Error(`the member ${name} changed`);
          }
          const first = JSON.parse(made.members.get('note.json').toString('utf8'));
          for (const key of ['id', 'title', 'created', 'page', 'backgroundImage', 'background', 'origin', 'layers', 'x-journey']) {
            if (canon(after.note[key]) !== canon(first[key])) throw new Error(`note.json: ${key} is ${j(after.note[key])}, was ${j(first[key])}`);
          }
          const strokes = decodeStrokes(after.members.get('strokes.bin'));
          console.log(`    ${strokes.length} traits dans le fichier (${written} écrits par la tablette)`);
          if (strokes.some((s) => s.points[0].t === 3000)) throw new Error('the deleted base line is still in the file');
          const diag1 = strokes.find((s) => s.points[0].t === 0);
          const diag2 = strokes.find((s) => s.points[0].t === 1500);
          if (!diag1 || !sameColor(diag1.color, RED)) throw new Error(`the first diagonal is ${colorText(diag1?.color)} in the file, expected red`);
          if (!diag2 || !sameColor(diag2.color, BLACK)) throw new Error(`the second diagonal is ${colorText(diag2?.color)} in the file, expected black`);
          if (strokes.length < 2 + written) throw new Error(`${strokes.length} strokes in the file, expected 2 old ones and ${written} of the tablet`);
        });

        await check('rouvrir la note : l’image de fond, l’original (« Revenir à l’original ») et les deux anciens traits retouchés sont là', async () => {
          const count = strokesOf(after.members.get('strokes.bin')).count;
          await openInEditor(app, file, count);
          const facts = await app.evaluate(`({
            image: document.querySelector('[data-section="background-image"] [data-slot="background-image-name"]')?.textContent ?? null,
            strokes: document.querySelectorAll(${j(EDITOR_STROKES)}).length,
            original: document.querySelector('[data-section="original"] button[data-action="original"]')?.disabled === false,
            fills: [...document.querySelectorAll(${j(EDITOR_STROKES)})].sort((a, b) => a.dataset.stroke - b.dataset.stroke).map((p) => p.getAttribute('fill')),
          })`);
          if (facts.image !== IMAGE_NAME) throw new Error(`background image: ${j(facts.image)}`);
          if (!facts.original) throw new Error(`« ${l('editor.original.button')} » is not available`);
          if (facts.strokes !== count) throw new Error(`${facts.strokes} strokes in the editor, ${count} in the file`);
          const reds = facts.fills.map(parseFill).filter((c) => sameColor(c, RED)).length;
          if (reds !== 1) throw new Error(`${reds} red strokes in the editor, expected 1`);
          await closeEditor(app);
          if (statSync(made.path).size === 0 || !readZip(readFileSync(made.path)).get('original.ardoise').equals(made.members.get('original.ardoise'))) throw new Error('the first version changed');
        });

      } finally {
        stream?.close();
      }
    },
  },
];
