/**
 * The live session as one window sees it (docs/design/tablet-live-classroom.md, "Live
 * session"): a `LiveSnapshot`, then the `LiveBatch`es that follow it. The control window and the
 * presentation window each subscribe (`api.liveSubscribe`) and run this same code.
 *
 * Two kinds of fields:
 * - reactive ones (`$state`) for the interface: session state, page number, style, view,
 *   presentation, saved notes, pages that failed to save, stroke count;
 * - plain ones for the canvases, which are told of every change through `subscribe` and draw
 *   on the next animation frame: the finished strokes of the page (`strokes`, appended in
 *   place; `generation` changes when they are replaced), the stroke in progress (`current`,
 *   whose points grow in place), the hovering pen (`hover`), and the points not painted yet
 *   (for the latency), and the marks written over the presentation (`marks`, replaced whole by
 *   each `marks` event; the viewers of the share draw them, the windows draw their own).
 *
 * Assumptions about the Rust side (package B), each handled both ways where it is unclear:
 * - (supposé) A snapshot's `page.strokes` holds the finished strokes only. If its last stroke is
 *   the one in progress here (same start), it is not drawn twice.
 * - (supposé) `points` without a `strokeStart` (subscribed in the middle of a stroke) start a
 *   stroke with the current style and layer.
 * - (supposé) `saved` events may carry every path saved so far or only the new ones: they are
 *   merged.
 * - Batches that arrive before the subscription's snapshot are kept and applied after it
 *   (Tauri may deliver a channel message before the command's answer).
 */
import type { InkPoint } from './ink';
import { wallClock, type PendingSample } from './latency';
import type {
  LiveBatch,
  LiveEvent,
  LivePoint,
  LiveSessionState,
  LiveSnapshot,
  LiveStyle,
  LiveViewState,
  MonitorInfo,
} from './live';
import { marksFromWire, reuseMarks } from './live-marks';
import { roundView, sameView } from './live-view';
import type { Orientation } from './orientation';
import type { Background, Layer, PageSize, Rgba, Stroke } from './note';

/** A page Rust failed to write into the library. */
export interface SaveError {
  /** 0-based. */
  pageIndex: number;
  /** The library error's code: `errorMessage({ code, message })` says it in words. */
  code: string;
  message: string;
}

export const DEFAULT_STYLE: LiveStyle = { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60 };
export const DEFAULT_VIEW: LiveViewState = { zoom: 1, x: 0, y: 0, board: false, laser: false };
const DEFAULT_PAGE: PageSize = { width: 29700, height: 21600 };
const DEFAULT_BACKGROUND: Background = { paper: 'white', template: 'blank', texture: false };

/** Points waiting for a frame are dropped beyond this (no canvas is painting). */
const MAX_UNPAINTED = 5000;
/** Local views sent and not yet echoed are forgotten beyond this. */
const MAX_IN_FLIGHT = 64;
/** At most one view sent to Rust per this many ms while dragging; the last one always goes. */
export const VIEW_SEND_MS = 32;

function inkPoint(p: LivePoint): InkPoint {
  return { x: p.x, y: p.y, pressure: p.p, t: p.t };
}

function sameColor(a: Rgba, b: Rgba): boolean {
  return a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
}

export function sameStyle(a: LiveStyle, b: LiveStyle): boolean {
  return a.tool === b.tool && a.width === b.width && sameColor(a.color, b.color);
}

function sameStart(a: Stroke, b: Stroke): boolean {
  const p = a.points[0];
  const q = b.points[0];
  return a.tool === b.tool && a.layer === b.layer && !!p && !!q && p.x === q.x && p.y === q.y && p.t === q.t;
}

/** Same strokes, compared cheaply (style, sample count, first and last sample). */
function sameStrokes(a: readonly Stroke[], b: readonly Stroke[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const s = a[i];
    const t = b[i];
    if (s === t) continue;
    if (
      s.tool !== t.tool ||
      s.width !== t.width ||
      s.layer !== t.layer ||
      !sameColor(s.color, t.color) ||
      s.points.length !== t.points.length
    ) {
      return false;
    }
    const n = s.points.length;
    if (n === 0) continue;
    const [p0, q0, p1, q1] = [s.points[0], t.points[0], s.points[n - 1], t.points[n - 1]];
    if (p0.x !== q0.x || p0.y !== q0.y || p1.x !== q1.x || p1.y !== q1.y) return false;
  }
  return true;
}

export class LiveState {
  /** A snapshot was applied: the fields below describe the session. */
  ready = $state(false);
  state = $state<LiveSessionState>('idle');
  pageIndex = $state(0);
  pageCount = $state(1);
  /** How the tablet is held now. */
  orientation = $state<Orientation>('landscape');
  /** Layer receiving new strokes. */
  layer = $state(0);
  style = $state.raw<LiveStyle>(DEFAULT_STYLE);
  view = $state.raw<LiveViewState>(DEFAULT_VIEW);
  presentation = $state.raw<MonitorInfo | null>(null);
  saved = $state.raw<string[]>([]);
  /** Pages that could not be written into the library, by page; cleared by the next `saved`. */
  saveErrors = $state.raw<SaveError[]>([]);
  /** Strokes of the page, the one in progress included (`data-strokes`). */
  strokeCount = $state(0);
  /** The page: size, paper, layers (its strokes are `strokes`). */
  noteId = $state('');
  pageSize = $state.raw<PageSize>(DEFAULT_PAGE);
  background = $state.raw<Background>(DEFAULT_BACKGROUND);
  layers = $state.raw<Layer[]>([]);

  /** Finished strokes of the page, in arrival order (appended in place). */
  strokes: Stroke[] = [];
  /** Changes whenever `strokes` is replaced rather than appended to. */
  generation = 0;
  /** The stroke in progress (its points grow in place). */
  current: Stroke | null = null;
  /** Where the pen hovers (page units), or null. */
  hover: { x: number; y: number } | null = null;
  /**
   * The marks written over the presentation, for the page shown (the `marks` events; gone when
   * the page changes). Strokes that did not change keep their object from one list to the next.
   */
  marks: readonly Stroke[] = [];

  private unpainted: PendingSample[] = [];
  private early: { batch: LiveBatch; arrival: number }[] = [];
  private listeners = new Set<() => void>();
  private closed = false;
  /** The last view Rust sent, and the local views on their way to it (oldest first). */
  private confirmedView: LiveViewState = DEFAULT_VIEW;
  private inFlight: LiveViewState[] = [];
  private lastSent: LiveViewState | null = null;
  private sendTimer: ReturnType<typeof setTimeout> | undefined;
  private sendQueued = false;
  private styleInFlight: LiveStyle[] = [];

  /** Calls `listener` after every change; returns the function that stops it. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    for (const listener of [...this.listeners]) listener();
  }

  /** The window goes away: later batches are ignored (Rust drops the channel on its own). */
  close(): void {
    this.closed = true;
    clearTimeout(this.sendTimer);
    this.listeners.clear();
    this.early = [];
    this.unpainted = [];
  }

  /** The snapshot returned by `live_subscribe`; batches received before it are applied after. */
  initial(snapshot: LiveSnapshot): void {
    if (this.closed) return;
    this.applySnapshot(snapshot);
    const early = this.early;
    this.early = [];
    for (const { batch, arrival } of early) this.applyEvents(batch.events, arrival);
    this.notify();
  }

  /**
   * The snapshot returned by `live_start`: used only if nothing newer came through the
   * channel (Rust broadcasts the start too).
   */
  adoptStart(snapshot: LiveSnapshot): void {
    if (this.closed || (this.ready && this.state !== 'idle')) return;
    this.initial(snapshot);
  }

  /** A batch from Rust, arrived at `arrival` (wall clock). */
  receive(batch: LiveBatch, arrival = wallClock()): void {
    if (this.closed) return;
    if (!this.ready) {
      this.early.push({ batch, arrival });
      return;
    }
    this.applyEvents(batch.events, arrival);
    this.notify();
  }

  private applyEvents(events: readonly LiveEvent[], arrival: number): void {
    for (const event of events) this.apply(event, arrival);
  }

  private apply(event: LiveEvent, arrival: number): void {
    switch (event.type) {
      case 'snapshot':
        this.applySnapshot(event.snapshot);
        break;
      case 'strokeStart': {
        if (event.pageIndex !== this.pageIndex) break;
        if (this.current) this.finishStroke();
        const { points, ...style } = event.stroke;
        this.current = { ...style, points: points.map(inkPoint) };
        // The pen writes: it no longer hovers (no laser dot left behind).
        this.hover = null;
        this.pending(points, arrival);
        this.strokeCount = this.strokes.length + 1;
        break;
      }
      case 'points': {
        if (event.pageIndex !== this.pageIndex) break;
        if (!this.current) {
          const { tool, color, width } = this.style;
          this.current = { tool, color, width, layer: this.layer, points: [] };
          this.strokeCount = this.strokes.length + 1;
        }
        const points = this.current.points;
        for (const p of event.points) points.push(inkPoint(p));
        this.pending(event.points, arrival);
        break;
      }
      case 'strokeEnd':
        if (event.pageIndex === this.pageIndex && this.current) this.finishStroke();
        break;
      case 'hover':
        this.hover = { x: event.x, y: event.y };
        break;
      case 'hoverEnd':
        this.hover = null;
        break;
      case 'view':
        this.incomingView(event.view);
        break;
      case 'marks':
        this.marks = reuseMarks(this.marks, marksFromWire(event.marks));
        break;
      case 'saved':
        this.mergeSaved(event.paths);
        // A successful save means that everything is written, the failed pages included.
        if (this.saveErrors.length > 0) this.saveErrors = [];
        break;
      case 'saveFailed':
        this.saveErrors = [
          ...this.saveErrors.filter((e) => e.pageIndex !== event.pageIndex),
          { pageIndex: event.pageIndex, code: event.code, message: event.message },
        ].sort((a, b) => a.pageIndex - b.pageIndex);
        break;
    }
  }

  private finishStroke(): void {
    if (this.current && this.current.points.length > 0) this.strokes.push(this.current);
    this.current = null;
    this.strokeCount = this.strokes.length;
  }

  private pending(points: readonly LivePoint[], arrival: number): void {
    for (const p of points) this.unpainted.push({ recv: p.recv, arrival });
    if (this.unpainted.length > MAX_UNPAINTED) this.unpainted.splice(0, this.unpainted.length - MAX_UNPAINTED);
  }

  /** The points received since the last call: the frame that just drew them takes them. */
  takeUnpainted(): PendingSample[] {
    const out = this.unpainted;
    this.unpainted = [];
    return out;
  }

  private mergeSaved(paths: readonly string[]): void {
    const missing = paths.filter((p) => !this.saved.includes(p));
    if (missing.length > 0) this.saved = [...this.saved, ...missing];
  }

  private applySnapshot(s: LiveSnapshot): void {
    const samePage = this.ready && s.pageIndex === this.pageIndex && s.page.id === this.noteId;
    let incoming = s.page.strokes;
    if (!samePage) {
      this.current = null;
      this.marks = [];
    } else if (this.current && incoming.length > 0 && sameStart(incoming[incoming.length - 1], this.current)) {
      // The stroke in progress is already in the page: it stays the one drawn as in progress.
      incoming = incoming.slice(0, -1);
    }
    if (!samePage || !sameStrokes(this.strokes, incoming)) {
      this.strokes = [...incoming];
      this.generation += 1;
    }
    this.ready = true;
    this.state = s.state;
    this.pageIndex = s.pageIndex;
    this.pageCount = Math.max(1, s.pageCount);
    this.orientation = s.orientation;
    this.layer = s.layer;
    this.noteId = s.page.id;
    if (this.pageSize.width !== s.page.page.width || this.pageSize.height !== s.page.page.height) {
      this.pageSize = s.page.page;
    }
    if (JSON.stringify(this.background) !== JSON.stringify(s.page.background)) this.background = s.page.background;
    if (JSON.stringify(this.layers) !== JSON.stringify(s.page.layers)) this.layers = s.page.layers;
    this.incomingStyle(s.style);
    this.incomingView(s.view);
    this.hover = s.hover;
    if (JSON.stringify(this.presentation) !== JSON.stringify(s.presentation)) this.presentation = s.presentation;
    this.mergeSaved(s.saved);
    this.strokeCount = this.strokes.length + (this.current ? 1 : 0);
  }

  // --- Changes made in this window ------------------------------------------------------------
  //
  // They show at once; Rust broadcasts them back to every window. An echo of an older local
  // change (the user is still dragging, or clicked twice) must not undo a newer one.

  /**
   * Shows `view` now and sends it with `send` (at most one per `VIEW_SEND_MS`; the last one is
   * always sent).
   */
  changeView(view: LiveViewState, send: (view: LiveViewState) => Promise<unknown>): void {
    if (this.closed) return;
    this.view = roundView(view);
    this.notify();
    if (this.sendTimer !== undefined) {
      this.sendQueued = true;
      return;
    }
    this.sendView(send);
  }

  private sendView(send: (view: LiveViewState) => Promise<unknown>): void {
    const view = this.view;
    this.lastSent = view;
    this.inFlight.push(view);
    if (this.inFlight.length > MAX_IN_FLIGHT) this.inFlight.shift();
    send(view).catch(() => this.viewFailed(view));
    this.sendTimer = setTimeout(() => {
      this.sendTimer = undefined;
      if (this.sendQueued) {
        this.sendQueued = false;
        if (!this.lastSent || !sameView(this.view, this.lastSent)) this.sendView(send);
      }
    }, VIEW_SEND_MS);
  }

  private viewFailed(view: LiveViewState): void {
    const i = this.inFlight.indexOf(view);
    if (i >= 0) this.inFlight.splice(i, 1);
    if (this.inFlight.length === 0 && !this.closed) {
      // Rust kept its view: show it again.
      this.view = this.confirmedView;
      this.lastSent = null;
      this.notify();
    }
  }

  private incomingView(view: LiveViewState): void {
    this.confirmedView = view;
    const i = this.inFlight.findIndex((v) => sameView(v, view));
    if (i >= 0) this.inFlight.splice(0, i + 1);
    // A newer local view is on its way (sent, or waiting to be sent): keep showing it.
    const localAhead = this.lastSent !== null && !sameView(this.view, this.lastSent);
    if (this.inFlight.length > 0 || localAhead) return;
    if (!sameView(this.view, view)) this.view = view;
  }

  /** Shows `style` now and sends it; a failure shows Rust's style again. */
  changeStyle(style: LiveStyle, send: (style: LiveStyle) => Promise<unknown>): Promise<void> {
    if (this.closed) return Promise.resolve();
    const previous = this.style;
    this.style = style;
    this.styleInFlight.push(style);
    if (this.styleInFlight.length > MAX_IN_FLIGHT) this.styleInFlight.shift();
    return send(style).then(
      () => undefined,
      (error: unknown) => {
        const i = this.styleInFlight.indexOf(style);
        if (i >= 0) this.styleInFlight.splice(i, 1);
        if (this.styleInFlight.length === 0) this.style = previous;
        throw error;
      },
    );
  }

  /** Shows `style` while a slider moves, without sending it. */
  previewStyle(style: LiveStyle): void {
    this.style = style;
  }

  private incomingStyle(style: LiveStyle): void {
    const i = this.styleInFlight.findIndex((s) => sameStyle(s, style));
    if (i >= 0) this.styleInFlight.splice(0, i + 1);
    if (this.styleInFlight.length > 0) return;
    if (!sameStyle(this.style, style)) this.style = style;
  }
}

/**
 * Subscribes `live` to the session (`api.liveSubscribe`, passed in so that this module stays
 * free of the desktop runtime): its snapshot, then every batch.
 */
export async function subscribeLive(
  live: LiveState,
  subscribe: (onBatch: (batch: LiveBatch) => void) => Promise<LiveSnapshot>,
): Promise<void> {
  const snapshot = await subscribe((batch) => live.receive(batch));
  live.initial(snapshot);
}
