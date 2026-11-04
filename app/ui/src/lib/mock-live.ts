/**
 * Browser stand-in for live mode and the classroom (`live_*`, `list_monitors`,
 * `open_presentation`, `close_presentation`; docs/design/tablet-live-classroom.md): a simulated
 * pen that writes lines of loops on an A4 Bamboo Slate page.
 *
 * Shaped like the Rust side: 200 records per second, one batch every 15 ms with 3 points
 * (`recv` a little before `sentAt`, as if Rust took about a millisecond), strokes started,
 * continued and ended, the hovering pen at most once per batch, `hoverEnd` when it leaves. The
 * session keeps pages (each saved as a note of the demo library when its first stroke ends,
 * when the page changes and at the end), layers, style, view (blackboard: slate paper and white
 * chalk), and a presentation window on one of two fictitious screens.
 *
 * `live_resume` (M12) takes a note of the demo library as the first page: never turned
 * (`canTurnPage: false`, `orientation` = how to hold it), written into the same note only once it
 * changed, forked into a new note of the same binder (`sheetForked`) when the file changed or went
 * away meanwhile, refused with `mockLiveConfig.resumeError`.
 *
 * Deterministic: the pen follows a seeded script (no `Math.random`), so that a test under
 * `vi.useFakeTimers` sees the same strokes every time.
 */
import type { InkPoint } from './ink';
import { wallClock } from './latency';
import type {
  LiveBatch,
  LiveCommand,
  LiveEvent,
  LiveMark,
  LivePoint,
  LiveSessionState,
  LiveSnapshot,
  LiveStyle,
  LiveViewState,
  MonitorInfo,
} from './live';
import { fileName, freshStamp, mockLibrary, notePath, notifyLibraryChanged, sameStamp, UNHANDLED } from './mock-data';
import type { FileStamp, Note, Rgba, Stroke } from './note';
import { isPortrait, orientPoint, pageSizeFor, turnPoint, turnsFrom, turnStrokes, type Orientation } from './orientation';

/** Pace of the stand-in; tests change it (and `resetMockLive` restores it). */
export const mockLiveConfig = {
  /** From `live_start` to the first pen data (the user presses the tablet's button). */
  waitMs: 1200,
  /** One batch per tick. */
  tickMs: 15,
  /** The simulated pen writes once live (false: tests send their own events). */
  pen: true,
  /** Fictitious screens offered (2: laptop + projector; 1; 0 for none). */
  monitors: 2,
  /** A message makes every write of a page into the library fail with it (null: writes succeed). */
  saveError: null as string | null,
  /** The code `live_resume` refuses with (`noteDamaged`, `noteNewerVersion`...); null: it resumes. */
  resumeError: null as string | null,
};

/** Records per second of the Slate: one every 5 ms. */
const RECORD_MS = 5;
const PAGE = { width: 29700, height: 21600 };
const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const CHALK_WHITE: Rgba = { r: 255, g: 255, b: 255, a: 255 };
const DEFAULT_STYLE: LiveStyle = { tool: 'ballpoint', color: BLACK, width: 60 };
const CHALK_STYLE: LiveStyle = { tool: 'chalk', color: CHALK_WHITE, width: 150 };

const MONITORS: Omit<MonitorInfo, 'chosen'>[] = [
  { key: '\\\\.\\DISPLAY1@0,0', name: '\\\\.\\DISPLAY1', x: 0, y: 0, width: 1920, height: 1080, scale: 1.25, primary: true },
  { key: '\\\\.\\DISPLAY2@1920,0', name: '\\\\.\\DISPLAY2', x: 1920, y: 0, width: 1280, height: 800, scale: 1, primary: false },
];

// --- The pen ------------------------------------------------------------------------------------

type PenSample = { kind: 'down'; x: number; y: number; p: number } | { kind: 'hover'; x: number; y: number } | { kind: 'away' };

/** A seeded pen: words of loops along lines, hovering between them, sometimes lifted away. */
class PenScript {
  private seed = 0x2545f491;
  private queue: PenSample[] = [];
  private x = 2000;
  private y = 3000;
  private strokes = 0;
  /** Where the last stroke ended. */
  private last: { x: number; y: number } | null = null;

  private random(): number {
    // xorshift32: small, deterministic.
    let s = this.seed;
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    this.seed = s >>> 0;
    return this.seed / 0x1_0000_0000;
  }

  private between(min: number, max: number): number {
    return min + (max - min) * this.random();
  }

  next(): PenSample {
    if (this.queue.length === 0) this.plan();
    return this.queue.shift() ?? { kind: 'away' };
  }

  /** The next stroke and how the pen gets there. */
  private plan(): void {
    const length = Math.round(this.between(1500, 4000));
    if (this.x + length > PAGE.width - 2000) {
      this.x = 2000;
      this.y += 1800;
      if (this.y > PAGE.height - 1500) this.y = 3000;
    }
    this.strokes += 1;
    if (this.strokes % 6 === 0) {
      // Lifted away for 150 to 300 ms, then back near the page.
      const away = Math.round(this.between(30, 60));
      for (let i = 0; i < away; i++) this.queue.push({ kind: 'away' });
    }
    const start = { x: this.x, y: this.y };
    const approach = Math.round(this.between(12, 30));
    const origin = this.last ?? { x: start.x - 600, y: start.y - 400 };
    for (let i = 1; i <= approach; i++) {
      const u = i / approach;
      this.queue.push({ kind: 'hover', x: origin.x + (start.x - origin.x) * u, y: origin.y + (start.y - origin.y) * u });
    }
    const records = Math.round(this.between(60, 140));
    const loops = Math.round(this.between(2, 5));
    const radius = this.between(250, 450);
    for (let i = 0; i < records; i++) {
      const u = i / (records - 1);
      const angle = 2 * Math.PI * loops * u;
      this.queue.push({
        kind: 'down',
        x: start.x + length * u - radius * Math.sin(angle),
        y: start.y - radius * 0.9 * (1 - Math.cos(angle)),
        p: 0.25 + 0.6 * Math.pow(Math.sin(Math.PI * u), 0.6),
      });
    }
    const end = this.queue[this.queue.length - 1] as { x: number; y: number };
    this.last = { x: end.x, y: end.y };
    this.x += length + 500;
  }
}

// --- The session ----------------------------------------------------------------------------------

interface Session {
  state: LiveSessionState;
  pages: Note[];
  pageIndex: number;
  layer: number;
  style: LiveStyle;
  /** The style before the blackboard was switched on. */
  styleBeforeBoard: LiveStyle | null;
  view: LiveViewState;
  hover: { x: number; y: number } | null;
  current: Stroke | null;
  /** Note id → path in the demo library. */
  saved: Map<string, string>;
  /** Ids of the pages whose last write failed (reported once, written again by the next success). */
  failed: Set<string>;
  /** Records since the session started. */
  record: number;
  /** Record at which each page was created (its `created` instant). */
  pageStart: Map<string, number>;
  pen: PenScript;
  /** The ink of the page last cleared, for « Undo the clearing ». */
  cleared: { pageIndex: number; strokes: Stroke[] } | null;
  /** The sheets resumed (M12), by note id. */
  resumed: Map<string, ResumedSheet>;
  /** The `sheetForked` events so far: a window that subscribes later gets them too. */
  forks: LiveEvent[];
  wait?: ReturnType<typeof setTimeout>;
  tick?: ReturnType<typeof setInterval>;
  reconnect?: ReturnType<typeof setTimeout>;
}

/** A sheet of the library that live mode took up again (`live_resume`). */
interface ResumedSheet {
  /** Where the note is, and the binder it stays in. */
  path: string;
  binder: string;
  /** The file's stamp when last read or written; null once the page went on in a new note. */
  stamp: FileStamp | null;
  /** The ink and layers as read: a page that still looks like this is not written. */
  seed: string;
  /** Time of the first new point, ms since the note's `created` (after the last stroke). */
  timeBase: number;
}

let session: Session | null = null;
let pageCounter = 0;
/** How the (simulated) tablet is held; the real one is `[tablet] orientation`. */
let heldAs: Orientation = 'landscape';
/** The orientation each page was written in (by note id): the page on screen follows `heldAs`. */
const pageOrientation = new Map<string, Orientation>();
const subscribers = new Set<(batch: LiveBatch) => void>();
let presentation: MonitorInfo | null = null;
let savedMonitor: string | null = null;

/** What crosses the IPC boundary is a copy. */
function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function blankPage(board: boolean): Note {
  pageCounter += 1;
  const created = Date.now();
  const id = `00000000-0000-4000-8000-${pageCounter.toString(16).padStart(12, '0')}`;
  pageOrientation.set(id, heldAs);
  return {
    id,
    title: '',
    created,
    modified: created,
    page: pageSizeFor(heldAs, PAGE.width, PAGE.height),
    background: { paper: board ? 'slate' : 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: [],
    timing: 'recorded',
    origin: { kind: 'live' },
  };
}

const IDLE_VIEW: LiveViewState = { zoom: 1, x: PAGE.width / 2, y: PAGE.height / 2, board: false, laser: false };

function snapshot(): LiveSnapshot {
  const s = session;
  if (!s) {
    return copy({
      state: 'idle',
      page: { ...blankPage(false), id: '' },
      pageIndex: 0,
      pageCount: 1,
      orientation: heldAs,
      canTurnPage: true,
      layer: 0,
      style: DEFAULT_STYLE,
      view: IDLE_VIEW,
      hover: null,
      presentation,
      saved: [],
    } satisfies LiveSnapshot);
  }
  return copy({
    state: s.state,
    page: s.pages[s.pageIndex],
    pageIndex: s.pageIndex,
    pageCount: s.pages.length,
    orientation: holdFor(s.pages[s.pageIndex], s),
    canTurnPage: !s.resumed.has(s.pages[s.pageIndex].id),
    layer: s.layer,
    style: s.style,
    view: s.view,
    hover: s.hover,
    presentation,
    saved: [...s.saved.values()],
  } satisfies LiveSnapshot);
}

/**
 * How the current page is to be held: the setting, except for a resumed sheet that is not the same
 * way round (a quarter turn on the same side: the button stays away from the writing hand).
 */
function holdFor(page: Note, s: Session): Orientation {
  if (!s.resumed.has(page.id)) return heldAs;
  const { width, height } = page.page;
  if (width === height || isPortrait(heldAs) === height > width) return heldAs;
  const other: Record<Orientation, Orientation> = {
    landscape: 'portraitLeft',
    landscapeFlipped: 'portraitRight',
    portraitLeft: 'landscape',
    portraitRight: 'landscapeFlipped',
  };
  return other[heldAs];
}

function broadcast(events: LiveEvent[]): void {
  if (events.length === 0) return;
  // Like Rust: a snapshot does not hold the stroke in progress, so it is followed by it.
  const last = events.map((e) => e.type).lastIndexOf('snapshot');
  const drawing = session?.current;
  if (last >= 0 && drawing && !events.slice(last).some((e) => e.type === 'strokeStart')) {
    const { points, ...style } = drawing;
    events = [
      ...events,
      {
        type: 'strokeStart',
        pageIndex: session!.pageIndex,
        stroke: { ...style, points: points.map((p) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, recv: wallClock() })) },
      },
    ];
  }
  const batch: LiveBatch = { events, sentAt: wallClock() };
  for (const subscriber of [...subscribers]) subscriber(copy(batch));
}

function broadcastSnapshot(): void {
  broadcast([{ type: 'snapshot', snapshot: snapshot() }]);
}

function running(): Session {
  if (!session) throw { code: 'liveNotRunning', message: 'live mode is not running' };
  return session;
}

/** A free file name in `binder` (the library root by default), like `create_unique`. */
function uniquePath(note: Note, binder = ''): string {
  const library = mockLibrary();
  const name = fileName(note).replace(/\.ardoise$/, '');
  let path = notePath(library.root, binder, `${name}.ardoise`);
  for (let n = 2; library.notes.has(path); n++) path = notePath(library.root, binder, `${name} (${n}).ardoise`);
  return path;
}

/** What tells that a resumed page was not touched. */
function looksLike(page: Note): string {
  return JSON.stringify({ strokes: page.strokes, layers: page.layers });
}

/**
 * Writes `page` into the demo library. A resumed sheet that changed elsewhere since it was read
 * (or is gone) is not overwritten: the page goes on in a new note of the same binder, and
 * `sheetForked` is sent once. Returns the events to send.
 */
function writePage(s: Session, page: Note): LiveEvent[] {
  const library = mockLibrary();
  const events: LiveEvent[] = [];
  const sheet = s.resumed.get(page.id);
  let path = s.saved.get(page.id) ?? sheet?.path ?? uniquePath(page);
  if (sheet?.stamp) {
    const current = library.notes.get(sheet.path);
    const reason = !current ? 'deleted' : sameStamp(current.stamp, sheet.stamp) ? null : 'changed';
    if (reason) {
      path = uniquePath(page, sheet.binder);
      const fork: LiveEvent = { type: 'sheetForked', pageIndex: s.pages.indexOf(page), reason, original: sheet.path, path };
      s.forks.push(fork);
      events.push(fork);
      sheet.stamp = null;
    }
  }
  const stamp = freshStamp();
  const kept = library.notes.get(path);
  library.notes.set(path, {
    note: copy({ ...page, modified: Date.now() }),
    stamp,
    binder: sheet?.binder ?? kept?.binder ?? '',
    original: kept?.original,
  });
  if (sheet?.stamp) sheet.stamp = stamp;
  s.saved.set(page.id, path);
  notifyLibraryChanged();
  return events;
}

/**
 * Writes `page` into the demo library (once it has strokes). Returns the events to send:
 * `saved` after a write (the pages whose write failed earlier are written again first, so it
 * means that everything is written), `saveFailed` the first time a page fails (not again while
 * the error repeats), nothing otherwise.
 */
function savePage(s: Session, page: Note): LiveEvent[] {
  const sheet = s.resumed.get(page.id);
  // A resumed sheet that looks as it was read is left as it is, as is a blank page.
  if (!s.saved.has(page.id) && (sheet ? looksLike(page) === sheet.seed : page.strokes.length === 0)) return [];
  const failure = mockLiveConfig.saveError;
  if (failure !== null) {
    if (s.failed.has(page.id)) return [];
    s.failed.add(page.id);
    return [{ type: 'saveFailed', pageIndex: s.pages.indexOf(page), code: 'libraryAccess', message: failure }];
  }
  const events: LiveEvent[] = [];
  for (const other of s.pages) if (s.failed.has(other.id)) events.push(...writePage(s, other));
  s.failed.clear();
  events.push(...writePage(s, page), savedEvent(s));
  return events;
}

function savedEvent(s: Session): LiveEvent {
  return { type: 'saved', paths: [...s.saved.values()] };
}

/** Ends the stroke in progress (adds it to its page); returns the events to send. */
function endStroke(s: Session): LiveEvent[] {
  if (!s.current) return [];
  const page = s.pages[s.pageIndex];
  page.strokes.push(s.current);
  s.current = null;
  const events: LiveEvent[] = [{ type: 'strokeEnd', pageIndex: s.pageIndex }];
  // A page becomes a note when its first stroke ends.
  if (!s.saved.has(page.id)) events.push(...savePage(s, page));
  return events;
}

/** Three records of the pen: one batch. */
function tick(s: Session): void {
  if (s.state !== 'live') return;
  const events: LiveEvent[] = [];
  const now = wallClock();
  let hoverAt: { x: number; y: number } | null = null;
  for (let j = 0; j < 3; j++) {
    s.record += 1;
    const sample = orientSample(s.pen.next());
    const page = s.pages[s.pageIndex];
    if (sample.kind === 'down') {
      hoverAt = null;
      const point: LivePoint = {
        x: Math.round(sample.x),
        y: Math.round(sample.y),
        p: Math.round(sample.p * 2047) / 2047,
        t: (s.resumed.get(page.id)?.timeBase ?? 0) + (s.record - (s.pageStart.get(page.id) ?? 0)) * RECORD_MS,
        // Received with the notification, a little before the batch left Rust.
        recv: now - 0.4 - 0.3 * ((s.record * 7) % 5),
      };
      const ink: InkPoint = { x: point.x, y: point.y, pressure: point.p, t: point.t };
      const last = events[events.length - 1];
      if (!s.current) {
        const { tool, color, width } = s.style;
        s.current = { tool, color: { ...color }, width, layer: s.layer, points: [ink] };
        events.push({ type: 'strokeStart', pageIndex: s.pageIndex, stroke: { tool, color: { ...color }, width, layer: s.layer, points: [point] } });
      } else {
        s.current.points.push(ink);
        if (last?.type === 'strokeStart') last.stroke.points.push(point);
        else if (last?.type === 'points') last.points.push(point);
        else events.push({ type: 'points', pageIndex: s.pageIndex, points: [point] });
      }
    } else {
      events.push(...endStroke(s));
      if (sample.kind === 'hover') {
        hoverAt = { x: Math.round(sample.x), y: Math.round(sample.y) };
        s.hover = hoverAt;
      } else if (s.hover) {
        s.hover = null;
        hoverAt = null;
        events.push({ type: 'hoverEnd' });
      }
    }
  }
  if (hoverAt) events.push({ type: 'hover', ...hoverAt });
  broadcast(events);
}

/** The pen's samples are in the tablet's frame; the page gets them turned, as the service does. */
function orientSample(sample: PenSample): PenSample {
  if (sample.kind === 'away') return sample;
  const [x, y] = orientPoint(sample.x, sample.y, PAGE.width, PAGE.height, heldAs);
  return { ...sample, x, y };
}

function startPen(s: Session): void {
  clearInterval(s.tick);
  if (mockLiveConfig.pen) s.tick = setInterval(() => tick(s), mockLiveConfig.tickMs);
}

function stopTimers(s: Session): void {
  clearTimeout(s.wait);
  clearInterval(s.tick);
  clearTimeout(s.reconnect);
}

function addPage(s: Session): void {
  const page = blankPage(s.view.board);
  s.pages.push(page);
  s.pageStart.set(page.id, s.record);
}

function goTo(s: Session, index: number): LiveEvent[] {
  const events = endStroke(s);
  const leaving = s.pages[s.pageIndex];
  if (s.saved.has(leaving.id)) events.push(...savePage(s, leaving));
  s.pageIndex = index;
  followTablet(s.pages[index]);
  s.layer = s.pages[index].layers.length - 1;
  return events;
}

function runCommand(s: Session, command: LiveCommand): void {
  let events: LiveEvent[] = [];
  switch (command.kind) {
    case 'nextPage':
      if (s.pageIndex === s.pages.length - 1) addPage(s);
      events = goTo(s, s.pageIndex + 1);
      break;
    case 'previousPage':
      if (s.pageIndex === 0) return;
      events = goTo(s, s.pageIndex - 1);
      break;
    case 'button':
    case 'newPage':
      addPage(s);
      events = goTo(s, s.pages.length - 1);
      break;
    case 'newLayer': {
      events = endStroke(s);
      const page = s.pages[s.pageIndex];
      page.layers.push({ name: `Calque ${page.layers.length + 1}`, visible: true });
      s.layer = page.layers.length - 1;
      break;
    }
    case 'clearPage': {
      s.current = null;
      const page = s.pages[s.pageIndex];
      if (page.strokes.length > 0) s.cleared = { pageIndex: s.pageIndex, strokes: page.strokes };
      page.strokes = [];
      break;
    }
    default:
      throw { code: 'invalidCommand', message: String((command as { kind: unknown }).kind) };
  }
  broadcast([...events, { type: 'snapshot', snapshot: snapshot() }]);
}

function setView(s: Session, requested: LiveViewState): void {
  const zoom = Math.min(8, Math.max(1, Number.isFinite(requested.zoom) ? requested.zoom : 1));
  const view = { ...requested, zoom };
  const boardChanged = view.board !== s.view.board;
  s.view = view;
  if (!boardChanged) {
    broadcast([{ type: 'view', view: copy(view) }]);
    return;
  }
  // Blackboard: slate paper and white chalk; switched off, the paper and style come back.
  const page = s.pages[s.pageIndex];
  page.background = { ...page.background, paper: view.board ? 'slate' : 'white' };
  if (view.board) {
    s.styleBeforeBoard = s.style;
    s.style = CHALK_STYLE;
  } else {
    s.style = s.styleBeforeBoard ?? DEFAULT_STYLE;
    s.styleBeforeBoard = null;
  }
  broadcastSnapshot();
}

/** A session waiting for the tablet, with its first page: blank, or the sheet resumed. */
function openSession(first: { page: Note; sheet: ResumedSheet } | null): LiveSnapshot {
  const s: Session = {
    state: 'waiting',
    pages: [],
    pageIndex: 0,
    layer: 0,
    style: DEFAULT_STYLE,
    styleBeforeBoard: null,
    view: { ...IDLE_VIEW },
    hover: null,
    current: null,
    saved: new Map(),
    failed: new Set(),
    record: 0,
    pageStart: new Map(),
    pen: new PenScript(),
    cleared: null,
    resumed: new Map(),
    forks: [],
  };
  if (first) {
    const { page, sheet } = first;
    s.pages.push(page);
    s.pageStart.set(page.id, s.record);
    s.resumed.set(page.id, sheet);
    // The blackboard is on only for a slate sheet; new ink goes on the last visible layer.
    s.view = { ...s.view, x: page.page.width / 2, y: page.page.height / 2, board: page.background.paper === 'slate' };
    if (s.view.board) s.style = CHALK_STYLE;
    if (!page.layers.some((layer) => layer.visible)) page.layers.push({ name: `Calque ${page.layers.length + 1}`, visible: true });
    s.layer = page.layers.map((layer) => layer.visible).lastIndexOf(true);
  } else {
    addPage(s);
  }
  session = s;
  s.wait = setTimeout(() => {
    s.state = 'live';
    broadcastSnapshot();
    startPen(s);
  }, mockLiveConfig.waitMs);
  broadcastSnapshot();
  return snapshot();
}

function startSession(): LiveSnapshot {
  if (session) return snapshot();
  return openSession(null);
}

/** `live_resume`: refused first (the running session, if any, goes on), then like a stop and a start. */
function resumeSession(path: string): LiveSnapshot {
  const library = mockLibrary();
  const refusal = mockLiveConfig.resumeError;
  if (refusal !== null) throw { code: refusal, message: path };
  const stored = library.notes.get(path);
  if (!stored) throw { code: path.startsWith(library.root) ? 'noteNotFound' : 'notInLibrary', message: path };
  stopSession();
  const page = copy(stored.note);
  pageOrientation.set(page.id, heldAs);
  const last = page.strokes.reduce((end, stroke) => Math.max(end, stroke.points.at(-1)?.t ?? 0), 0);
  const sheet: ResumedSheet = {
    path,
    binder: stored.binder,
    stamp: stored.stamp,
    seed: looksLike(page),
    timeBase: last + 1000,
  };
  return openSession({ page, sheet });
}

function stopSession(): string[] {
  const s = session;
  if (!s) return [];
  stopTimers(s);
  const events = endStroke(s);
  for (const page of s.pages) events.push(...savePage(s, page).filter((e) => e.type !== 'saved'));
  const paths = [...s.saved.values()];
  // A `saved` after a failure means that everything is written: not sent while a page failed.
  if (s.failed.size === 0) events.push({ type: 'saved', paths });
  session = null;
  broadcast([...events, { type: 'snapshot', snapshot: snapshot() }]);
  return paths;
}

function monitors(): MonitorInfo[] {
  return MONITORS.slice(0, Math.max(0, mockLiveConfig.monitors)).map((m) => ({ ...m, chosen: m.key === savedMonitor }));
}

/** The screen of `open_presentation`, chosen like `presentation.rs`. */
function chooseMonitor(requested: string | null): MonitorInfo {
  const list = monitors();
  const chosen =
    list.find((m) => m.key === requested) ??
    list.find((m) => m.key === savedMonitor) ??
    list.find((m) => !m.primary) ??
    list[0];
  if (!chosen) throw { code: 'noMonitor', message: 'no monitor' };
  return chosen;
}

export function mockLiveInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'live_start':
      return startSession();
    case 'live_resume':
      return resumeSession(String(args.path));
    case 'live_stop':
      return stopSession();
    case 'live_set_style': {
      const s = running();
      s.style = copy(args.style as LiveStyle);
      broadcastSnapshot();
      return undefined;
    }
    case 'live_command':
      runCommand(running(), args.command as LiveCommand);
      return undefined;
    case 'live_restore_cleared': {
      const s = running();
      const cleared = s.cleared;
      s.cleared = null;
      if (!cleared || cleared.pageIndex !== s.pageIndex) return false;
      const page = s.pages[s.pageIndex];
      page.strokes = [...cleared.strokes, ...page.strokes];
      broadcastSnapshot();
      return true;
    }
    case 'live_set_view':
      setView(running(), args.view as LiveViewState);
      return undefined;
    case 'live_set_marks':
      running();
      mockLiveMarks.push({ pageIndex: args.pageIndex as number, marks: args.marks as LiveMark[] });
      return undefined;
    case 'list_monitors':
      return monitors();
    case 'open_presentation': {
      const monitor = chooseMonitor((args.monitor as string | null) ?? null);
      // The assistant's bridge (`remember: false`) changes no setting.
      if (args.remember !== false) savedMonitor = monitor.key;
      presentation = { ...monitor, chosen: true };
      broadcastSnapshot();
      return copy(presentation);
    }
    case 'close_presentation':
      if (presentation) {
        presentation = null;
        broadcastSnapshot();
      }
      return undefined;
    default:
      return UNHANDLED;
  }
}

/** `live_subscribe` in the browser: the current session, then every batch. */
export async function mockLiveSubscribe(onBatch: (batch: LiveBatch) => void): Promise<LiveSnapshot> {
  subscribers.add(onBatch);
  // Like Rust: a sheet forked before this window subscribed is told to it too, once.
  const forks = session?.forks ?? [];
  if (forks.length > 0) queueMicrotask(() => onBatch(copy({ events: forks, sentAt: wallClock() })));
  return snapshot();
}

// --- For tests ----------------------------------------------------------------------------------

/** The lists of marks the presentation window sent (`live_set_marks`), in order. */
export const mockLiveMarks: { pageIndex: number; marks: LiveMark[] }[] = [];

/** Stops the session, forgets subscribers, screens, settings and marks. */
export function resetMockLive(): void {
  mockLiveMarks.length = 0;
  if (session) stopTimers(session);
  session = null;
  subscribers.clear();
  presentation = null;
  savedMonitor = null;
  pageCounter = 0;
  heldAs = 'landscape';
  pageOrientation.clear();
  Object.assign(mockLiveConfig, { waitMs: 1200, tickMs: 15, pen: true, monitors: 2, saveError: null, resumeError: null });
}

/** Sends `events` to every subscriber as one batch. */
export function mockLiveEmit(events: LiveEvent[]): void {
  broadcast(events);
}

/** The tablet's link drops for `ms`, then comes back. */
export function mockLiveReconnect(ms: number): void {
  const s = running();
  clearInterval(s.tick);
  s.state = 'reconnecting';
  broadcastSnapshot();
  s.reconnect = setTimeout(() => {
    s.state = 'live';
    broadcastSnapshot();
    startPen(s);
  }, ms);
}

/** Pen ticks without timers (tests that drive the pen step by step). */
export function mockLiveTick(count = 1): void {
  const s = running();
  for (let i = 0; i < count; i++) tick(s);
}

/** Turns `page` with its ink to the way the tablet is held. */
function followTablet(page: Note): void {
  // A resumed sheet is never turned: the session follows it instead (`holdFor`).
  if (session?.resumed.has(page.id)) return;
  const from = pageOrientation.get(page.id) ?? 'landscape';
  const turns = turnsFrom(heldAs, from);
  pageOrientation.set(page.id, heldAs);
  if (turns === 0) return;
  const { width, height } = page.page;
  page.strokes = turnStrokes(page.strokes, width, height, turns);
  if (turns % 2 === 1) page.page = { width: height, height: width };
}

/**
 * The tablet is held another way (`tablet_set_options`, `tablet_set_orientation`). In a running
 * session, as in Rust: the stroke in progress ends, the page on screen is turned with its ink,
 * blank pages not written yet take the new size, the other pages follow when they are shown
 * again, and every window gets a snapshot. As in Rust, the switch also counts while the session
 * still waits for the tablet (or reconnects): the snapshot follows at once.
 */
export function setMockLiveOrientation(orientation: Orientation): void {
  if (orientation === heldAs) return;
  const s = session;
  const before = s ? { ...s.pages[s.pageIndex].page } : null;
  const turns = s && !s.resumed.has(s.pages[s.pageIndex].id) ? turnsFrom(orientation, pageOrientation.get(s.pages[s.pageIndex].id) ?? 'landscape') : 0;
  const events = s ? endStroke(s) : [];
  heldAs = orientation;
  if (!s || !before) return;
  s.hover = null;
  s.pages.forEach((page, index) => {
    if (page.strokes.length === 0 && !s.saved.has(page.id) && !s.resumed.has(page.id)) {
      page.page = pageSizeFor(orientation, PAGE.width, PAGE.height);
      pageOrientation.set(page.id, orientation);
    } else if (index === s.pageIndex) {
      followTablet(page);
      if (s.saved.has(page.id)) events.push(...savePage(s, page));
    }
  });
  const page = s.pages[s.pageIndex].page;
  if (s.view.zoom <= 1) {
    s.view = { ...s.view, x: page.width / 2, y: page.height / 2 };
  } else {
    const [x, y] = turnPoint(s.view.x, s.view.y, before.width, before.height, turns);
    s.view = { ...s.view, x, y };
  }
  broadcast([...events, { type: 'snapshot', snapshot: snapshot() }]);
}

export function mockLiveSubscribers(): number {
  return subscribers.size;
}
