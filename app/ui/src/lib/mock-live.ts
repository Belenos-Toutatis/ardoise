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
 * Touching up (M12): `live_edit_strokes`, `live_undo_edit`, `live_redo_edit`, with the rules of
 * `live/edits.rs`: the same checks and codes, a revision per page, a history per page kept when it
 * is left and turned with it, `strokesEdited` events, « Clear the page » in the history.
 *
 * Correcting during live (M14), with the rules of `live/pen.rs`, `live/layers.rs` and `session.rs`
 * (docs/design/tablet-live-classroom.md, "M14" and its "As built (B)"): each page has the tablet's layer and
 * the computer's (`layer`, `computerLayer` in the snapshot); `live_computer_pen` writes the computer's
 * stroke beside the tablet's (`pen: 'computer'` and `id` on its events, `strokeCancel`, a `history` after
 * every computer `strokeEnd`), its end is an undo step; an eraser's deletes of one `group` are undone and
 * redone together; `live_layers` adds, renames, shows or hides, moves, merges, and chooses the layers of
 * the computer and of the tablet, one snapshot when something changed.
 *
 * Paper and image of the current page (M17, docs/design/paper-projection-narration.md §1): `live_page_paper` and
 * `live_page_image` (`set` of a staged image, `place`, `remove`) with the same refusals (`liveEditStale`,
 * `liveEditInvalid`, `backgroundMissing`); the snapshot has the page's own paper (`pagePaper`: what it has without the
 * blackboard, which the board's slate hides) and the key and size of the image's pixels (`imagePixels`). A new page
 * keeps the paper, the template and the texture of the page it is made from, not its image; a new session starts white;
 * a page with an image is written even without ink; one with an image and ink is held like a resumed sheet (the
 * tablet follows it), one with an image and no ink takes the tablet's new size with its image placed again.
 *
 * Deterministic: the pen follows a seeded script (no `Math.random`), so that a test under
 * `vi.useFakeTimers` sees the same strokes every time.
 */
import type { InkPoint } from './ink';
import { wallClock } from './latency';
import type {
  ComputerSample,
  ImagePixels,
  LiveBatch,
  LiveCommand,
  LiveComputerPenInput,
  LiveEvent,
  LiveLayerOp,
  LiveMark,
  LivePageImageOp,
  LivePoint,
  LiveSessionState,
  LiveSnapshot,
  LiveStopped,
  LiveStrokesEdit,
  LiveStrokesEdited,
  LiveStroke,
  LiveStyle,
  LiveViewState,
  MonitorInfo,
  PlacedStroke,
} from './live';
import { currentFit, refit } from './background';
import { insertStrokes, recolourStrokes, removeStrokes } from './live-edits';
import { fileName, freshStamp, mockLibrary, notePath, notifyLibraryChanged, sameStamp, UNHANDLED } from './mock-data';
import { backgroundDataUrl } from './mock-notes';
import type { Background, BackgroundImage, FileStamp, Note, PageSize, Paper, Rgba, Stroke } from './note';
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
  /** The tablet's layer and the computer's, per page (by note id): a page left keeps them. */
  layersOf: Map<string, PageLayers>;
  style: LiveStyle;
  /** The style before the blackboard was switched on. */
  styleBeforeBoard: LiveStyle | null;
  view: LiveViewState;
  hover: { x: number; y: number } | null;
  current: Stroke | null;
  /** The computer's stroke in progress (`live_computer_pen`). */
  computer: ComputerDrawing | null;
  /** Note id → path in the demo library. */
  saved: Map<string, string>;
  /** Ids of the pages whose last write failed (reported once, written again by the next success). */
  failed: Set<string>;
  /** Records since the session started. */
  record: number;
  /** Record at which each page was created (its `created` instant). */
  pageStart: Map<string, number>;
  pen: PenScript;
  /** What can be undone on each page, by note id. */
  histories: Map<string, PageHistory>;
  /** The sheets resumed (M12), by note id. */
  resumed: Map<string, ResumedSheet>;
  /** M17: each page's own paper (by note id), what it has without the blackboard; kept while the board hides it. */
  own: Map<string, Paper>;
  /** M17: the pixels of each page's image (by note id). */
  images: Map<string, ImagePixels>;
  /** The `sheetForked` events so far: a window that subscribes later gets them too. */
  forks: LiveEvent[];
  wait?: ReturnType<typeof setTimeout>;
  tick?: ReturnType<typeof setInterval>;
  reconnect?: ReturnType<typeof setTimeout>;
}

/** Where the pens write on a page (indices of its `layers`). */
interface PageLayers {
  tablet: number;
  computer: number;
}

/** The computer's stroke in progress: the window's number of the gesture, its page, its points so far. */
interface ComputerDrawing {
  id: number;
  pageIndex: number;
  stroke: Stroke;
  /** The wall time of its last sample (Unix ms). */
  lastWall: number | null;
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
  /** The note as read. */
  before: Note;
  /** Ink was added while the blackboard was on. */
  inkOnBoard: boolean;
  /** The wall time (Unix ms) the computer's times count from: the resume's, less `timeBase`. */
  origin: number;
  /** A deliberate act on the layers (`live_layers`): its layers are written as they are, new ink or not. */
  layersEdited: boolean;
  /** M17: the paper or the image was changed on purpose (`live_page_paper`, `live_page_image`): written as they are. */
  paperEdited: boolean;
  imageEdited: boolean;
}

/** Most strokes one edit may name, steps (groups) a page keeps (`live/edits.rs`). */
const MAX_EDIT_STROKES = 1 << 20;
const MAX_UNDO_STEPS = 100;
/** Samples one call of the computer's pen may bring, points one computer stroke may hold (`live/pen.rs`). */
export const MAX_PEN_SAMPLES = 4096;
export const MAX_COMPUTER_POINTS = 100_000;
/** A sample's time is at most this long before its call (ms). */
const MAX_SAMPLE_AGE_MS = 10_000;
/** Layers a page may have, characters a layer's name may have (`live/layers.rs`). */
export const MAX_LAYERS = 100;
export const MAX_LAYER_NAME = 200;

interface ColourChange {
  index: number;
  before: Rgba;
  after: Rgba;
}

/** `group`: an eraser's gesture; steps next to each other with the same group are undone and redone together. */
type UndoStep =
  // `left`: the strokes left in the list just after the removal (what « Undo the clearing » needs).
  | { kind: 'removed'; strokes: PlacedStroke[]; cleared: boolean; left: number; group?: number }
  | { kind: 'recoloured'; changes: ColourChange[]; group?: number }
  | { kind: 'added'; indices: number[]; group?: number };
type RedoStep =
  | { kind: 'remove'; indices: number[]; cleared: boolean; group?: number }
  | { kind: 'recolour'; changes: ColourChange[]; group?: number }
  | { kind: 'add'; strokes: PlacedStroke[]; group?: number };

/** The edits of a page, and what the indices of its strokes mean. */
interface PageHistory {
  revision: number;
  undo: UndoStep[];
  redo: RedoStep[];
}

let session: Session | null = null;
/** A session was stopped (`live_stop`): the computer's pen then answers `false` instead of `liveNotRunning`. */
let stoppedOnce = false;
let pageCounter = 0;
/** Revisions are never used twice: a command for another session or page cannot pass. */
let lastRevision = 0;
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

/** A new page: blank, slate under the blackboard; `from` gives the paper, template and texture of the page it follows. */
function blankPage(board: boolean, from?: { paper: Paper; template: Background['template']; texture: boolean }): Note {
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
    background: { paper: board ? 'slate' : (from?.paper ?? 'white'), template: from?.template ?? 'blank', texture: from?.texture ?? false },
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
      computerLayer: 0,
      style: DEFAULT_STYLE,
      view: IDLE_VIEW,
      hover: null,
      presentation,
      saved: [],
      revision: 0,
      canUndo: false,
      canRedo: false,
      pagePaper: 'white',
      imagePixels: null,
    } satisfies LiveSnapshot);
  }
  const history = historyOf(s, s.pages[s.pageIndex]);
  const layers = layersOf(s, s.pages[s.pageIndex]);
  return copy({
    state: s.state,
    page: s.pages[s.pageIndex],
    pageIndex: s.pageIndex,
    pageCount: s.pages.length,
    orientation: holdFor(s.pages[s.pageIndex], s),
    canTurnPage: !heldFixed(s, s.pages[s.pageIndex]),
    layer: layers.tablet,
    computerLayer: layers.computer,
    style: s.style,
    view: s.view,
    hover: s.hover,
    presentation,
    saved: [...s.saved.values()],
    revision: history.revision,
    canUndo: history.undo.length > 0,
    canRedo: history.redo.length > 0,
    pagePaper: ownPaper(s, s.pages[s.pageIndex]),
    imagePixels: s.images.get(s.pages[s.pageIndex].id) ?? null,
  } satisfies LiveSnapshot);
}

/** The page's own paper: what it has without the blackboard (`Page::own_paper`). */
function ownPaper(s: Session, page: Note): Paper {
  if (!s.view.board) return page.background.paper;
  return s.own.get(page.id) ?? s.resumed.get(page.id)?.before.background.paper ?? 'white';
}

/** A page the tablet cannot turn: a resumed sheet, or a page with an image and ink (`Page::held_fixed`). */
function heldFixed(s: Session, page: Note): boolean {
  return s.resumed.has(page.id) || (page.backgroundImage !== undefined && page.strokes.length > 0);
}

/** The layers the pens write on, on `page` (a new page: its first layer for both). */
function layersOf(s: Session, page: Note): PageLayers {
  let layers = s.layersOf.get(page.id);
  if (!layers) {
    layers = { tablet: 0, computer: 0 };
    s.layersOf.set(page.id, layers);
  }
  return layers;
}

function historyOf(s: Session, page: Note): PageHistory {
  let history = s.histories.get(page.id);
  if (!history) {
    history = { revision: (lastRevision += 1), undo: [], redo: [] };
    s.histories.set(page.id, history);
  }
  return history;
}

/** The viewer's rule: a stroke of a layer that does not exist shows. */
function isDrawn(page: Note, stroke: Stroke): boolean {
  return page.layers[stroke.layer]?.visible !== false;
}

function stale(): { code: string; message: string } {
  return { code: 'liveEditStale', message: 'the page changed since these strokes were chosen' };
}

function invalid(message: string): { code: string; message: string } {
  return { code: 'liveEditInvalid', message };
}

/** The indices of an edit in ascending order, checked as `StrokesEdit::checked` does. */
function checkedIndices(indices: readonly number[]): number[] {
  if (indices.length === 0) throw invalid('no stroke was chosen');
  if (indices.length > MAX_EDIT_STROKES) throw invalid(`too many strokes were chosen (${indices.length})`);
  if (indices.some((i) => !Number.isInteger(i) || i < 0)) throw invalid("a stroke's index is not valid");
  const sorted = [...indices].sort((a, b) => a - b);
  const twice = sorted.find((value, i) => value === sorted[i + 1]);
  if (twice !== undefined) throw invalid(`stroke ${twice} was chosen twice`);
  return sorted;
}

/** An eraser's group: a whole `u32` (`BadGroup`). */
function checkedGroup(group: unknown): number | undefined {
  if (group === undefined || group === null) return undefined;
  if (typeof group !== 'number' || !Number.isInteger(group) || group < 0 || group > 0xffff_ffff) {
    throw invalid("an eraser's group is not valid");
  }
  return group;
}

function checkedColor(color: Rgba): Rgba {
  const channel = (v: number) => Number.isInteger(v) && v >= 0 && v <= 255;
  if (![color.r, color.g, color.b, color.a].every(channel) || color.a === 0) throw invalid('this colour is not valid');
  return { r: color.r, g: color.g, b: color.b, a: color.a };
}

/** The steps at the top of `steps` that go together: the top one and every next one of its group. */
function topGroup<T extends { group?: number }>(steps: readonly T[]): number {
  const top = steps[steps.length - 1];
  if (!top) return 0;
  let count = 1;
  while (top.group !== undefined && count < steps.length && steps[steps.length - 1 - count].group === top.group) count++;
  return count;
}

/** How many groups `steps` holds (a step without a group is one). */
function groupCount(steps: readonly { group?: number }[]): number {
  let count = 0;
  steps.forEach((step, i) => {
    if (i === 0 || step.group === undefined || steps[i - 1].group !== step.group) count++;
  });
  return count;
}

/** Pushes a step; beyond `MAX_UNDO_STEPS` groups the oldest group goes, whole. */
function pushUndo(history: PageHistory, step: UndoStep): void {
  history.undo.push(step);
  while (groupCount(history.undo) > MAX_UNDO_STEPS) {
    const first = history.undo[0];
    let count = 1;
    while (first.group !== undefined && count < history.undo.length && history.undo[count].group === first.group) count++;
    history.undo.splice(0, count);
  }
}

/** A new step of the computer (a stroke, an edit): the steps undone can no longer be redone (`History::record`). */
function record(history: PageHistory, step: UndoStep): void {
  history.redo = [];
  pushUndo(history, step);
}

function takeStrokes(page: Note, indices: readonly number[]): PlacedStroke[] {
  const taken = indices.map((index) => ({ index, stroke: page.strokes[index] }));
  removeStrokes(page.strokes, indices);
  return taken;
}

/** The event of a step, with what the page's history says now. */
function edited(s: Session, page: Note, parts: Partial<Pick<LiveStrokesEdited, 'removed' | 'inserted' | 'recoloured'>>): LiveStrokesEdited {
  const history = historyOf(s, page);
  return {
    type: 'strokesEdited',
    pageIndex: s.pages.indexOf(page),
    revision: history.revision,
    removed: parts.removed ?? [],
    inserted: parts.inserted ?? [],
    recoloured: parts.recoloured ?? [],
    canUndo: history.undo.length > 0,
    canRedo: history.redo.length > 0,
  };
}

/** `live_edit_strokes`: a stale page or revision, or anything wrong in the list, changes nothing. */
function editStrokes(s: Session, pageIndex: number, revision: number, edit: LiveStrokesEdit): number {
  const page = s.pages[s.pageIndex];
  const history = historyOf(s, page);
  const indices = checkedIndices(edit.indices);
  const color = edit.kind === 'recolour' ? checkedColor(edit.color) : null;
  const group = edit.kind === 'delete' ? checkedGroup(edit.group) : undefined;
  if (pageIndex !== s.pageIndex || revision !== history.revision) throw stale();
  for (const index of indices) {
    const stroke = page.strokes[index];
    if (!stroke) throw invalid(`there is no stroke ${index} on a page of ${page.strokes.length} strokes`);
    if (!isDrawn(page, stroke)) throw invalid(`stroke ${index} is on a hidden layer`);
  }
  if (color) {
    const same = (a: Rgba, b: Rgba) => a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
    const changes = indices
      .filter((index) => !same(page.strokes[index].color, color))
      .map((index) => ({ index, before: page.strokes[index].color, after: color }));
    if (changes.length === 0) return history.revision;
    record(history, { kind: 'recoloured', changes });
    recolourStrokes(page.strokes, [{ color, indices: changes.map((c) => c.index) }]);
    broadcast([edited(s, page, { recoloured: [{ color, indices: changes.map((c) => c.index) }] })]);
  } else {
    const strokes = takeStrokes(page, indices);
    record(history, { kind: 'removed', strokes, cleared: false, left: page.strokes.length, ...(group !== undefined ? { group } : {}) });
    history.revision = lastRevision += 1;
    broadcast([edited(s, page, { removed: indices })]);
  }
  return history.revision;
}

function groupedColours(changes: readonly ColourChange[], pick: (change: ColourChange) => Rgba): LiveStrokesEdited['recoloured'] {
  const groups = new Map<string, { color: Rgba; indices: number[] }>();
  for (const change of changes) {
    const color = pick(change);
    const key = `${color.r},${color.g},${color.b},${color.a}`;
    const group = groups.get(key) ?? { color, indices: [] };
    group.indices.push(change.index);
    groups.set(key, group);
  }
  return [...groups.values()];
}

/** The group of a step, kept by the step that undoes or redoes it. */
function grouped(group: number | undefined): { group?: number } {
  return group === undefined ? {} : { group };
}

/** Undoes one step; returns its event. */
function undoOne(s: Session, page: Note, history: PageHistory, step: UndoStep): LiveStrokesEdited {
  switch (step.kind) {
    case 'removed':
      insertStrokes(page.strokes, step.strokes);
      history.redo.push({ kind: 'remove', indices: step.strokes.map((placed) => placed.index), cleared: step.cleared, ...grouped(step.group) });
      history.revision = lastRevision += 1;
      return edited(s, page, { inserted: step.strokes });
    case 'added': {
      // A computer stroke taken off: it comes back at its place on a redo.
      const strokes = takeStrokes(page, step.indices);
      history.redo.push({ kind: 'add', strokes, ...grouped(step.group) });
      history.revision = lastRevision += 1;
      return edited(s, page, { removed: step.indices });
    }
    case 'recoloured': {
      const recoloured = groupedColours(step.changes, (change) => change.before);
      recolourStrokes(page.strokes, recoloured);
      history.redo.push({ kind: 'recolour', changes: step.changes, ...grouped(step.group) });
      return edited(s, page, { recoloured });
    }
  }
}

/** Redoes one step; returns its event. */
function redoOne(s: Session, page: Note, history: PageHistory, step: RedoStep): LiveStrokesEdited {
  switch (step.kind) {
    case 'remove': {
      const strokes = takeStrokes(page, step.indices);
      pushUndo(history, { kind: 'removed', strokes, cleared: step.cleared, left: page.strokes.length, ...grouped(step.group) });
      history.revision = lastRevision += 1;
      return edited(s, page, { removed: step.indices });
    }
    case 'add':
      insertStrokes(page.strokes, step.strokes);
      pushUndo(history, { kind: 'added', indices: step.strokes.map((placed) => placed.index), ...grouped(step.group) });
      history.revision = lastRevision += 1;
      return edited(s, page, { inserted: step.strokes });
    case 'recolour': {
      const recoloured = groupedColours(step.changes, (change) => change.after);
      recolourStrokes(page.strokes, recoloured);
      pushUndo(history, { kind: 'recoloured', changes: step.changes, ...grouped(step.group) });
      return edited(s, page, { recoloured });
    }
  }
}

/**
 * Undoes the page's last step, with every step of its group (an eraser's gesture), top down: one
 * `strokesEdited` per step, in one batch. False when there is none.
 */
function undoStep(s: Session): boolean {
  const page = s.pages[s.pageIndex];
  const history = historyOf(s, page);
  const count = topGroup(history.undo);
  if (count === 0) return false;
  const events: LiveEvent[] = [];
  for (let i = 0; i < count; i++) {
    const step = history.undo.pop();
    if (step) events.push(undoOne(s, page, history, step));
  }
  broadcast(events);
  return true;
}

function redoStep(s: Session): boolean {
  const page = s.pages[s.pageIndex];
  const history = historyOf(s, page);
  const count = topGroup(history.redo);
  if (count === 0) return false;
  const events: LiveEvent[] = [];
  for (let i = 0; i < count; i++) {
    const step = history.redo.pop();
    if (step) events.push(redoOne(s, page, history, step));
  }
  broadcast(events);
  return true;
}

/**
 * « Undo the clearing », out of order as `History::unclear` does it (M14): the ink of the page's last
 * clearing comes back under what was written since, whichever pen wrote it; what was done since stays
 * done and in the history, each place it names moved for the strokes back (Ctrl+Z still undoes the
 * last of it); the clearing leaves the history and the redo steps go. One `strokesEdited`. False when
 * the page has no clearing to undo (none, already undone, or a merge dropped the history).
 */
function restoreCleared(s: Session): boolean {
  const page = s.pages[s.pageIndex];
  const history = historyOf(s, page);
  const at = history.undo.map((step) => step.kind === 'removed' && step.cleared).lastIndexOf(true);
  const clearing = history.undo[at];
  if (clearing?.kind !== 'removed') return false;
  history.undo.splice(at, 1);
  // The clearing left `left` strokes, first in the list; after them come the strokes written since.
  // For each cleared stroke, how many of those it left stood before it; which are still there.
  const count = clearing.strokes.length;
  const keptBefore = clearing.strokes.map((placed, i) => placed.index - i);
  let present: number[] | null = null;
  const keptNow = () => (present ? present.length : clearing.left);
  const moved = (place: number) => {
    if (place >= keptNow()) return place + count;
    const rank = present ? present[place] : place;
    return place + keptBefore.filter((before) => before <= rank).length;
  };
  for (const step of history.undo.slice(at)) {
    switch (step.kind) {
      case 'removed': {
        const places = step.strokes.map((placed) => placed.index);
        step.strokes = step.strokes.map((placed) => ({ ...placed, index: moved(placed.index) }));
        if (places.some((place) => place < keptNow())) {
          const ranks: number[] = present ?? Array.from({ length: clearing.left }, (_, i) => i);
          present = ranks.filter((_, i) => !places.includes(i));
        }
        step.left += count;
        break;
      }
      case 'recoloured':
        step.changes = step.changes.map((change) => ({ ...change, index: moved(change.index) }));
        break;
      case 'added':
        step.indices = step.indices.map((index) => index + count);
        break;
    }
  }
  history.redo = [];
  const kept = present;
  const back = clearing.strokes.map((placed, i) => ({
    index: i + (kept ? kept.filter((rank) => rank < keptBefore[i]).length : Math.min(keptBefore[i], clearing.left)),
    stroke: placed.stroke,
  }));
  insertStrokes(page.strokes, back);
  history.revision = lastRevision += 1;
  broadcast([edited(s, page, { inserted: back })]);
  return true;
}

/**
 * How the current page is to be held: the setting, except for a resumed sheet that is not the same
 * way round (a quarter turn on the same side: the button stays away from the writing hand).
 */
function holdFor(page: Note, s: Session): Orientation {
  if (!heldFixed(s, page)) return heldAs;
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

/** A stroke in progress as `strokeStart` sends it. */
function wireStroke(stroke: Stroke, recv: (i: number) => number): LiveStroke {
  const { points, ...style } = stroke;
  return { ...style, points: points.map((p, i) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, recv: recv(i) })) };
}

function broadcast(events: LiveEvent[]): void {
  if (events.length === 0) return;
  // Like Rust: a snapshot does not hold the strokes in progress, so it is followed by them, the tablet's first.
  const last = events.map((e) => e.type).lastIndexOf('snapshot');
  if (last >= 0 && session) {
    const after = events.slice(last);
    const started = (computer: boolean) => after.some((e) => e.type === 'strokeStart' && (e.pen === 'computer') === computer);
    const drawing = session.current;
    if (drawing && !started(false)) {
      events = [...events, { type: 'strokeStart', pageIndex: session.pageIndex, stroke: wireStroke(drawing, () => wallClock()) }];
    }
    const computer = session.computer;
    if (computer && computer.pageIndex === session.pageIndex && !started(true)) {
      const walls = computerWalls.get(computer) ?? [];
      events = [
        ...events,
        { type: 'strokeStart', pageIndex: computer.pageIndex, stroke: wireStroke(computer.stroke, (i) => walls[i] ?? wallClock()), pen: 'computer', id: computer.id },
      ];
    }
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
  // Like `hub::written_sheet`: a sheet without new ink is the sheet it was, and the blackboard's
  // slate is written only with ink under it.
  const same = sheet && JSON.stringify(page.strokes) === JSON.stringify(sheet.before.strokes);
  // The paper: slate when ink was added under the board, else the own paper once it was changed on purpose, else the one read.
  const written: Note =
    sheet && (same || !sheet.inkOnBoard)
      ? {
          ...page,
          layers: same && !sheet.layersEdited ? sheet.before.layers : page.layers,
          background: { ...page.background, paper: sheet.paperEdited ? ownPaper(s, page) : sheet.before.background.paper },
        }
      : page;
  library.notes.set(path, {
    note: copy({ ...written, modified: Date.now() }),
    stamp,
    binder: sheet?.binder ?? kept?.binder ?? '',
    // Like `original::keep_resumed_original`: the sheet before the resume, blank or not.
    original:
      kept?.original ??
      (sheet && !same
        ? copy({
            source: 'firstVersion',
            page: sheet.before.page,
            layers: sheet.before.layers,
            strokes: sheet.before.strokes,
            paper: sheet.before.background.paper,
            ...(sheet.before.backgroundImage ? { backgroundImage: sheet.before.backgroundImage } : {}),
          })
        : undefined),
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
  // A page with an image is kept even without ink (a worksheet shown in class); paper alone is nothing.
  const untouched = sheet
    ? looksLike(page) === sheet.seed && !sheet.paperEdited && !sheet.imageEdited
    : page.strokes.length === 0 && page.backgroundImage === undefined;
  if (!s.saved.has(page.id) && untouched) return [];
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
  const sheet = s.resumed.get(page.id);
  if (sheet && s.view.board) sheet.inkOnBoard = true;
  const events: LiveEvent[] = [{ type: 'strokeEnd', pageIndex: s.pageIndex }];
  // A page becomes a note when its first stroke ends.
  if (!s.saved.has(page.id)) events.push(...savePage(s, page));
  return events;
}

// --- The computer's pen (M14) ------------------------------------------------------------------

/** The wall time of each point of a computer stroke in progress (its `recv`), for a snapshot that repeats it. */
const computerWalls = new WeakMap<ComputerDrawing, number[]>();

/** A sample checked as `ComputerPenInput::checked` does: finite coordinates, the pressure within 0..1 (1 if not a number). */
function checkedSamples(samples: readonly ComputerSample[] | undefined, begin: boolean): ComputerSample[] {
  const list = samples ?? [];
  if (list.length > MAX_PEN_SAMPLES) throw invalid(`too many samples in one call (${list.length})`);
  const kept = list
    .filter((sample) => Number.isFinite(sample.x) && Number.isFinite(sample.y))
    .map((sample) => ({ ...sample, p: Number.isFinite(sample.p) ? Math.min(1, Math.max(0, sample.p)) : 1 }));
  if (begin && kept.length === 0) throw invalid('a stroke needs one sample at least');
  return kept;
}

/** The style as Rust sanitizes it: a tool of the list, an opaque colour, a width that makes sense. */
function sanitizedStyle(style: LiveStyle | undefined): LiveStyle {
  const tools: readonly string[] = ['ballpoint', 'fountainPen', 'feltTip', 'pencil', 'brush', 'highlighter', 'chalk'];
  const tool = style && tools.includes(style.tool) ? style.tool : 'ballpoint';
  const channel = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? Math.min(255, Math.max(0, Math.round(v))) : 0);
  const c = style?.color;
  const color = c ? { r: channel(c.r), g: channel(c.g), b: channel(c.b), a: 255 } : { ...BLACK };
  const width = style && Number.isFinite(style.width) ? Math.min(5000, Math.max(1, style.width)) : 60;
  return { tool, color, width };
}

/** The origin of a page's times (Unix ms): its `created`, or a resumed sheet's shifted origin. */
function timeOrigin(s: Session, page: Note): number {
  return s.resumed.get(page.id)?.origin ?? page.created;
}

/** Appends `samples` to the computer's stroke (times as `sample_wall`, `note_time`); returns its new points. */
function computerSamples(s: Session, drawing: ComputerDrawing, samples: readonly ComputerSample[], now: number): LivePoint[] {
  const page = s.pages[drawing.pageIndex];
  const wallNow = wallClock();
  const { width, height } = page.page;
  const walls = computerWalls.get(drawing) ?? [];
  computerWalls.set(drawing, walls);
  const points: LivePoint[] = [];
  const room = MAX_COMPUTER_POINTS - drawing.stroke.points.length;
  for (const sample of samples.slice(0, Math.max(0, room))) {
    let wall = wallNow - Math.max(0, now - sample.ms);
    if (drawing.lastWall !== null) wall = Math.max(wall, drawing.lastWall);
    wall = Math.max(Math.min(wall, wallNow), wallNow - MAX_SAMPLE_AGE_MS);
    drawing.lastWall = wall;
    const previous = drawing.stroke.points[drawing.stroke.points.length - 1]?.t;
    const since = Math.floor(wall - timeOrigin(s, page));
    const t = Math.max(previous ?? 0, Math.min(0xffff_ffff, Math.max(0, since)));
    const x = Math.min(2 * width, Math.max(-width, sample.x));
    const y = Math.min(2 * height, Math.max(-height, sample.y));
    drawing.stroke.points.push({ x, y, pressure: sample.p, t });
    walls.push(wall);
    points.push({ x, y, p: sample.p, t, recv: wall });
  }
  return points;
}

/** Ends the computer's stroke in progress: it joins the tail of its page, as an undo step (`strokeEnd`, then `history`). */
function endComputer(s: Session): LiveEvent[] {
  const drawing = s.computer;
  if (!drawing) return [];
  s.computer = null;
  const page = s.pages[drawing.pageIndex];
  const place = page.strokes.length;
  page.strokes.push(drawing.stroke);
  const history = historyOf(s, page);
  record(history, { kind: 'added', indices: [place] });
  const sheet = s.resumed.get(page.id);
  if (sheet && s.view.board) sheet.inkOnBoard = true;
  const events: LiveEvent[] = [
    { type: 'strokeEnd', pageIndex: drawing.pageIndex, pen: 'computer', id: drawing.id },
    { type: 'history', pageIndex: drawing.pageIndex, canUndo: history.undo.length > 0, canRedo: history.redo.length > 0 },
  ];
  // A page's first stroke creates its note, whichever pen.
  if (!s.saved.has(page.id)) events.push(...savePage(s, page));
  return events;
}

/** Drops the computer's stroke in progress (`strokeCancel`, with its number). */
function cancelComputer(s: Session): LiveEvent[] {
  const drawing = s.computer;
  if (!drawing) return [];
  s.computer = null;
  return [{ type: 'strokeCancel', pageIndex: drawing.pageIndex, pen: 'computer', id: drawing.id }];
}

/** `live_computer_pen`, as `LiveHub::computer_pen` and `LiveSession::computer_pen` answer it. */
function computerPen(input: LiveComputerPenInput): boolean {
  const s = session;
  if (!s) {
    // Stopped: the pen only learns that its stroke does not go on; before any start, nothing runs.
    if (input.kind === 'begin' || !stoppedOnce) throw { code: 'liveNotRunning', message: 'live mode is not running' };
    return false;
  }
  const samples = input.kind === 'cancel' ? [] : checkedSamples(input.samples, input.kind === 'begin');
  const ours = s.computer !== null && s.computer.id === input.stroke;
  const events: LiveEvent[] = [];
  let goesOn = true;
  switch (input.kind) {
    case 'begin': {
      if (input.pageIndex !== s.pageIndex) throw stale();
      const page = s.pages[s.pageIndex];
      const layer = layersOf(s, page).computer;
      if (page.layers[layer]?.visible === false) throw { code: 'liveLayerHidden', message: 'the chosen layer is hidden' };
      events.push(...endComputer(s));
      const { tool, color, width } = sanitizedStyle(input.style);
      const drawing: ComputerDrawing = { id: input.stroke, pageIndex: s.pageIndex, stroke: { tool, color, width, layer, points: [] }, lastWall: null };
      s.computer = drawing;
      const points = computerSamples(s, drawing, samples, input.now);
      events.push({ type: 'strokeStart', pageIndex: s.pageIndex, stroke: { tool, color, width, layer, points }, pen: 'computer', id: input.stroke });
      break;
    }
    case 'points':
    case 'end': {
      if (!ours || !s.computer) return false;
      const drawing = s.computer;
      const points = computerSamples(s, drawing, samples, input.now);
      if (points.length > 0) events.push({ type: 'points', pageIndex: drawing.pageIndex, points, pen: 'computer', id: drawing.id });
      if (input.kind === 'end') {
        events.push(...endComputer(s));
      } else if (drawing.stroke.points.length >= MAX_COMPUTER_POINTS) {
        // The limit: the stroke ends there, kept, and the window stops sending for that gesture.
        events.push(...endComputer(s));
        goesOn = false;
      }
      break;
    }
    case 'cancel':
      if (!ours) return false;
      events.push(...cancelComputer(s));
      break;
  }
  broadcast(events);
  return goesOn;
}

// --- Layers during live (M14) ----------------------------------------------------------------------

function badLayer(message: string): { code: string; message: string } {
  return { code: 'liveLayerInvalid', message };
}

/** An index of `page.layers`, checked as `live/layers.rs` does. */
function existingLayer(page: Note, value: unknown): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value >= page.layers.length) {
    throw badLayer(`there is no layer ${String(value)}`);
  }
  return value;
}

/** Where a layer index goes once the layer `from` moved to `to` (`followMove`). */
function followMoved(index: number, from: number, to: number): number {
  if (index === from) return to;
  if (from < to && index > from && index <= to) return index - 1;
  if (from > to && index < from && index >= to) return index + 1;
  return index;
}

/** Where a layer index goes once layer `merged` went into the one below it. */
function followMerged(index: number, merged: number): number {
  return index >= merged ? Math.max(0, index - 1) : index;
}

/** Every stroke the page's history keeps (to renumber their layers). */
function keptStrokes(history: PageHistory): Stroke[] {
  const out: Stroke[] = [];
  for (const step of history.undo) if (step.kind === 'removed') out.push(...step.strokes.map((placed) => placed.stroke));
  for (const step of history.redo) if (step.kind === 'add') out.push(...step.strokes.map((placed) => placed.stroke));
  return out;
}

/** `live_layers`: refused as a whole, or done, with one snapshot when something changed. */
function layerOp(s: Session, pageIndex: number, op: LiveLayerOp): void {
  const page = s.pages[s.pageIndex];
  const layers = layersOf(s, page);
  if (pageIndex !== s.pageIndex) throw stale();
  let changed = false;
  let edited = false;
  switch (op.kind) {
    case 'add':
      if (page.layers.length >= MAX_LAYERS) throw badLayer('this page has too many layers');
      page.layers.push({ name: `Calque ${page.layers.length + 1}`, visible: true });
      layers.computer = page.layers.length - 1;
      changed = edited = true;
      break;
    case 'rename': {
      const layer = existingLayer(page, op.layer);
      const name = typeof op.name === 'string' ? op.name.trim() : '';
      if (name === '' || [...name].length > MAX_LAYER_NAME) throw badLayer('this name is not valid');
      if (page.layers[layer].name !== name) {
        page.layers[layer] = { ...page.layers[layer], name };
        changed = edited = true;
      }
      break;
    }
    case 'visible': {
      const layer = existingLayer(page, op.layer);
      if (typeof op.visible !== 'boolean') throw badLayer('visible is not a boolean');
      if (page.layers[layer].visible !== op.visible) {
        page.layers[layer] = { ...page.layers[layer], visible: op.visible };
        changed = edited = true;
      }
      break;
    }
    case 'move': {
      const from = existingLayer(page, op.from);
      const to = existingLayer(page, op.to);
      if (from === to) throw badLayer('a layer cannot move onto itself');
      const count = page.layers.length;
      const follow = (index: number) => (index < count ? followMoved(index, from, to) : index);
      const [moved] = page.layers.splice(from, 1);
      page.layers.splice(to, 0, moved);
      // Places unchanged: the revision and a selection stay.
      for (const stroke of [...page.strokes, ...keptStrokes(historyOf(s, page))]) stroke.layer = follow(stroke.layer);
      layers.tablet = follow(layers.tablet);
      layers.computer = follow(layers.computer);
      if (s.current) s.current.layer = follow(s.current.layer);
      if (s.computer && s.computer.pageIndex === s.pageIndex) s.computer.stroke.layer = follow(s.computer.stroke.layer);
      changed = edited = true;
      break;
    }
    case 'merge': {
      const layer = existingLayer(page, op.layer);
      if (layer === 0) throw badLayer('the bottom layer has none below it');
      const lower = page.layers[layer - 1];
      const upper = page.layers[layer];
      page.layers.splice(layer, 1);
      page.layers[layer - 1] = { ...lower, visible: lower.visible || upper.visible };
      // The merged strokes go after the others: drawn above the lower layer's ink, as in the editor.
      const kept: Stroke[] = [];
      const merged: Stroke[] = [];
      for (const stroke of page.strokes) {
        if (stroke.layer === layer) merged.push({ ...stroke, layer: layer - 1 });
        else kept.push(stroke.layer > layer ? { ...stroke, layer: stroke.layer - 1 } : stroke);
      }
      page.strokes = [...kept, ...merged];
      // Places moved: the steps would name other strokes.
      const history = historyOf(s, page);
      history.undo = [];
      history.redo = [];
      history.revision = lastRevision += 1;
      layers.tablet = followMerged(layers.tablet, layer);
      layers.computer = followMerged(layers.computer, layer);
      if (s.current) s.current.layer = followMerged(s.current.layer, layer);
      if (s.computer && s.computer.pageIndex === s.pageIndex) s.computer.stroke.layer = followMerged(s.computer.stroke.layer, layer);
      changed = edited = true;
      break;
    }
    case 'computer': {
      const layer = existingLayer(page, op.layer);
      changed = layers.computer !== layer;
      layers.computer = layer;
      break;
    }
    case 'tablet': {
      const layer = existingLayer(page, op.layer);
      changed = layers.tablet !== layer;
      layers.tablet = layer;
      break;
    }
    default:
      throw badLayer(String((op as { kind: unknown }).kind));
  }
  const sheet = s.resumed.get(page.id);
  if (edited && sheet) sheet.layersEdited = true;
  if (changed) broadcastSnapshot();
}

/** The size of a staged image: the browser's stand-in (`mock-background.ts`) makes a PDF page 2480 x 3508 pixels, a photo 1600 x 1200. */
function pixelsOfImage(image: BackgroundImage, key: string): ImagePixels {
  return image.pdfPage === undefined ? { key, width: 1600, height: 1200 } : { key, width: 2480, height: 3508 };
}

/** The most pages away an image's rectangle may reach (`live/paper.rs`). */
const MAX_IMAGE_REACH = 16;
const MAX_IMAGE_NAME = 255;

/** An image's description as `live_page_image` accepts it: opacity clamped, quarter turns taken modulo 4. */
function checkedImage(page: PageSize, image: BackgroundImage): BackgroundImage {
  if (typeof image?.name !== 'string' || image.name === '' || [...image.name].length > MAX_IMAGE_NAME) {
    throw invalid('the image has no valid name');
  }
  if (image.pdfPage !== undefined && (!Number.isInteger(image.pdfPage) || image.pdfPage < 1)) throw invalid('the PDF page is not valid');
  const { x, y, width, height } = image.rect ?? ({} as Partial<BackgroundImage['rect']>);
  if (![x, y, width, height].every((value) => typeof value === 'number' && Number.isFinite(value))) {
    throw invalid('the image rectangle is not valid');
  }
  const [rx, ry, rw, rh] = [x as number, y as number, width as number, height as number];
  if (rw <= 0 || rh <= 0) throw invalid('the image rectangle is not valid');
  const far = (value: number, size: number) => Math.abs(value) > MAX_IMAGE_REACH * size;
  if (far(rx, page.width) || far(ry, page.height) || far(rx + rw, page.width) || far(ry + rh, page.height)) {
    throw invalid('the image is too far from the page');
  }
  const opacity = image.opacity ?? 1;
  if (typeof opacity !== 'number' || !Number.isFinite(opacity)) throw invalid('the opacity is not a number');
  const turns = (((image.quarterTurns ?? 0) % 4) + 4) % 4;
  return { ...image, rect: { x: rx, y: ry, width: rw, height: rh }, quarterTurns: turns, opacity: Math.min(1, Math.max(0, opacity)) };
}

/** `live_page_paper`: the paper of the page on screen. The blackboard keeps the page slate: only the template and the texture show. */
function pagePaper(s: Session, pageIndex: number, background: Background): void {
  if (pageIndex !== s.pageIndex) throw stale();
  const page = s.pages[pageIndex];
  const before = JSON.stringify([page.background, ownPaper(s, page)]);
  if (s.view.board) {
    s.own.set(page.id, background.paper);
    page.background = { ...copy(background), paper: page.background.paper };
  } else {
    page.background = copy(background);
    s.own.set(page.id, background.paper);
  }
  if (JSON.stringify([page.background, ownPaper(s, page)]) === before) return;
  const sheet = s.resumed.get(page.id);
  if (sheet) sheet.paperEdited = true;
  broadcastSnapshot();
}

/** `live_page_image`: the image of the page on screen. Not undone by Ctrl+Z, no history step. */
function pageImage(s: Session, pageIndex: number, op: LivePageImageOp): void {
  if (pageIndex !== s.pageIndex) throw stale();
  const page = s.pages[pageIndex];
  const current = page.backgroundImage;
  switch (op.kind) {
    case 'set': {
      // Only what `stage_background_image` staged is known (its keys are `mock-...`).
      if (typeof op.key !== 'string' || !op.key.startsWith('mock-')) throw { code: 'backgroundMissing', message: String(op.key) };
      const image = checkedImage(page.page, op.image);
      page.backgroundImage = image;
      s.images.set(page.id, pixelsOfImage(image, op.key));
      break;
    }
    case 'place': {
      if (!current) throw invalid('the page has no image');
      const image = checkedImage(page.page, op.image);
      if (image.name !== current.name || image.pdfPage !== current.pdfPage) throw invalid("this is not the page's image");
      if (JSON.stringify(image) === JSON.stringify(current)) return;
      page.backgroundImage = image;
      break;
    }
    case 'remove':
      if (!current) return;
      delete page.backgroundImage;
      s.images.delete(page.id);
      break;
    default:
      throw invalid(String((op as { kind: unknown }).kind));
  }
  const sheet = s.resumed.get(page.id);
  if (sheet) sheet.imageEdited = true;
  broadcastSnapshot();
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
        const layer = layersOf(s, page).tablet;
        s.current = { tool, color: { ...color }, width, layer, points: [ink] };
        events.push({ type: 'strokeStart', pageIndex: s.pageIndex, stroke: { tool, color: { ...color }, width, layer, points: [point] } });
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
  // The paper of the page on screen goes on (its own paper, not the slate of the board; no image).
  const from = s.pages[s.pageIndex];
  const own = from ? ownPaper(s, from) : undefined;
  const page = blankPage(s.view.board, from && own ? { paper: own, template: from.background.template, texture: from.background.texture } : undefined);
  if (own) s.own.set(page.id, own);
  s.pages.push(page);
  s.pageStart.set(page.id, s.record);
}

function goTo(s: Session, index: number): LiveEvent[] {
  // Both strokes in progress end there (kept); the page keeps its two layers.
  const events = [...endStroke(s), ...endComputer(s)];
  const leaving = s.pages[s.pageIndex];
  if (s.saved.has(leaving.id)) events.push(...savePage(s, leaving));
  s.pageIndex = index;
  followTablet(s.pages[index]);
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
      const page = s.pages[s.pageIndex];
      // Beyond the limit the tablet's button does nothing at all.
      if (page.layers.length >= MAX_LAYERS) return;
      events = endStroke(s);
      page.layers.push({ name: `Calque ${page.layers.length + 1}`, visible: true });
      // The tablet writes on the new layer; the computer stays on its own.
      layersOf(s, page).tablet = page.layers.length - 1;
      break;
    }
    case 'clearPage': {
      s.current = null;
      // The computer's stroke in progress is dropped too, told before the snapshot.
      events = cancelComputer(s);
      const page = s.pages[s.pageIndex];
      // What shows: nobody can tell that the ink of a hidden layer is going.
      const shown = page.strokes.flatMap((stroke, index) => (isDrawn(page, stroke) ? [index] : []));
      if (shown.length > 0) {
        const history = historyOf(s, page);
        const strokes = takeStrokes(page, shown);
        record(history, { kind: 'removed', strokes, cleared: true, left: page.strokes.length });
        history.revision = lastRevision += 1;
      }
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
  if (view.board) s.own.set(page.id, page.background.paper);
  page.background = {
    ...page.background,
    paper: view.board ? 'slate' : (s.own.get(page.id) ?? s.resumed.get(page.id)?.before.background.paper ?? 'white'),
  };
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
    layersOf: new Map(),
    style: DEFAULT_STYLE,
    styleBeforeBoard: null,
    view: { ...IDLE_VIEW },
    hover: null,
    current: null,
    computer: null,
    saved: new Map(),
    failed: new Set(),
    record: 0,
    pageStart: new Map(),
    pen: new PenScript(),
    histories: new Map(),
    resumed: new Map(),
    own: new Map(),
    images: new Map(),
    forks: [],
  };
  if (first) {
    const { page, sheet } = first;
    s.pages.push(page);
    s.pageStart.set(page.id, s.record);
    s.resumed.set(page.id, sheet);
    if (page.backgroundImage) s.images.set(page.id, pixelsOfImage(page.backgroundImage, `resume-${page.id}`));
    // The blackboard is on only for a slate sheet; new ink goes on the last visible layer.
    s.view = { ...s.view, x: page.page.width / 2, y: page.page.height / 2, board: page.background.paper === 'slate' };
    if (s.view.board) s.style = CHALK_STYLE;
    if (!page.layers.some((layer) => layer.visible)) page.layers.push({ name: `Calque ${page.layers.length + 1}`, visible: true });
    // The tablet goes on the last visible layer, the computer with it.
    const visible = page.layers.map((layer) => layer.visible).lastIndexOf(true);
    s.layersOf.set(page.id, { tablet: visible, computer: visible });
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
  stoppedOnce = false;
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
    before: copy(stored.note),
    inkOnBoard: false,
    origin: wallClock() - (last + 1000),
    layersEdited: false,
    paperEdited: false,
    imageEdited: false,
  };
  stoppedOnce = false;
  return openSession({ page, sheet });
}

function stopSession(): LiveStopped {
  const s = session;
  if (!s) return { saved: [], forks: [], narration: null };
  stopTimers(s);
  const events = [...endStroke(s), ...endComputer(s)];
  for (const page of s.pages) events.push(...savePage(s, page).filter((e) => e.type !== 'saved'));
  const paths = [...s.saved.values()];
  // A `saved` after a failure means that everything is written: not sent while a page failed.
  if (s.failed.size === 0) events.push({ type: 'saved', paths });
  session = null;
  stoppedOnce = true;
  broadcast([...events, { type: 'snapshot', snapshot: snapshot() }]);
  const forks = s.forks.flatMap((fork) => (fork.type === 'sheetForked' ? [fork] : []));
  return {
    saved: paths,
    forks: forks.map(({ pageIndex, reason, original, path }) => ({ pageIndex, reason, original, path })),
    narration: null,
  };
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
    case 'live_restore_cleared':
      return restoreCleared(running());
    case 'live_edit_strokes':
      return editStrokes(running(), args.pageIndex as number, args.revision as number, args.edit as LiveStrokesEdit);
    case 'live_undo_edit': {
      const s = running();
      if (args.pageIndex !== s.pageIndex) throw stale();
      return undoStep(s);
    }
    case 'live_redo_edit': {
      const s = running();
      if (args.pageIndex !== s.pageIndex) throw stale();
      return redoStep(s);
    }
    case 'live_computer_pen':
      return computerPen(args.input as LiveComputerPenInput);
    case 'live_layers':
      layerOp(running(), args.pageIndex as number, args.op as LiveLayerOp);
      return undefined;
    case 'live_page_paper':
      pagePaper(running(), args.pageIndex as number, args.background as Background);
      return undefined;
    case 'live_page_image':
      pageImage(running(), args.pageIndex as number, args.op as LivePageImageOp);
      return undefined;
    case 'live_page_background':
      return backgroundDataUrl(
        args.page as PageSize,
        args.background as Background,
        args.widthPx as number,
        session?.pages[args.pageIndex as number]?.backgroundImage,
      );
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
  lastRevision = 0;
  stoppedOnce = false;
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
  // A resumed sheet, or a page with an image and ink, is never turned: the session follows it instead (`holdFor`).
  if (session && heldFixed(session, page)) return;
  const from = pageOrientation.get(page.id) ?? 'landscape';
  const turns = turnsFrom(heldAs, from);
  pageOrientation.set(page.id, heldAs);
  if (turns === 0) return;
  const { width, height } = page.page;
  page.strokes = turnStrokes(page.strokes, width, height, turns);
  // What can come back turns with it, so that it returns where it would be had it stayed.
  for (const step of (session && session.histories.get(page.id)?.undo) ?? []) {
    if (step.kind !== 'removed') continue;
    const turned = turnStrokes(step.strokes.map((placed) => placed.stroke), width, height, turns);
    step.strokes = step.strokes.map((placed, i) => ({ index: placed.index, stroke: turned[i] }));
  }
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
  const turns = s && !heldFixed(s, s.pages[s.pageIndex]) ? turnsFrom(orientation, pageOrientation.get(s.pages[s.pageIndex].id) ?? 'landscape') : 0;
  // Both strokes in progress end (kept) before the page turns, so that it turns with their ink.
  const events = s ? [...endStroke(s), ...endComputer(s)] : [];
  heldAs = orientation;
  if (!s || !before) return;
  s.hover = null;
  s.pages.forEach((page, index) => {
    const { undo, redo } = historyOf(s, page);
    // A page without ink takes the new size: a blank one not written yet, or one with an image (written, M17), whose
    // image is placed again with the fit it had.
    const blank = page.strokes.length === 0 && !s.resumed.has(page.id) && undo.length === 0 && redo.length === 0;
    if (blank && (page.backgroundImage !== undefined || !s.saved.has(page.id))) {
      const old = page.page;
      page.page = pageSizeFor(orientation, PAGE.width, PAGE.height);
      pageOrientation.set(page.id, orientation);
      const image = page.backgroundImage;
      const pixels = s.images.get(page.id);
      if (image && pixels) {
        const fit = currentFit(image, old, pixels) ?? 'contain';
        page.backgroundImage = refit({ ...image, quarterTurns: 0 }, page.page, pixels, fit);
      }
      if (s.saved.has(page.id)) events.push(...savePage(s, page));
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
