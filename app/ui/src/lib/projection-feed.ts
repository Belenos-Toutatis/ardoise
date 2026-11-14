/**
 * What the editor sends to its projection (milestone M17; docs/design/paper-projection-narration.md §2.4): small
 * updates computed by object identity, the whole page only when its layout changes, the stroke in progress point by
 * point, one call in flight (`lib/projection.ts` is what Rust receives).
 *
 * - **Diff by identity** (`projectionDiff`): the editor's doc is immutable (`lib/editor/state.ts`) and the replay's
 *   `noteAt` keeps the objects of the strokes it shows whole (`lib/replay.ts`), so the strokes that did not change are
 *   the same objects as those sent: the common prefix is found by `===` and only the rest goes (`strokes`). Another note,
 *   page size, paper, image, layers (by value) or image key sends the whole page (`page`), and so does a tail of more
 *   than half the strokes (and more than one: a page is then about the same size, and simpler).
 * - **One call in flight**: every input only records the latest state and asks for a pump in a microtask; the pump sends
 *   one update, and the next one is computed against what was sent when it resolves. In order: the page when nothing
 *   was sent yet or its layout changed (the projection needs a scene first), then what tells Rust that a stroke it
 *   shows in progress is over (`strokes` with `ended`, or `strokeCancel`), then the stroke in progress (`strokeBegin`,
 *   then `strokePoints`, at most `MAX_PROJECTION_POINTS` a call), then the strokes, then the view.
 * - **The stroke in progress.** Rust holds at most one (the window draws it as the computer's stroke): before another
 *   begins, the one it holds is resolved, since the window would otherwise keep the old one as a finished stroke. A stroke
 *   ended with `committed` is held until the strokes update that brings it carries `ended` (the window swaps the stroke in
 *   progress for the finished one in one batch, no flash); if none comes within `END_WAIT_MS`, or the page is sent
 *   instead, a `strokeCancel` follows. A stroke dropped (`committed: false`) is cancelled at once. The points of a stroke
 *   not yet sent when it ends are not sent: the finished stroke brings them.
 * - **A refused call** (`projectionStale`, `projectionInvalid`, an IPC failure) sends the whole page next, and cancels
 *   the stroke Rust may hold; a page refused is sent again when the editor shows another note object, or after
 *   `PAGE_RETRY_MS`, twice as long at each new refusal up to `PAGE_RETRY_MAX_MS` (no loop).
 *   Refusals are counted (`rejections`, `lastError`), never shown: the next page repairs.
 */
import type { InkPoint, Tool } from './ink';
import type { Background, BackgroundImage, Layer, Note, PageSize, Rgba, Stroke } from './note';
import {
  MAX_PROJECTION_POINTS,
  MAX_PROJECTION_STROKES,
  type ProjectionPoint,
  type ProjectionUpdate,
  type ProjectionView,
} from './projection';

/** What the editor tells of its stroke in progress (`Canvas`'s `oninprogress`, package E calls it). */
export type InProgress =
  | {
      kind: 'begin';
      id: number;
      tool: Tool;
      color: Rgba;
      width: number;
      layer: number;
      points: readonly InkPoint[];
      /** Wall clock (Unix ms) at note time `base`: a point's wall time is `wallAtBase + (p.t - base)`. */
      wallAtBase: number;
      base: number;
    }
  /** All the points so far (the feed sends the new ones). */
  | { kind: 'grow'; id: number; points: readonly InkPoint[] }
  /** `committed`: the doc gets the stroke next. */
  | { kind: 'end'; id: number; committed: boolean };

/** The last page sent: what a new note is compared with (`projectionDiff`). */
export interface SentPage {
  id: string;
  page: PageSize;
  background: Background;
  backgroundImage: BackgroundImage | undefined;
  layers: readonly Layer[];
  imageKey: string | null;
  /** The stroke objects as sent: the common prefix is found by `===`. */
  strokes: readonly Stroke[];
  /** The page objects (text frames) as sent: a change of them sends the whole page again. */
  objects: Note['objects'];
}

/** A committed stroke the projection still shows in progress waits this long for the strokes update that brings it. */
export const END_WAIT_MS = 1000;
/** A page Rust refused is tried again after this long, then twice as long each time it is refused again, up to the last. */
export const PAGE_RETRY_MS = 250;
export const PAGE_RETRY_MAX_MS = 8000;

/** What `note` is, as `projectionDiff` remembers it once sent. */
export function sentPageOf(note: Note, imageKey: string | null): SentPage {
  return {
    id: note.id,
    page: note.page,
    background: note.background,
    backgroundImage: note.backgroundImage,
    layers: note.layers,
    imageKey,
    strokes: note.strokes,
    objects: note.objects,
  };
}

/** Plain data compared by value (backgrounds, image placements, layers: a few fields each). */
function sameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((v, i) => sameValue(v, b[i]));
  const ka = Object.keys(a).filter((k) => (a as Record<string, unknown>)[k] !== undefined);
  const kb = Object.keys(b).filter((k) => (b as Record<string, unknown>)[k] !== undefined);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => sameValue((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/** The page objects the projection can show: bookmarks are the editor's only (SPEC §4.4 « Signets »). */
function projectedObjects(objects: Note['objects']): Note['objects'] {
  return objects?.filter((o) => o.kind !== 'bookmark');
}

/**
 * The page as Rust shows it is still `note`'s: same note, size, paper, image, layers, pixels and page objects (a
 * bookmark added, renamed or moved is no change of what the projection shows).
 */
function sameLayout(sent: SentPage, note: Note, imageKey: string | null): boolean {
  return (
    sent.id === note.id &&
    sent.imageKey === imageKey &&
    sent.page.width === note.page.width &&
    sent.page.height === note.page.height &&
    sameValue(sent.background, note.background) &&
    sameValue(sent.backgroundImage ?? null, note.backgroundImage ?? null) &&
    sameValue(sent.layers, note.layers) &&
    (sent.objects === note.objects || sameValue(projectedObjects(sent.objects) ?? [], projectedObjects(note.objects) ?? []))
  );
}

/** The update that brings what was sent to `note`, or null when nothing changed (pure; tested alone). */
export function projectionDiff(
  sent: SentPage | null,
  note: Note,
  imageKey: string | null,
  view: ProjectionView,
): ProjectionUpdate | null {
  const page: ProjectionUpdate = { kind: 'page', note, imageKey, view };
  if (!sent || !sameLayout(sent, note, imageKey)) return page;
  const before = sent.strokes;
  const after = note.strokes;
  if (before === after) return null;
  const common = Math.min(before.length, after.length);
  let keep = 0;
  while (keep < common && before[keep] === after[keep]) keep += 1;
  if (keep === before.length && keep === after.length) return null;
  const tail = after.slice(keep);
  if (tail.length > MAX_PROJECTION_STROKES || (tail.length > 1 && tail.length * 2 > after.length)) return page;
  return { kind: 'strokes', keep, tail, ended: null };
}

/** The projection's view of `page` before the editor gives one: the whole page. */
function wholePageView(page: PageSize): ProjectionView {
  return { zoom: 1, x: Math.round(page.width / 2), y: Math.round(page.height / 2) };
}

function sameView(a: ProjectionView, b: ProjectionView): boolean {
  return a.zoom === b.zoom && a.x === b.x && a.y === b.y;
}

/** The editor's stroke in progress, as the feed follows it. */
interface Drawing {
  id: number;
  begin: Extract<InProgress, { kind: 'begin' }>;
  /** All the points so far (the editor's own array: it may grow in place). */
  points: readonly InkPoint[];
  /** How many of them Rust has. */
  sent: number;
  /** `strokeBegin` was sent. */
  begun: boolean;
}

/** The stroke Rust holds in progress (the window draws it as the computer's), and how it ended in the editor. */
interface Held {
  id: number;
  /** The editor ended it: `true` kept (the doc brings it), `false` dropped; `null` still drawn. */
  committed: boolean | null;
  /** Rust must drop it: no strokes update brought it in time, the page was sent instead, or a call was refused. */
  cancel: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
}

export class ProjectionFeed {
  /** Refused calls so far (`projectionStale`, `projectionInvalid`, IPC), and the last refusal: never shown, the page repairs. */
  rejections = 0;
  lastError: unknown = null;

  private readonly send: (update: ProjectionUpdate) => Promise<void>;
  private sent: SentPage | null = null;
  private latest: { note: Note; imageKey: string | null } | null = null;
  private wanted: ProjectionView | null = null;
  private viewSent: ProjectionView | null = null;
  private drawing: Drawing | null = null;
  private held: Held | null = null;
  /** A page Rust refused: not sent again until another note object is shown, or its retry is due. */
  private refusedNote: Note | null = null;
  private retryMs = PAGE_RETRY_MS;
  private retryTimer: ReturnType<typeof setTimeout> | undefined;
  private inFlight = false;
  private queued = false;
  private disposed = false;

  /** `send` delivers one update to Rust (`api.projectionUpdate`); the feed never has two calls in flight. */
  constructor(send: (update: ProjectionUpdate) => Promise<void>) {
    this.send = send;
  }

  /** The editor shows `note` (with the staged pixels of its image under `imageKey`, or null). */
  show(note: Note, imageKey: string | null): void {
    if (this.latest?.note === note && this.latest.imageKey === imageKey) return;
    this.latest = { note, imageKey };
    if (note !== this.refusedNote) this.clearRefused();
    this.schedule();
  }

  /** The projected area changed. */
  view(view: ProjectionView): void {
    if (this.wanted && sameView(this.wanted, view)) return;
    this.wanted = { zoom: view.zoom, x: view.x, y: view.y };
    this.schedule();
  }

  /** The editor's stroke in progress. */
  stroke(event: InProgress): void {
    switch (event.kind) {
      case 'begin':
        // A begin without the end of the stroke before it: that one was dropped.
        if (this.drawing) this.ended(this.drawing.id, false);
        this.drawing = { id: event.id, begin: event, points: event.points, sent: 0, begun: false };
        break;
      case 'grow':
        if (this.drawing?.id !== event.id) return;
        this.drawing.points = event.points;
        break;
      case 'end':
        this.ended(event.id, event.committed);
        break;
    }
    this.schedule();
  }

  /**
   * Rust knows nothing any more (the projection was closed, or opened again): nothing is sent until the next `show`,
   * which sends the whole page; the stroke being drawn, if any, is no longer followed.
   */
  reset(): void {
    this.sent = null;
    this.viewSent = null;
    this.clearRefused();
    this.retryMs = PAGE_RETRY_MS;
    this.latest = null;
    this.drawing = null;
    this.dropHeld();
  }

  /** The feed goes (the editor closes the projection): nothing more is sent. */
  dispose(): void {
    this.disposed = true;
    this.clearRefused();
    this.dropHeld();
    this.drawing = null;
    this.latest = null;
  }

  /** An update is being sent (tests, the perf harness). */
  get busy(): boolean {
    return this.inFlight;
  }

  private ended(id: number, committed: boolean): void {
    if (this.drawing?.id !== id) return;
    const begun = this.drawing.begun;
    this.drawing = null;
    const held = this.held;
    if (!begun || held?.id !== id) return;
    held.committed = committed;
    if (committed) {
      held.timer = setTimeout(() => {
        if (this.held !== held) return;
        held.cancel = true;
        held.timer = undefined;
        this.schedule();
      }, END_WAIT_MS);
    }
  }

  private clearRefused(): void {
    this.refusedNote = null;
    if (this.retryTimer !== undefined) clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
  }

  private dropHeld(): void {
    if (this.held?.timer !== undefined) clearTimeout(this.held.timer);
    this.held = null;
  }

  private schedule(): void {
    if (this.queued || this.inFlight || this.disposed) return;
    this.queued = true;
    queueMicrotask(() => {
      this.queued = false;
      this.pump();
    });
  }

  private pump(): void {
    if (this.inFlight || this.disposed) return;
    const update = this.next();
    if (!update) return;
    this.inFlight = true;
    let sending: Promise<void>;
    try {
      sending = this.send(update);
    } catch (error) {
      sending = Promise.reject(error);
    }
    sending.then(
      () => {
        this.inFlight = false;
        if (update.kind === 'page') this.retryMs = PAGE_RETRY_MS;
        this.schedule();
      },
      (error: unknown) => {
        this.inFlight = false;
        this.refused(update, error);
        this.schedule();
      },
    );
  }

  /** The next update to send (and what is then taken as sent), or null when Rust has everything. */
  private next(): ProjectionUpdate | null {
    const latest = this.latest;
    const view = this.wanted ?? (latest ? wholePageView(latest.note.page) : null);
    const diff = latest && view ? projectionDiff(this.sent, latest.note, latest.imageKey, view) : null;
    // 1. The page: first of all (the projection needs a scene), and whenever its layout changed.
    if (latest && view && diff?.kind === 'page' && latest.note !== this.refusedNote) {
      this.sent = sentPageOf(latest.note, latest.imageKey);
      this.viewSent = view;
      // A finished stroke Rust still shows in progress is in the page now: drop it.
      if (this.held && this.held.committed === true) this.held.cancel = true;
      return diff;
    }
    if (!this.sent || !latest) return null;
    // 2. A stroke Rust shows in progress that the editor no longer draws.
    const held = this.held;
    if (held && (held.cancel || held.id !== this.drawing?.id)) {
      if (!held.cancel && held.committed === true && diff?.kind === 'strokes') {
        this.dropHeld();
        this.sent = { ...this.sent, strokes: latest.note.strokes };
        return { ...diff, ended: held.id };
      }
      // Kept but not in the doc yet: wait for it, unless another stroke begins (the window would keep this one).
      if (held.cancel || held.committed === false || held.committed === null || this.drawing) {
        this.dropHeld();
        return { kind: 'strokeCancel', id: held.id };
      }
    }
    // 3. The stroke in progress.
    const drawing = this.drawing;
    if (drawing && (!this.held || this.held.id === drawing.id)) {
      if (!drawing.begun) {
        const { id, tool, color, width, layer } = drawing.begin;
        const points = this.points(drawing);
        drawing.begun = true;
        this.held = { id, committed: null, cancel: false, timer: undefined };
        return { kind: 'strokeBegin', id, tool, color, width, layer, points };
      }
      if (drawing.sent < drawing.points.length) return { kind: 'strokePoints', id: drawing.id, points: this.points(drawing) };
    }
    // 4. The strokes.
    if (diff?.kind === 'strokes') {
      this.sent = { ...this.sent, strokes: latest.note.strokes };
      return diff;
    }
    // 5. The view.
    if (view && (!this.viewSent || !sameView(view, this.viewSent))) {
      this.viewSent = view;
      return { kind: 'view', view };
    }
    return null;
  }

  /** The next points of `drawing` Rust lacks (at most `MAX_PROJECTION_POINTS`), with their wall times. */
  private points(drawing: Drawing): ProjectionPoint[] {
    const { wallAtBase, base } = drawing.begin;
    const end = Math.min(drawing.points.length, drawing.sent + MAX_PROJECTION_POINTS);
    const out: ProjectionPoint[] = [];
    for (let i = drawing.sent; i < end; i++) {
      const p = drawing.points[i];
      out.push({ x: p.x, y: p.y, p: p.pressure, t: p.t, wall: wallAtBase + (p.t - base) });
    }
    drawing.sent = end;
    return out;
  }

  /** Rust refused `update`: the whole page goes next, and the stroke it may hold is dropped then begun again. */
  private refused(update: ProjectionUpdate, error: unknown): void {
    this.rejections += 1;
    this.lastError = error;
    if (update.kind === 'page') {
      // Not again at once (no loop): after a while, longer each time, or as soon as the editor shows another note.
      this.clearRefused();
      const refusedNote = update.note;
      this.refusedNote = refusedNote;
      this.retryTimer = setTimeout(() => {
        this.retryTimer = undefined;
        if (this.refusedNote !== refusedNote) return;
        this.refusedNote = null;
        this.schedule();
      }, this.retryMs);
      this.retryMs = Math.min(PAGE_RETRY_MAX_MS, this.retryMs * 2);
    }
    this.sent = null;
    this.viewSent = null;
    if (this.held) {
      if (this.held.timer !== undefined) clearTimeout(this.held.timer);
      this.held.timer = undefined;
      this.held.cancel = true;
    }
    // The stroke being drawn starts again from its first point once Rust dropped what it held.
    if (this.drawing) {
      this.drawing.begun = false;
      this.drawing.sent = 0;
    }
  }
}
