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
 * - Refusals: `projectionStale` for `strokes` without a page or whose `keep` exceeds the page's strokes;
 *   `projectionInvalid` for indices or ids that are not whole numbers, points that are not finite and more points or
 *   strokes than the limits.
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
    orientation: note.page.width >= note.page.height ? 'landscape' : 'portraitLeft',
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

const whole = (n: unknown): n is number => typeof n === 'number' && Number.isInteger(n) && n >= 0;

function checkView(view: ProjectionView): ProjectionView {
  if (![view?.zoom, view?.x, view?.y].every(Number.isFinite)) throw refused('projectionInvalid', 'the view is not finite');
  return { zoom: Math.min(8, Math.max(1, view.zoom)), x: view.x, y: view.y };
}

function checkPoints(points: readonly ProjectionPoint[]): LivePoint[] {
  if (points.length > MAX_PROJECTION_POINTS) throw refused('projectionInvalid', `${points.length} points in one update`);
  return points.map((p) => {
    if (![p.x, p.y, p.p, p.t, p.wall].every(Number.isFinite)) throw refused('projectionInvalid', 'a point is not finite');
    return { x: p.x, y: p.y, p: p.p, t: p.t, recv: p.wall };
  });
}

function apply(update: ProjectionUpdate): LiveEvent[] {
  switch (update.kind) {
    case 'page': {
      revision += 1;
      scene = { note: copy(update.note), imageKey: update.imageKey ?? null, view: checkView(update.view), revision };
      return opening();
    }
    case 'strokes': {
      const { keep, tail, ended } = update;
      if (!whole(keep) || (ended !== null && !whole(ended))) throw refused('projectionInvalid', 'keep and ended are whole numbers');
      if (tail.length > MAX_PROJECTION_STROKES) throw refused('projectionInvalid', `${tail.length} strokes in one tail`);
      if (!scene) throw refused('projectionStale', 'no page yet');
      const strokes = scene.note.strokes;
      if (keep > strokes.length) throw refused('projectionStale', `keep ${keep} of ${strokes.length} strokes`);
      const removed = Array.from({ length: strokes.length - keep }, (_, i) => keep + i);
      const added = copy(tail) as Stroke[];
      scene.note = { ...scene.note, strokes: [...strokes.slice(0, keep), ...added] };
      scene.revision = ++revision;
      const events: LiveEvent[] = [
        {
          type: 'strokesEdited',
          pageIndex: 0,
          revision: scene.revision,
          removed,
          inserted: added.map((stroke, i) => ({ index: keep + i, stroke })),
          recoloured: [],
          canUndo: false,
          canRedo: false,
        },
      ];
      if (ended !== null && current?.id === ended) {
        current = null;
        events.push({ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: ended });
      }
      return events;
    }
    case 'view': {
      const view = checkView(update.view);
      if (scene) scene.view = view;
      return [{ type: 'view', view: { ...view, board: false, laser: false } }];
    }
    case 'strokeBegin': {
      const { id, tool, color, width, layer } = update;
      if (!whole(id) || !whole(layer) || !Number.isFinite(width)) throw refused('projectionInvalid', 'id, layer and width');
      const points = checkPoints(update.points);
      // A begin replaces the stroke in progress.
      current = { id, stroke: { tool, color: { ...color }, width, layer, points } };
      return startEvent();
    }
    case 'strokePoints': {
      if (!whole(update.id)) throw refused('projectionInvalid', 'the id is a whole number');
      const points = checkPoints(update.points);
      if (current?.id !== update.id) return [];
      current.stroke.points.push(...points);
      return [{ type: 'points', pageIndex: 0, points: copy(points), pen: 'computer', id: update.id }];
    }
    case 'strokeCancel': {
      if (!whole(update.id)) throw refused('projectionInvalid', 'the id is a whole number');
      if (current?.id !== update.id) return [];
      current = null;
      return [{ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: update.id }];
    }
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
