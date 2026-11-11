// Milestone M14 journey « corriger pendant le direct » (SPEC §4.2 « Corriger pendant le direct », docs/questions.md Q11,
// docs/design/tablet-live-classroom.md, "M14: correct during live on one's own layer", "End-to-end journeys").
//
// On the production executable with the simulated tablet (`--simulated-tablet`), in French and in English. The simulated
// pen writes the whole time; the teacher's pen (the computer's) is `Input.dispatchMouseEvent` with `pointerType: 'pen'`
// and `force` (as `journeys-m8-input.mjs`). Three places are compared: the control window (its preview), the presentation
// window (a second DevTools target of the same process) and the share, read by a Node client of the event stream that
// builds the page from the messages (`TwoPens`: the strokes in progress of both pens, `strokeCancel`, `strokesEdited`).
//
// The steps: the layers zone (one row, chosen, with the tablet); « Ajouter un calque » (a second row, chosen, the tablet
// still on the first), renamed « Correction »; Pen (P); a stroke of the computer while the tablet writes, seen by the
// projector and by the share before the pen is lifted, its points on the stream in less than 50 ms (median); Ctrl+Z and
// Ctrl+Y on it, the tablet's strokes all kept; the pen's eraser end across a tablet stroke and the computer's stroke (only
// the computer's goes, Ctrl+Z gives it back); « Calque 1 » chosen, a tablet stroke erased then given back by Ctrl+Z; the
// tablet sent to « Correction » (its next strokes come with layer 1); the tablet's layer hidden (the warning; the share gets
// no tablet stroke for 2 s while the preview's count grows), shown again; « Terminer », then the note's file: its layers,
// the tablet's strokes on the layers they were on, the computer's stroke on layer 1 in its ink (red felt tip 120), its time
// within 150 ms of the pen down and between the tablet's strokes written just before and just after it.
//
// The eraser end: the DevTools protocol may drop the eraser button (32, `journeys-m8-input.mjs`). It is tried once with the
// Pen tool off (a plain tip there only grabs the projected area of a whole page, which does not move); when the page did
// not see the button, the eraser's gestures are `PointerEvent`s built in the page (`pointerType: 'pen'`, `buttons: 32`)
// and dispatched on the preview, and the log says so.
//
// Written without a run: package B (the Rust side: `live_computer_pen`, `live_layers`, the events) was not merged when this
// was written. What it relies on is in docs/design/tablet-live-classroom.md, "Contracts between packages".
import { closePanel, EventStream, request, until } from './journeys-m11-share.mjs';
import { attach, canvasData, LIVE_CANVAS, notesOf, registerTablet, STATUS, stopLive, text } from './journeys-m12-resume.mjs';
import { decodeStrokes } from './journeys-m12-touchup.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const wallMs = () => performance.timeOrigin + performance.now();
const PREVIEW_MARGIN = 12;
/** The presentation window covers the control window: without these flags the control window would stop painting. */
const BROWSER_ARGS = '--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding';
/** Where the control window keeps the computer's ink (`lib/live-computer-style.ts`): removed, the default applies. */
const STYLE_KEY = 'ardoise.live.computerStyle';
/** The computer's default ink: felt tip (tool code 2 of docs/file-format.md), Inkspace red, width 120. */
const FELT_TIP = 2;
const RED = { r: 216, g: 0, b: 0, a: 255 };
const COMPUTER_WIDTH = 120;
/** The share's figure for the computer's points (`recv` → arrival in Node), as M11 measures the tablet's. */
const SHARE_MEDIAN_MS = 50;
/** The computer stroke's first `t` and the wall time of the pen down, on the page's clock. */
const TIME_WITHIN_MS = 150;
/** For so long the share must get no stroke of the tablet once its layer is hidden. */
const HIDDEN_QUIET_MS = 2000;
/** The foot of the page, below the lines of the simulated pen (its last baseline is at 19 600, as for the M12 pixel sheet). */
const FOOT_Y = 20_700;

const ZONE = 'section.layers-zone[data-where="live"]';
const row = (index) => `${ZONE} li.layer[data-layer="${index}"]`;
const inRow = (index, action) => `${row(index)} [data-action="${action}"]`;
const PEN_BUTTON = '[data-slot="touch-up"] button[data-action="pen"]';

/** Page expression: the zone's rows, top first. */
const zoneRows = `[...document.querySelectorAll(${j(`${ZONE} li.layer`)})].map((li) => ({
  index: Number(li.dataset.layer),
  chosen: li.dataset.chosen === 'true',
  tablet: li.dataset.tablet === 'true',
  hidden: li.dataset.hidden === 'true',
  name: (li.querySelector('.layer-name')?.textContent ?? li.querySelector('input.layer-rename')?.value ?? '').trim(),
}))`;
/** Page expression: the kinds of the warnings the zone shows. */
const zoneWarnings = `[...document.querySelectorAll(${j(`${ZONE} [data-slot="layer-warning"]`)})].map((p) => p.dataset.kind)`;
const countOf = (selector) => `Number(document.querySelector(${j(selector)})?.dataset.strokes ?? -1)`;

/** Where the journey is, for the message of a step that does not end (a DevTools call that never answers). */
let lastPoint = '';
const at = (point) => {
  lastPoint = point;
};

/** `check` with a limit on each step, as `journeys-m12-touchup.mjs`: a window that stops answering fails the step. */
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

/** `promise`, or an error naming the window that did not answer within `ms`. */
function within(promise, ms, what) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} did not answer within ${ms / 1000} s`)), ms);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

// --- What the stream says -------------------------------------------------------------------------------------------

/**
 * A stroke's identity: its first point (time, and place rounded to the unit). The tablet's `t` alone is unique (5 ms apart);
 * a stroke of the computer may start in the same millisecond as one of the tablet, not at the same place.
 */
const printOf = (stroke) => {
  const p = stroke.points[0];
  return `${p.t}|${Math.round(p.x)}|${Math.round(p.y)}`;
};
const penOf = (event) => (event.pen === 'computer' ? 'computer' : 'tablet');

/**
 * The page as a viewer of the share builds it from `/events`, with the rules of `LiveState` for two pens: the finished
 * strokes, one stroke in progress per pen (`pen: "computer"` or nothing), `strokeCancel` dropping that pen's, a snapshot
 * dropping both. Each finished stroke keeps its pen and its layer (from its `strokeStart`). `registry` is the order in
 * which strokes ended (the list's order: a stroke joins the tail when it ends); `deleted` the strokes an edit took out
 * and did not give back. Every event is kept with its message (`log`) to say what came when.
 */
class TwoPens {
  constructor(stream) {
    this.stream = stream;
    this.read = 0;
    this.page = null;
    this.pageIndex = -1;
    this.layers = [];
    this.strokes = [];
    this.current = { tablet: null, computer: null };
    this.registry = new Map();
    /** print → { pen, layer } */
    this.info = new Map();
    this.deleted = new Set();
    this.edits = [];
    /** Every event: { i (message), at (arrival, wall ms), reset, event }. */
    this.log = [];
  }

  register(stroke, pen) {
    const print = printOf(stroke);
    if (!this.registry.has(print)) this.registry.set(print, this.registry.size);
    if (!this.info.has(print)) this.info.set(print, { pen, layer: stroke.layer ?? null });
    return print;
  }

  count() {
    return this.strokes.length + (this.current.tablet ? 1 : 0) + (this.current.computer ? 1 : 0);
  }

  health() {
    const last = this.stream.messages.at(-1);
    return `last message ${last ? Math.round(wallMs() - last.at) : '-'} ms ago, ${this.stream.closed ? 'closed' : 'open'}`;
  }

  sync() {
    if (this.stream.failure) throw new Error(this.stream.failure);
    if (this.stream.closed && this.read >= this.stream.messages.length) throw new Error('the share closed the event stream of this client');
    for (; this.read < this.stream.messages.length; this.read++) {
      const entry = this.stream.messages[this.read];
      if (entry.msg.type !== 'live') continue;
      for (const event of entry.msg.batch.events) {
        this.log.push({ i: entry.i, at: entry.at, reset: entry.msg.reset === true, event });
        this.apply(event, entry.i);
      }
    }
    return this;
  }

  finish(pen) {
    const stroke = this.current[pen];
    this.current[pen] = null;
    if (!stroke || stroke.points.length === 0) return;
    this.register(stroke, pen);
    this.strokes.push(stroke);
  }

  apply(event, i) {
    const pen = penOf(event);
    switch (event.type) {
      case 'snapshot': {
        const { snapshot } = event;
        const before = new Set(this.strokes.map(printOf));
        this.page = snapshot.page.page;
        this.pageIndex = snapshot.pageIndex;
        this.layers = snapshot.page.layers;
        this.strokes = [...snapshot.page.strokes];
        const after = new Set(this.strokes.map((stroke) => this.register(stroke, this.info.get(printOf(stroke))?.pen ?? 'tablet')));
        for (const print of before) if (!after.has(print)) this.deleted.add(print);
        for (const print of after) this.deleted.delete(print);
        this.current = { tablet: null, computer: null };
        break;
      }
      case 'strokeStart':
        if (event.pageIndex !== this.pageIndex) break;
        // As `LiveState`: a stroke of a pen that starts ends that pen's stroke in progress, never the other pen's.
        if (this.current[pen]) this.finish(pen);
        this.current[pen] = { ...event.stroke, points: [...event.stroke.points], startedIn: i };
        break;
      case 'points':
        if (event.pageIndex !== this.pageIndex) break;
        if (!this.current[pen]) this.current[pen] = { layer: null, color: null, points: [] };
        this.current[pen].points.push(...event.points);
        break;
      case 'strokeEnd':
        if (event.pageIndex === this.pageIndex) this.finish(pen);
        else this.current[pen] = null;
        break;
      case 'strokeCancel':
        this.current[pen] = null;
        break;
      case 'strokesEdited': {
        if (event.pageIndex !== this.pageIndex) break;
        const removed = event.removed.map((index) => printOf(this.strokes[index]));
        const gone = new Set(event.removed);
        this.strokes = this.strokes.filter((_, index) => !gone.has(index));
        for (const { index, stroke } of event.inserted) this.strokes.splice(index, 0, stroke);
        const inserted = event.inserted.map(({ stroke }) => this.register(stroke, this.info.get(printOf(stroke))?.pen ?? 'tablet'));
        for (const { color, indices } of event.recoloured) for (const index of indices) this.strokes[index].color = color;
        for (const print of removed) this.deleted.add(print);
        for (const print of inserted) this.deleted.delete(print);
        this.edits.push({ i, keys: Object.keys(event).sort().join(), removed, inserted });
        break;
      }
      default:
        break;
    }
  }

  /** Throws unless the list is the registry less the deleted strokes, in the registry's order. */
  check(what) {
    this.sync();
    const prints = this.strokes.map(printOf);
    for (let k = 1; k < prints.length; k++) {
      if (!(this.registry.get(prints[k]) > this.registry.get(prints[k - 1]))) {
        throw new Error(`${what}: the strokes are not in their order at place ${k} (${prints[k - 1]} then ${prints[k]})`);
      }
    }
    const expected = [...this.registry.keys()].filter((print) => !this.deleted.has(print));
    if (expected.length !== prints.length || expected.some((print, k) => print !== prints[k])) {
      throw new Error(`${what}: the list holds ${prints.length} strokes, the registry less the deleted ones ${expected.length}`);
    }
  }

  editsSince(from) {
    this.sync();
    return this.edits.filter((edit) => edit.i >= from);
  }

  /** The events of messages from index `from` on. */
  eventsSince(from) {
    this.sync();
    return this.log.filter((entry) => entry.i >= from);
  }

  /** The finished strokes of `pen` (prints), in the list's order. */
  finished(pen) {
    this.sync();
    return this.strokes.map(printOf).filter((print) => this.info.get(print)?.pen === pen);
  }
}

// --- The three places ---------------------------------------------------------------------------------------------------

class Places {
  constructor({ app, model, presentation }) {
    this.app = app;
    this.model = model;
    this.presentation = presentation;
  }

  async counts() {
    at('reading the strokes of the three places');
    this.model.sync();
    const [control, presented] = await Promise.all([
      within(this.app.evaluate(countOf(LIVE_CANVAS)), 10_000, 'the control window'),
      this.presentation ? within(this.presentation.evaluate(countOf(LIVE_CANVAS)), 10_000, 'the presentation window') : null,
    ]);
    this.model.sync();
    return { control, presented, stream: this.model.count() };
  }

  /** Waits until the places hold the same number of strokes (the tablet adds one now and then: read again). */
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
}

// --- Input ----------------------------------------------------------------------------------------------------------------

/** A pen event of the DevTools protocol (`journeys-m8-input.mjs`): `type` is mousePressed / mouseMoved / mouseReleased. */
const pen = (app, type, x, y, { buttons = 0, force = 0.5 } = {}) =>
  app.send('Input.dispatchMouseEvent', {
    type,
    x,
    y,
    pointerType: 'pen',
    button: type === 'mouseMoved' && buttons === 0 ? 'none' : 'left',
    buttons,
    force,
    tiltX: 10,
    tiltY: -5,
    clickCount: type === 'mouseMoved' ? 0 : 1,
  });

/** A key press on the control window. */
const press = (app, name, options) => {
  at(`key ${name}${options?.ctrl ? ' with Ctrl' : ''}`);
  return app.key(name, options);
};

/**
 * A key press made again when nothing came of it: Ctrl+Z is ignored while a gesture of the computer is under way and one
 * edit is handled at a time (`lib/live-touch-up.ts`). `done()` says whether the press had its effect.
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

/** A wave across the foot of the page, in page units: two periods, `n` segments. */
const wave = (x0, x1, y, n, amplitude = 250) =>
  Array.from({ length: n + 1 }, (_, i) => ({ x: x0 + ((x1 - x0) * i) / n, y: y + amplitude * Math.sin((i / n) * Math.PI * 4) }));

/** Window pixels every `step` pixels from `a` to `b`. */
const segment = (a, b, step = 4) => {
  const n = Math.max(2, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y) / step));
  return Array.from({ length: n + 1 }, (_, i) => ({ x: a.x + ((b.x - a.x) * i) / n, y: a.y + ((b.y - a.y) * i) / n }));
};

/** Page expression: a gesture of the pen's eraser end built in the page (`buttons: 32`) along window points, a move every 8 ms. */
const syntheticEraser = (points) => `(async () => {
  const c = document.querySelector(${j(LIVE_CANVAS)});
  const points = ${j(points)};
  const fire = (type, p, buttons, button) =>
    c.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        composed: true,
        pointerId: 41,
        pointerType: 'pen',
        isPrimary: true,
        clientX: p.x,
        clientY: p.y,
        buttons,
        button,
        pressure: buttons ? 0.5 : 0,
      }),
    );
  fire('pointermove', points[0], 0, -1);
  fire('pointerdown', points[0], 32, 5);
  for (const p of points.slice(1)) {
    await new Promise((resolve) => setTimeout(resolve, 8));
    fire('pointermove', p, 32, -1);
  }
  fire('pointerup', points[points.length - 1], 0, 5);
  return points.length;
})()`;

/**
 * Whether the DevTools protocol brings the eraser button (32) to the page: one press on an empty corner of the page with
 * the Pen tool off (with the tool off, a press there that came as a plain tip grabs the projected area, which cannot move
 * while it shows the whole page).
 */
async function eraserReachesThePage(app, where) {
  await app.evaluate(`(() => {
    window.__eraserSeen = false;
    document.querySelector(${j(LIVE_CANVAS)}).addEventListener('pointerdown', (e) => {
      if ((e.buttons & 32) !== 0 || e.button === 5) window.__eraserSeen = true;
    }, { capture: true, once: true });
    return true;
  })()`);
  await pen(app, 'mouseMoved', where.x, where.y);
  await pen(app, 'mousePressed', where.x, where.y, { buttons: 32 });
  await pen(app, 'mouseReleased', where.x, where.y);
  await sleep(100);
  return (await app.evaluate('window.__eraserSeen === true')) === true;
}

/** Joins the share with the code, as a browser would: the cookie for `/events` (as `journeys-m12-touchup.mjs`). */
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

const sameColor = (a, b) => !!a && !!b && a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
const colorText = (c) => (c ? `${c.r},${c.g},${c.b},${c.a}` : 'none');

/** The stroke of the file whose first point is the stream's stroke `print` (the file keeps a hundredth of a unit). */
function inFile(file, print) {
  const [t, x, y] = print.split('|').map(Number);
  return file.find((s) => s.points[0]?.t === t && Math.abs(s.points[0].x - x) <= 1 && Math.abs(s.points[0].y - y) <= 1) ?? null;
}

/** The points of the computer's stroke events since message `from`, with the delay of their message (`recv` → arrival). */
function computerDelays(model, from) {
  const delays = [];
  for (const { i, at: arrival, reset, event } of model.eventsSince(from)) {
    // A batch that starts the stream again carries a stroke received long ago.
    if (i < from || reset || event.pen !== 'computer') continue;
    const points = event.type === 'points' ? event.points : event.type === 'strokeStart' ? event.stroke.points : [];
    for (const point of points) if (typeof point.recv === 'number') delays.push(arrival - point.recv);
  }
  return delays;
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted.length === 0 ? NaN : sorted.length % 2 ? sorted[(sorted.length - 1) / 2] : (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2;
};

// --- The journey -----------------------------------------------------------------------------------------------------------

export const liveComputerJourneys = [
  {
    name: 'corriger pendant le direct : l’ordinateur écrit sur « Correction » pendant que la tablette écrit',
    args: ['--simulated-tablet'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    browserArgs: BROWSER_ARGS,
    async run(app, rawCheck) {
      const check = limited(rawCheck);
      const l = (key) => text(app.language, key);
      const firstLayer = l('editor.layers.defaultName').replace('{n}', '1');
      let presentation = null;
      let stream = null;
      let model = null;
      let places = null;
      let map = null;
      let notesBefore = [];
      /** The computer's stroke: its print, the wall time of the pen down, the places of its start and end in the model's log. */
      const computer = { print: null, downAt: 0, startedIn: -1, endedIn: -1 };
      let eraserByProtocol = false;

      const replan = async () => {
        map = await previewMap(app, model.sync().page);
        return map;
      };
      const rows = () => app.evaluate(zoneRows);
      const warnings = () => app.evaluate(zoneWarnings);
      /** An eraser gesture along page points: by the protocol when it carries the button, else built in the page. */
      const erase = async (pagePoints) => {
        at('an eraser gesture');
        await replan();
        const points = [];
        for (let k = 1; k < pagePoints.length; k++) points.push(...segment(map.at(pagePoints[k - 1]), map.at(pagePoints[k])).slice(k === 1 ? 0 : 1));
        if (eraserByProtocol) {
          const [first, ...rest] = points;
          await pen(app, 'mouseMoved', first.x, first.y);
          await pen(app, 'mousePressed', first.x, first.y, { buttons: 32 });
          for (const p of rest) {
            await pen(app, 'mouseMoved', p.x, p.y, { buttons: 32 });
            await sleep(8);
          }
          await pen(app, 'mouseReleased', rest.at(-1).x, rest.at(-1).y);
        } else {
          await app.evaluate(syntheticEraser(points));
        }
      };
      /** A finished stroke of the tablet on `layer`, away from where its pen is now, and one of its middle points. */
      const tabletStroke = (layer, { near = null } = {}) => {
        model.sync();
        const pen = model.current.tablet?.points.at(-1) ?? null;
        const candidates = model.strokes.filter((s) => {
          const info = model.info.get(printOf(s));
          return info?.pen === 'tablet' && info.layer === layer && s.points.length >= 6;
        });
        const middle = (s) => s.points[Math.floor(s.points.length / 2)];
        const far = candidates.filter((s) => !pen || Math.hypot(middle(s).x - pen.x, middle(s).y - pen.y) > 4000);
        const pool = far.length > 0 ? far : candidates;
        if (pool.length === 0) return null;
        const distance = (s) => (near ? Math.hypot(middle(s).x - near.x, middle(s).y - near.y) : 0);
        const stroke = [...pool].sort((a, b) => distance(a) - distance(b))[0];
        return { stroke, print: printOf(stroke), point: middle(stroke) };
      };

      try {
        await check('l’encre de l’ordinateur par défaut : rien n’est retenu dans le navigateur', async () => {
          await app.evaluate(`(() => { try { localStorage.removeItem(${j(STYLE_KEY)}); } catch {} return true; })()`);
        });
        await registerTablet(app, check);
        await check('le dossier avant le direct', async () => {
          notesBefore = notesOf(app.library).map((n) => n.path);
        });

        await check('ouvrir le mode direct (tablette simulée) : l’encre de la tablette arrive', async () => {
          await openLive(app);
        });

        await check('projeter : la fenêtre de présentation dessine la page', async () => {
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
          presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window');
          await presentation.waitFor(`Number(${canvasData('strokes')}) >= 1`, 'the strokes in the presentation');
          await presentation.waitFor(`typeof window.__ardoiseLatency?.computer?.samples === 'function'`, 'the latency of the computer’s points in the presentation');
        });

        await check('partage : un client Node du flux a rejoint', async () => {
          const code = await startShare(app);
          stream = await joinStream(app, code);
          model = new TwoPens(stream).sync();
          places = new Places({ app, model, presentation });
          await closePanel(app, l('common.close'));
          if (await app.evaluate(`document.querySelector('.share-panel') !== null`)) throw new Error('the share panel stays open and would take the keys');
          await until(() => model.sync().finished('tablet').length >= 3, 'three strokes of the tablet in the stream', 60_000);
        });

        // --- The layers zone -----------------------------------------------------------------------------------------

        await check('la zone des calques : une ligne, choisie, avec la tablette ; plus de bouton « Nouveau calque »', async () => {
          await app.waitFor(`document.querySelector(${j(ZONE)}) !== null`, 'the layers zone of the live window');
          const list = await rows();
          if (list.length !== 1) throw new Error(`the zone has ${list.length} rows: ${j(list)}`);
          if (!list[0].chosen || !list[0].tablet || list[0].index !== 0) throw new Error(`the row is not chosen and the tablet's: ${j(list)}`);
          if (list[0].name !== firstLayer) throw new Error(`the layer is named ${j(list[0].name)}, expected ${j(firstLayer)}`);
          if (await app.evaluate(`document.querySelector('button[data-action="new-layer"]') !== null`)) throw new Error('the old « new layer » button is still there');
        });

        await check('« Ajouter un calque » : une deuxième ligne, choisie ; la tablette reste sur la première', async () => {
          await app.click(`${ZONE} button[data-action="add-layer"]`);
          await app.waitFor(`${zoneRows}.length === 2`, 'a second row');
          await app.waitFor(`document.querySelector(${j(`${row(1)}[data-chosen="true"]`)}) !== null`, 'the new layer chosen');
          const list = await rows();
          if (list.map((r) => r.index).join() !== '1,0') throw new Error(`the rows are not top first: ${j(list)}`);
          if (!list[1].tablet || list[0].tablet) throw new Error(`the tablet moved: ${j(list)}`);
          if (list[1].chosen) throw new Error('two rows are chosen');
        });

        await check('renommer le nouveau calque « Correction » (crayon, nom, Entrée)', async () => {
          await app.click(inRow(1, 'rename-layer'));
          await app.waitFor(`document.activeElement?.matches?.(${j(`${row(1)} input.layer-rename`)}) === true`, 'the name field, focused');
          // The field's text is selected: typing replaces it.
          await app.evaluate(`(() => { document.activeElement.select(); return true; })()`);
          await app.type('Correction');
          await press(app, 'Enter');
          await app.waitFor(`(document.querySelector(${j(`${row(1)} .layer-name`)})?.textContent ?? '').trim() === 'Correction'`, 'the new name');
          if (await app.evaluate(`document.querySelector(${j(`${ZONE} input.layer-rename`)}) !== null`)) throw new Error('the name field is still open');
        });

        await check('« Stylo » (touche P) : l’outil est actif, l’aide du stylo s’affiche', async () => {
          await press(app, 'p');
          await app.waitFor(`document.querySelector(${j(PEN_BUTTON)})?.getAttribute('aria-pressed') === 'true'`, 'the Pen tool on');
          await app.waitFor(`document.querySelector(${j(`${LIVE_CANVAS}[data-tool="pen"]`)}) !== null`, 'the canvas to take the pen tool');
          await app.waitFor(`document.querySelector('[data-slot="pen-hint"]') !== null`, 'the pen’s help');
          if (await app.evaluate(`document.querySelector('button[data-action="select"]').getAttribute('aria-pressed') === 'true'`)) throw new Error('Select is on with Pen');
        });

        // --- The computer's stroke ----------------------------------------------------------------------------------

        await check('un trait de l’ordinateur pendant que la tablette écrit : vu par le vidéoprojecteur et par le partage avant que le stylet se lève', async () => {
          await places.agree('before the computer’s stroke');
          await replan();
          const path = wave(6000, 16_000, FOOT_Y, 160).map(map.at);
          const half = 80;
          const mark = stream.mark();
          const samplesBefore = await presentation.evaluate(`window.__ardoiseLatency.computer.samples().length`);
          at('the computer’s pen down');
          await pen(app, 'mouseMoved', path[0].x, path[0].y);
          // The system's wall clock, as Rust's `SystemTime` and the note's `created`.
          computer.downAt = Date.now();
          await pen(app, 'mousePressed', path[0].x, path[0].y, { buttons: 1, force: 0.5 });
          for (const p of path.slice(1, half)) {
            await pen(app, 'mouseMoved', p.x, p.y, { buttons: 1, force: 0.6 });
            await sleep(10);
          }
          // The pen is still down: the stream has the stroke's start and its points, the windows count it.
          const started = await until(
            () => model.eventsSince(mark).find((e) => e.event.type === 'strokeStart' && e.event.pen === 'computer'),
            'a strokeStart with pen "computer" in the stream while the pen is down',
            10_000,
          );
          await until(
            () => model.eventsSince(mark).some((e) => e.event.type === 'points' && e.event.pen === 'computer'),
            'points of the computer in the stream while the pen is down',
            10_000,
          );
          if ('id' in started.event) throw new Error('the share sends the window’s number of the stroke (`id`)');
          // Places in the model's log of events: two pens' events may share a message (a batch of the hub).
          computer.startedIn = model.log.indexOf(started);
          computer.print = printOf(started.event.stroke);
          if (started.event.stroke.layer !== 1) throw new Error(`the computer writes on layer ${started.event.stroke.layer}, expected 1 (« Correction »)`);
          if (!model.current.computer) throw new Error('the stream has no stroke of the computer in progress');
          const counts = await places.agree('the computer’s stroke in progress');
          await presentation.waitFor(`window.__ardoiseLatency.computer.samples().length > ${samplesBefore}`, 'the presentation to draw the computer’s points');
          console.log(`    trait de l’ordinateur en cours : ${j(counts)}`);
          for (const p of path.slice(half)) {
            await pen(app, 'mouseMoved', p.x, p.y, { buttons: 1, force: 0.6 });
            await sleep(10);
          }
          await pen(app, 'mouseReleased', path.at(-1).x, path.at(-1).y);
          at('the computer’s stroke ends');
          const ended = await until(
            () => model.eventsSince(mark).find((e) => e.event.type === 'strokeEnd' && e.event.pen === 'computer'),
            'the strokeEnd of the computer in the stream',
            10_000,
          );
          computer.endedIn = model.log.indexOf(ended);
          if (!model.finished('computer').includes(computer.print)) throw new Error('the computer’s stroke is not among the finished strokes of the stream');
          const between = model.log
            .slice(computer.startedIn + 1, computer.endedIn)
            .filter((e) => penOf(e.event) === 'tablet' && (e.event.type === 'points' || e.event.type === 'strokeStart'));
          if (between.length === 0) throw new Error('the tablet sent no point while the computer wrote: the two pens were not tested together');
          const delays = computerDelays(model, mark);
          const m = median(delays);
          console.log(`    partage : ${delays.length} points de l’ordinateur, médiane ${m.toFixed(1)} ms de la réception à l’arrivée ; ${between.length} évènements de la tablette pendant le trait`);
          if (!(m < SHARE_MEDIAN_MS)) throw new Error(`the computer’s points reach the share in ${m.toFixed(1)} ms (median), expected under ${SHARE_MEDIAN_MS}`);
          const latency = await presentation.evaluate(`window.__ardoiseLatency.computer.median()`);
          console.log(`    vidéoprojecteur : latence médiane du stylet de l’ordinateur ${latency === null ? '-' : latency.toFixed(1)} ms`);
          model.check('after the computer’s stroke');
          await places.agree('after the computer’s stroke');
        });

        await check('Ctrl+Z retire le trait de l’ordinateur des trois endroits, les traits de la tablette restent ; Ctrl+Y le rend', async () => {
          const tablet = model.finished('tablet');
          const mark = stream.mark();
          await pressUntil(app, 'z', { ctrl: true }, () => model.editsSince(mark).some((e) => e.removed.includes(computer.print)), 'the undo of the computer’s stroke');
          const [edit] = model.editsSince(mark);
          if (edit.removed.join() !== computer.print || edit.inserted.length > 0) throw new Error(`the undo removed ${j(edit.removed)} and inserted ${j(edit.inserted)}`);
          model.check('after Ctrl+Z');
          const kept = model.finished('tablet');
          if (tablet.some((print) => !kept.includes(print))) throw new Error('Ctrl+Z took away a stroke the tablet wrote');
          await places.agree('after Ctrl+Z');
          const redo = stream.mark();
          await pressUntil(app, 'y', { ctrl: true }, () => model.editsSince(redo).some((e) => e.inserted.includes(computer.print)), 'the redo of the computer’s stroke');
          model.check('after Ctrl+Y');
          if (!model.finished('computer').includes(computer.print)) throw new Error('the computer’s stroke did not come back');
          await places.agree('after Ctrl+Y');
          if (model.log.some((e) => e.event.type === 'history')) throw new Error('a `history` event reached the share');
        });

        // --- The eraser end -----------------------------------------------------------------------------------------

        await check('le bout gomme en travers d’un trait de la tablette et du trait de l’ordinateur : seul celui de l’ordinateur part ; Ctrl+Z le rend', async () => {
          // Pen off: a press of the eraser end that would come as a plain tip must not write.
          await press(app, 'p');
          await app.waitFor(`document.querySelector(${j(PEN_BUTTON)})?.getAttribute('aria-pressed') === 'false'`, 'the Pen tool off');
          await replan();
          eraserByProtocol = await eraserReachesThePage(app, map.at({ x: 300, y: 300 }));
          console.log(
            eraserByProtocol
              ? '    bout gomme : le protocole DevTools transmet le bouton 32'
              : '    bout gomme : le protocole DevTools ne transmet pas le bouton 32 ; gestes construits dans la page (PointerEvent, pointerType "pen", buttons 32)',
          );
          const computerStroke = model.strokes.find((s) => printOf(s) === computer.print);
          const onComputer = computerStroke.points[Math.floor(computerStroke.points.length / 2)];
          const target = await until(() => tabletStroke(0, { near: onComputer }), 'a finished stroke of the tablet on the first layer', 20_000);
          const mark = stream.mark();
          await erase([target.point, onComputer]);
          await until(() => model.editsSince(mark).some((e) => e.removed.includes(computer.print)), 'the eraser to take the computer’s stroke', 10_000);
          // Time for a delete of a later move of the gesture.
          await sleep(800);
          const removed = model.editsSince(mark).flatMap((e) => e.removed);
          if (removed.join() !== computer.print) throw new Error(`the eraser took ${j(removed)}, expected only the computer’s stroke ${computer.print}`);
          if (!model.finished('tablet').includes(target.print)) throw new Error('the eraser took the tablet’s stroke on the other layer');
          await places.agree('after the eraser');
          const undo = stream.mark();
          await pressUntil(app, 'z', { ctrl: true }, () => model.editsSince(undo).some((e) => e.inserted.includes(computer.print)), 'the undo of the eraser');
          model.check('after undoing the eraser');
          await places.agree('after undoing the eraser');
        });

        await check('choisir « Calque 1 » (clic sur sa ligne) ; le bout gomme efface un trait de la tablette ; Ctrl+Z le rend', async () => {
          await app.click(row(0));
          await app.waitFor(`document.querySelector(${j(`${row(0)}[data-chosen="true"]`)}) !== null`, 'the first layer chosen');
          if ((await rows()).find((r) => r.index === 0)?.tablet !== true) throw new Error('choosing the layer moved the tablet');
          const target = await until(() => tabletStroke(0), 'a finished stroke of the tablet away from its pen', 20_000);
          const mark = stream.mark();
          const p = target.point;
          await erase([{ x: p.x - 150, y: p.y - 150 }, p, { x: p.x + 150, y: p.y + 150 }]);
          await until(() => model.editsSince(mark).some((e) => e.removed.includes(target.print)), 'the eraser to take the tablet’s stroke', 10_000);
          await sleep(800);
          const removed = model.editsSince(mark).flatMap((e) => e.removed);
          if (removed.includes(computer.print)) throw new Error('the eraser took the computer’s stroke, on the layer not chosen');
          if (removed.some((print) => model.info.get(print)?.pen !== 'tablet' || model.info.get(print)?.layer !== 0)) throw new Error(`the eraser took strokes of another layer: ${j(removed)}`);
          await places.agree('after erasing a stroke of the tablet');
          const undo = stream.mark();
          await pressUntil(app, 'z', { ctrl: true }, () => removed.every((print) => model.editsSince(undo).some((e) => e.inserted.includes(print))), 'the undo to give the tablet’s strokes back');
          model.check('after undoing the eraser of the first layer');
          if (!model.finished('tablet').includes(target.print)) throw new Error('the tablet’s stroke did not come back');
          await places.agree('after undoing the eraser of the first layer');
          console.log(`    ${removed.length} trait(s) de la tablette effacé(s) puis rendu(s)`);
        });

        // --- The tablet's layer ---------------------------------------------------------------------------------------

        await check('envoyer la tablette sur « Correction » (l’icône pâle au survol) : ses traits suivants arrivent avec layer 1', async () => {
          const where = await app.centre(row(1));
          await app.mouse('mouseMoved', where.x, where.y);
          await sleep(150);
          const mark = stream.mark();
          await app.click(inRow(1, 'tablet-layer'));
          await app.waitFor(`document.querySelector(${j(`${row(1)}[data-tablet="true"]`)}) !== null`, 'the tablet on « Correction »');
          const list = await rows();
          if (list.find((r) => r.index === 0)?.tablet) throw new Error('two rows show the tablet');
          if (!list.find((r) => r.index === 0)?.chosen) throw new Error('sending the tablet changed the chosen layer');
          const next = await until(
            () => model.eventsSince(mark).find((e) => e.event.type === 'strokeStart' && penOf(e.event) === 'tablet' && e.event.stroke.layer === 1),
            'a stroke of the tablet on layer 1 in the stream',
            20_000,
          );
          const later = model.eventsSince(next.i).filter((e) => e.event.type === 'strokeStart' && penOf(e.event) === 'tablet');
          if (later.some((e) => e.event.stroke.layer !== 1)) throw new Error('the tablet went back to another layer');
        });

        await check('masquer le calque de la tablette : l’avertissement ; pendant 2 s le partage ne reçoit aucun trait de la tablette, l’aperçu en compte de nouveaux ; le réafficher', async () => {
          const hideMark = stream.mark();
          await app.click(inRow(1, 'layer-visible'));
          await app.waitFor(`document.querySelector(${j(`${row(1)}[data-hidden="true"]`)}) !== null`, 'the layer hidden');
          await app.waitFor(`${zoneWarnings}.includes('tabletHidden')`, 'the warning that the tablet writes on a hidden layer');
          if ((await warnings()).includes('chosenHidden')) throw new Error('the chosen layer (shown) is said to be hidden');
          await until(() => model.eventsSince(hideMark).some((e) => e.event.type === 'snapshot'), 'the snapshot of the hidden layer in the stream', 10_000);
          const quiet = stream.mark();
          const before = await app.evaluate(countOf(LIVE_CANVAS));
          await sleep(HIDDEN_QUIET_MS);
          const leaked = model.eventsSince(quiet).filter((e) => ['strokeStart', 'points', 'strokeEnd', 'strokeCancel'].includes(e.event.type) && penOf(e.event) === 'tablet');
          if (leaked.length > 0) throw new Error(`the share got ${leaked.length} events of the tablet on its hidden layer (${leaked.map((e) => e.event.type).join(', ')})`);
          await app.waitFor(`${countOf(LIVE_CANVAS)} > ${before}`, 'the preview to count the strokes the tablet writes on its hidden layer', 8000);
          console.log(`    calque masqué : aperçu ${before} -> ${await app.evaluate(countOf(LIVE_CANVAS))} traits, aucun envoyé au partage en ${HIDDEN_QUIET_MS} ms`);
          const showMark = stream.mark();
          await app.click(inRow(1, 'layer-visible'));
          await app.waitFor(`document.querySelector(${j(`${row(1)}[data-hidden="true"]`)}) === null`, 'the layer shown');
          await app.waitFor(`!${zoneWarnings}.includes('tabletHidden')`, 'the warning to go');
          await until(
            () => model.eventsSince(showMark).some((e) => e.event.type === 'strokeStart' && penOf(e.event) === 'tablet'),
            'a stroke of the tablet in the stream again',
            20_000,
          );
          await places.agree('after showing the tablet’s layer again');
        });

        // --- The note ---------------------------------------------------------------------------------------------------

        await check('Terminer : la note a les calques [Calque 1, Correction], les traits de la tablette sur leurs calques, celui de l’ordinateur sur « Correction » à son heure', async () => {
          presentation.close();
          presentation = null;
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('close_presentation').then(() => true)`);
          model.sync();
          await stopLive(app);
          await sleep(300);
          model.sync();
          const added = notesOf(app.library).filter((n) => !notesBefore.includes(n.path));
          if (added.length !== 1) throw new Error(`${added.length} new notes after the live session: ${added.map((n) => n.path).join(', ')}`);
          const [sheet] = added;
          const layers = sheet.note.layers ?? [];
          if (j(layers.map((x) => x.name)) !== j([firstLayer, 'Correction'])) throw new Error(`the note's layers are ${j(layers)}`);
          if (!layers.every((x) => x.visible)) throw new Error(`a layer of the note is hidden: ${j(layers)}`);
          const file = decodeStrokes(sheet.members.get('strokes.bin'));
          const shown = model.strokes.map(printOf);
          console.log(`    flux : ${shown.length} traits ; fichier : ${file.length} traits`);
          if (file.length < shown.length || file.length > shown.length + 1) throw new Error(`the file holds ${file.length} strokes, the stream ${shown.length}`);
          // The tablet's strokes, on the layers they were written on.
          let compared = 0;
          for (const print of shown) {
            const info = model.info.get(print);
            if (info?.pen !== 'tablet' || info.layer === null) continue;
            const stroke = inFile(file, print);
            if (!stroke) throw new Error(`the tablet's stroke ${print} is not in the file`);
            if (stroke.layer !== info.layer) throw new Error(`the tablet's stroke ${print} is on layer ${stroke.layer} in the file, ${info.layer} on screen`);
            compared++;
          }
          // The computer's stroke: its layer, its ink, its time.
          const mine = inFile(file, computer.print);
          if (!mine) throw new Error(`the computer's stroke ${computer.print} is not in the file`);
          if (mine.layer !== 1) throw new Error(`the computer's stroke is on layer ${mine.layer}`);
          if (mine.tool !== FELT_TIP || !sameColor(mine.color, RED) || Math.abs(mine.width - COMPUTER_WIDTH) > 1e-3) {
            throw new Error(`the computer's stroke is tool ${mine.tool}, colour ${colorText(mine.color)}, width ${mine.width}; expected felt tip, red, 120`);
          }
          const created = Number(sheet.note.created);
          const offset = created + mine.points[0].t - computer.downAt;
          console.log(`    trait de l’ordinateur : premier t ${mine.points[0].t} ms, ${offset.toFixed(0)} ms de l’appui du stylet sur l’horloge de la page ; ${compared} traits de la tablette sur leur calque`);
          if (Math.abs(offset) > TIME_WITHIN_MS) throw new Error(`the computer's stroke starts ${offset.toFixed(0)} ms from the pen down, more than ${TIME_WITHIN_MS} ms`);
          // Between the tablet's strokes started just before and just after it.
          const starts = model.log
            .map((e, place) => ({ ...e, place }))
            .filter((e) => e.event.type === 'strokeStart' && penOf(e.event) === 'tablet' && !e.reset);
          const before = starts.filter((e) => e.place < computer.startedIn).at(-1);
          const after = starts.find((e) => e.place > computer.startedIn);
          if (!before || !after) throw new Error('the tablet did not write just before and just after the computer’s stroke');
          const [b, a] = [inFile(file, printOf(before.event.stroke)), inFile(file, printOf(after.event.stroke))];
          if (!b || !a) throw new Error('the tablet’s strokes around the computer’s one are not in the file');
          if (!(b.points[0].t < mine.points[0].t && mine.points[0].t < a.points[0].t)) {
            throw new Error(`the computer's stroke (t ${mine.points[0].t}) is not between the tablet's strokes around it (t ${b.points[0].t} and ${a.points[0].t})`);
          }
          if (model.log.some((e) => e.event.type === 'history')) throw new Error('a `history` event reached the share');
          if (model.log.some((e) => e.event.pen === 'computer' && 'id' in e.event)) throw new Error('the share sends the window’s number of a computer stroke (`id`)');
        });
      } finally {
        presentation?.close();
        stream?.close();
      }
    },
  },
];
