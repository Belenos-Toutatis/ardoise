/**
 * Browser stand-in for the editor's projection commands (`projection_*`, milestone M17; Rust side `projection.rs`,
 * docs/design/paper-projection-narration.md §2.3), with Rust's rules, so that the editor's tab, the feed and the
 * projection window work in a plain browser and in the tests:
 *
 * - `projection_open` chooses the screen as `open_presentation` does (the one asked for, else the saved one, else the
 *   first that is not the primary one, else the primary one; `noMonitor` without any), closes live's presentation
 *   (they never coexist) and tells the watcher; `projection_close` drops the scene, the stroke in progress and the
 *   subscriber, and tells the watcher (Rust's `ProjectionRuntime::closed`, also what Esc in the window does).
 * - `projection_watch` and `projection_subscribe` keep the latest callback (one editor, one window); the subscriber
 *   gets the snapshot of a one-page session, then live's own events: `page` → a snapshot (followed by the stroke in
 *   progress), `strokes` → `strokesEdited` (then `strokeCancel` when `ended` names the stroke in progress, in the same
 *   batch), `view` → `view`, `strokeBegin` / `strokePoints` / `strokeCancel` → the computer's `strokeStart` / `points`
 *   / `strokeCancel`, each point's `recv` its pointer sample's wall time. Updates while nobody subscribes are applied,
 *   not sent.
 * - Refusals (as package C's `projection.rs`): `projectionStale` for every update while the projection is closed
 *   (nothing kept), for `strokes`, `view`, `strokeBegin` and `strokePoints` before any page, and for a `keep` beyond the
 *   page's strokes (a `strokeCancel` without a page does nothing); `projectionInvalid` for a `keep`, an id, an `ended` or
 *   a layer that is not a whole number from 0, a point that is not finite, more than `MAX_PROJECTION_POINTS` points in
 *   one call, a page without a size or a layer. A view is kept on the page (live's `sanitized`); the same view again,
 *   or a `strokes` update that changes nothing, sends nothing; a `strokeBegin` while a stroke is in progress cancels it
 *   first; `ended` drops only the stroke in progress; points beyond `MAX_PROJECTED_STROKE_POINTS` are not kept.
 *
 * Several tabs of a browser (`npm run dev`): the tab that gets the updates also posts its batches on a
 * `BroadcastChannel`, so that a tab opened at `?view=projection` draws them (and its Esc closes the projection of the
 * editor's tab). Not in the tests (`import.meta.env.VITEST`), whose files run side by side.
 */
import { mockLiveInvoke } from './mock-live';
import { UNHANDLED } from './mock-data';
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot, LiveStroke, MonitorInfo } from './live';
import type { Note, Stroke } from './note';
import {
  MAX_PROJECTION_POINTS,
  MAX_PROJECTION_STROKES,
  type ProjectionPoint,
  type ProjectionStatus,
  type ProjectionUpdate,
  type ProjectionView,
} from './projection';

/** What the stand-in holds: Rust's `Projection`. */
interface Scene {
  note: Note;
  imageKey: string | null;
  view: ProjectionView;
  revision: number;
}

let open: MonitorInfo | null = null;
let scene: Scene | null = null;
let current: { id: number; stroke: LiveStroke } | null = null;
let subscriber: ((batch: LiveBatch) => void) | null = null;
let watcher: ((status: ProjectionStatus) => void) | null = null;
let revision = 0;

/** The pixel sizes the stand-in of the staging gives (`mock-background.ts`): a PDF page at 300 dpi, a photo. */
const PDF_PAGE_PIXELS = { width: 2480, height: 3508 };
const PHOTO_PIXELS = { width: 1600, height: 1200 };

const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const wall = () => (typeof performance !== 'undefined' ? performance.timeOrigin + performance.now() : Date.now());

function refused(code: 'projectionStale' | 'projectionInvalid', message: string): { code: string; message: string } {
  return { code, message };
}

// --- Between tabs (a plain browser only) ----------------------------------------------------------

type Relayed = { kind: 'batch'; batch: LiveBatch } | { kind: 'hello' } | { kind: 'close' };

const relay: BroadcastChannel | null =
  typeof BroadcastChannel !== 'undefined' && !import.meta.env.VITEST ? new BroadcastChannel('ardoise-mock-projection') : null;

relay?.addEventListener('message', (message: MessageEvent<Relayed>) => {
  const data = message.data;
  if (data.kind === 'batch') subscriber?.(copy(data.batch));
  else if (data.kind === 'hello' && scene) relay?.postMessage({ kind: 'batch', batch: { events: opening(), sentAt: wall() } });
  else if (data.kind === 'close') closed();
});

// --- The events -------------------------------------------------------------------------------------

function idleSnapshot(): LiveSnapshot {
  const page = { width: 29700, height: 21600 };
  return {
    state: 'idle',
    page: {
      id: '',
      title: '',
      created: 0,
      modified: 0,
      page,
      background: { paper: 'white', template: 'blank', texture: false },
      tags: [],
      layers: [{ name: 'Calque 1', visible: true }],
      strokes: [],
      timing: 'recorded',
      origin: { kind: 'live' },
    },
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    canTurnPage: false,
    layer: 0,
    computerLayer: 0,
    style: { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60 },
    view: { zoom: 1, x: page.width / 2, y: page.height / 2, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
    revision: 0,
    canUndo: false,
    canRedo: false,
    pagePaper: 'white',
    imagePixels: null,
  };
}

/** The staged pixels of `key` for the page's image (the stand-in stages every key it is given). */
function pixelsOf(note: Note, key: string | null): LiveSnapshot['imagePixels'] {
  if (!key || !note.backgroundImage) return null;
  const size = note.backgroundImage.pdfPage === undefined ? PHOTO_PIXELS : PDF_PAGE_PIXELS;
  return { key, ...size };
}

/** The snapshot of a one-page session that shows the scene (the idle, empty page without one). */
function snapshot(): LiveSnapshot {
  if (!scene) return { ...idleSnapshot(), presentation: open ? { ...open } : null };
  const { note, view } = scene;
  return {
    state: 'live',
    page: copy(note),
    pageIndex: 0,
    pageCount: 1,
    // As Rust: the default orientation (nothing turns a projected page).
    orientation: 'landscape',
    canTurnPage: false,
    layer: 0,
    computerLayer: 0,
    style: { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60 },
    view: { ...view, board: false, laser: false },
    hover: null,
    presentation: open ? { ...open } : null,
    saved: [],
    revision: scene.revision,
    canUndo: false,
    canRedo: false,
    pagePaper: note.background.paper,
    imagePixels: pixelsOf(note, scene.imageKey),
  };
}

/** The stroke in progress as `strokeStart` sends it. */
function startEvent(): LiveEvent[] {
  return current ? [{ type: 'strokeStart', pageIndex: 0, stroke: copy(current.stroke), pen: 'computer', id: current.id }] : [];
}

/** What a window that subscribes gets first: the snapshot, then the stroke in progress. */
function opening(): LiveEvent[] {
  return [{ type: 'snapshot', snapshot: snapshot() }, ...startEvent()];
}

function emit(events: LiveEvent[]): void {
  if (events.length === 0) return;
  const batch: LiveBatch = { events, sentAt: wall() };
  subscriber?.(copy(batch));
  relay?.postMessage({ kind: 'batch', batch });
}

function tell(): void {
  watcher?.({ open: open ? { ...open } : null });
}

/** Rust's `ProjectionRuntime::closed`: the window went, by any means. */
function closed(): void {
  const was = open;
  open = null;
  scene = null;
  current = null;
  subscriber = null;
  if (was) tell();
}

// --- The checks -----------------------------------------------------------------------------------

/** Points of the stroke in progress kept and sent at most (Rust's `MAX_PROJECTED_STROKE_POINTS`, live's computer pen's). */
export const MAX_PROJECTED_STROKE_POINTS = 100_000;

/** An index, an id or a layer: a whole number from 0 (Rust's `whole`), else `projectionInvalid`. */
function whole(value: unknown, what: string): number {
  if (typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff) return value;
  throw refused('projectionInvalid', `${what} is not a whole number: ${String(value)}`);
}

/** The page size of the scene, or `projectionStale` without one. */
function pageSize(): { width: number; height: number } {
  if (!scene) throw refused('projectionStale', 'no page is projected');
  return scene.note.page;
}

/** Live's `LiveViewState::sanitized`: the zoom within 1..8 (1 when not finite), the centre on the page. */
function sanitizedView(view: ProjectionView, page: { width: number; height: number }): ProjectionView {
  const within = (value: number, max: number) => (Number.isFinite(value) ? Math.min(max, Math.max(0, value)) : max / 2);
  return {
    zoom: Number.isFinite(view?.zoom) ? Math.min(8, Math.max(1, view.zoom)) : 1,
    x: within(view?.x, page.width),
    y: within(view?.y, page.height),
  };
}

/** The points as the window draws them (Rust's `live_points`): finite, at most `MAX_PROJECTION_POINTS`, kept near the page. */
function checkPoints(points: readonly ProjectionPoint[], page: { width: number; height: number }): LivePoint[] {
  if (points.length > MAX_PROJECTION_POINTS) throw refused('projectionInvalid', `${points.length} points in one update`);
  const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));
  return points.map((point) => {
    const { x, y, p, t, wall } = point;
    if (![x, y, p, t, wall].every(Number.isFinite)) throw refused('projectionInvalid', 'a point is not finite');
    return {
      x: clamp(x, -page.width, 2 * page.width),
      y: clamp(y, -page.height, 2 * page.height),
      p: clamp(p, 0, 1),
      t: clamp(Math.floor(t), 0, 0xffff_ffff),
      recv: wall,
    };
  });
}

function strokeCount(count: number, what: string): void {
  if (count > MAX_PROJECTION_STROKES) throw refused('projectionInvalid', `${count} strokes ${what}`);
}

const strokeCancel = (id: number): LiveEvent => ({ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id });

/** The stroke in progress `id` leaves the window; nothing for another stroke. */
function dropCurrent(id: number): LiveEvent[] {
  if (current?.id !== id) return [];
  current = null;
  return [strokeCancel(id)];
}

function apply(update: ProjectionUpdate): LiveEvent[] {
  if (!open) throw refused('projectionStale', 'the projection is closed');
  switch (update.kind) {
    case 'page': {
      const note = update.note;
      if (!(note.page.width > 0) || !(note.page.height > 0)) throw refused('projectionInvalid', 'the page has no size');
      if (note.layers.length === 0) throw refused('projectionInvalid', 'the page has no layer');
      strokeCount(note.strokes.length, 'on the page');
      revision += 1;
      scene = { note: copy(note), imageKey: update.imageKey ?? null, view: sanitizedView(update.view, note.page), revision };
      return opening();
    }
    case 'strokes': {
      const keep = whole(update.keep, 'keep');
      const ended = update.ended === null ? null : whole(update.ended, 'ended');
      strokeCount(update.tail.length, 'in one tail');
      if (!scene) throw refused('projectionStale', 'no page is projected');
      const strokes = scene.note.strokes;
      if (keep > strokes.length) throw refused('projectionStale', `${keep} strokes kept, the page has ${strokes.length}`);
      const events: LiveEvent[] = [];
      if (keep < strokes.length || update.tail.length > 0) {
        const removed = Array.from({ length: strokes.length - keep }, (_, i) => keep + i);
        const added = copy(update.tail) as Stroke[];
        scene.note = { ...scene.note, strokes: [...strokes.slice(0, keep), ...added] };
        scene.revision = ++revision;
        events.push({
          type: 'strokesEdited',
          pageIndex: 0,
          revision: scene.revision,
          removed,
          inserted: added.map((stroke, i) => ({ index: keep + i, stroke })),
          recoloured: [],
          canUndo: false,
          canRedo: false,
        });
      }
      // In the same batch: the stroke is on the page when it leaves the pen.
      if (ended !== null) events.push(...dropCurrent(ended));
      return events;
    }
    case 'view': {
      const view = sanitizedView(update.view, pageSize());
      if (!scene || (view.zoom === scene.view.zoom && view.x === scene.view.x && view.y === scene.view.y)) return [];
      scene.view = view;
      return [{ type: 'view', view: { ...view, board: false, laser: false } }];
    }
    case 'strokeBegin': {
      const id = whole(update.id, 'id');
      const layer = whole(update.layer, 'layer');
      const points = checkPoints(update.points, pageSize()).slice(0, MAX_PROJECTED_STROKE_POINTS);
      const { tool, color, width } = update;
      // A begin replaces the stroke in progress, which the window drops first (kept, it would be among its strokes).
      const events: LiveEvent[] = current ? [strokeCancel(current.id)] : [];
      current = { id, stroke: { tool, color: { ...color }, width, layer, points } };
      return [...events, ...startEvent()];
    }
    case 'strokePoints': {
      const id = whole(update.id, 'id');
      const points = checkPoints(update.points, pageSize());
      if (current?.id !== id) return [];
      const kept = points.slice(0, Math.max(0, MAX_PROJECTED_STROKE_POINTS - current.stroke.points.length));
      if (kept.length === 0) return [];
      current.stroke.points.push(...kept);
      return [{ type: 'points', pageIndex: 0, points: copy(kept), pen: 'computer', id }];
    }
    case 'strokeCancel':
      return dropCurrent(whole(update.id, 'id'));
  }
}

/** The screen `projection_open` uses, chosen as `presentation.rs` does (the list is live's stand-in's). */
function chooseMonitor(requested: string | null): MonitorInfo {
  const list = mockLiveInvoke('list_monitors', {}) as MonitorInfo[];
  const chosen = list.find((m) => m.key === requested) ?? list.find((m) => m.chosen) ?? list.find((m) => !m.primary) ?? list[0];
  if (!chosen) throw { code: 'noMonitor', message: 'no screen is connected' };
  return { ...chosen, chosen: true };
}

// --- The commands ---------------------------------------------------------------------------------

export async function mockProjectionWatch(onStatus: (status: ProjectionStatus) => void): Promise<ProjectionStatus> {
  watcher = onStatus;
  return { open: open ? { ...open } : null };
}

export async function mockProjectionSubscribe(onBatch: (batch: LiveBatch) => void): Promise<LiveSnapshot> {
  subscriber = onBatch;
  // Like Rust: the channel starts with the snapshot and the stroke in progress.
  const first = opening();
  queueMicrotask(() => {
    if (subscriber === onBatch) onBatch(copy({ events: first, sentAt: wall() }));
  });
  // Another tab holds the editor (a plain browser): ask it for its scene.
  if (!scene) relay?.postMessage({ kind: 'hello' });
  return snapshot();
}

export function mockProjectionInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'projection_open': {
      const monitor = chooseMonitor((args.monitor as string | null) ?? null);
      // Live's presentation window and the projection window never coexist.
      mockLiveInvoke('close_presentation', {});
      const was = open;
      open = monitor;
      if (!was || was.key !== monitor.key) tell();
      return { ...monitor };
    }
    case 'projection_close':
      relay?.postMessage({ kind: 'close' });
      closed();
      return undefined;
    case 'projection_update':
      emit(apply(args.update as ProjectionUpdate));
      return undefined;
    default:
      return UNHANDLED;
  }
}

// --- For tests ------------------------------------------------------------------------------------

/** What the stand-in holds now: the screen, the page and its view, the stroke in progress. */
export function mockProjectionState(): {
  open: MonitorInfo | null;
  note: Note | null;
  imageKey: string | null;
  view: ProjectionView | null;
  current: { id: number; points: number } | null;
  subscribed: boolean;
  watched: boolean;
} {
  return {
    open: open ? { ...open } : null,
    note: scene ? copy(scene.note) : null,
    imageKey: scene?.imageKey ?? null,
    view: scene ? { ...scene.view } : null,
    current: current ? { id: current.id, points: current.stroke.points.length } : null,
    subscribed: subscriber !== null,
    watched: watcher !== null,
  };
}

/** The projection closed from its own window (Esc there): what Rust tells the editor. */
export function mockProjectionClosedByWindow(): void {
  closed();
}

/** Forgets everything (between tests). */
export function resetMockProjection(): void {
  open = null;
  scene = null;
  current = null;
  subscriber = null;
  watcher = null;
  revision = 0;
}
