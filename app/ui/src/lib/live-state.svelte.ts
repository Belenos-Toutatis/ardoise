/**
 * The live session as one window sees it (docs/design/tablet-live-classroom.md, "Live
 * session"): a `LiveSnapshot`, then the `LiveBatch`es that follow it. The control window and the
 * presentation window each subscribe (`api.liveSubscribe`) and run this same code.
 *
 * Two kinds of fields:
 * - reactive ones (`$state`) for the interface: session state, page number, style, view,
 *   presentation, saved notes, pages that failed to save, stroke count, whether the page turns
 *   with the tablet and the resumed sheets that went on in a new note (M12: the control window
 *   only; the share's viewers are never sent either, and keep the defaults), the page's own paper and the key of its
 *   image's pixels (M17: `pagePaper`, `imagePixels`; a viewer reads them as the background's paper and no image);
 * - plain ones for the canvases, which are told of every change through `subscribe` and draw
 *   on the next animation frame: the finished strokes of the page (`strokes`, appended in
 *   place; `generation` changes when they are replaced or edited), the strokes in progress, one per
 *   pen (`current`, the tablet's, and `computerCurrent`, M14: the control window's pen or mouse;
 *   their points grow in place), the hovering pen (`hover`), and the points not painted yet
 *   (for the latency, the tablet's and the computer's apart), and the marks written over the
 *   presentation (`marks`, replaced whole by each `marks` event; the viewers of the share draw them,
 *   the windows draw their own).
 *
 * The two pens (M14): events with `pen: 'computer'` concern `computerCurrent`, the others `current`, and
 * a stroke of one pen never ends the other's. `lastComputerDone` is the window's number (`id`) of the
 * last computer stroke the session ended or dropped: the control window draws its own stroke until
 * then (`ComputerPen.locals`). `computerLayer` is the layer the computer acts on (`layer` stays the
 * tablet's); `history` keeps `canUndo` and `canRedo` up to date when a computer stroke is recorded.
 *
 * Assumptions about the Rust side (package B), each handled both ways where it is unclear:
 * - A snapshot's `page.strokes` holds the finished strokes only, and it ends the stroke in
 *   progress here: Rust follows a snapshot with a `strokeStart` carrying the points sent so far
 *   when a stroke is in progress (and with nothing when there is none, as after clearing the
 *   page), so the window never finishes a partial stroke into `strokes`.
 * - (supposé) `points` without a `strokeStart` (subscribed in the middle of a stroke) start a
 *   stroke with the current style and layer.
 * - (supposé) `saved` events may carry every path saved so far or only the new ones: they are
 *   merged.
 * - Batches that arrive before the subscription's snapshot are kept and applied after it
 *   (Tauri may deliver a channel message before the command's answer).
 */
import { BOARD_POSITIONS as ALL_BOARD_POSITIONS } from './board-position';
import type { BoardPosition, TabletKind } from './tablet';
import type { InkPoint } from './ink';
import { wallClock, type PendingSample } from './latency';
import type {
  ImagePixels,
  LiveBatch,
  LiveEvent,
  LivePoint,
  LiveSessionState,
  LiveSnapshot,
  LiveStrokesEdited,
  LiveStyle,
  LiveViewState,
  MonitorInfo,
  SheetFork,
} from './live';
import { applyStrokeEdit, type StrokeEdit } from './live-edits';
import { marksFromWire, reuseMarks } from './live-marks';
import { roundView, sameView } from './live-view';
import type { Orientation } from './orientation';
import type { Background, BackgroundImage, Layer, PageObject, PageSize, Paper, Rgba, Stroke } from './note';

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
/** The last edits kept for the canvases: one that has not painted for longer (a hidden window) redraws everything. */
const MAX_EDIT_LOG = 32;
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
  /**
   * The resume was refused after it was accepted (`refused` event): the tablet that connected cannot write on the sheet.
   * `code` selects `errorCodes.*`; kept until `clearRefused` (the view asks again, or closes).
   */
  refused = $state.raw<{ code: string; message: string } | null>(null);
  state = $state<LiveSessionState>('idle');
  /** The kind of tablet the session writes with (`LiveSnapshot.tablet`): null until its tablet first connects. */
  tablet = $state<TabletKind | null>(null);
  /** How the Xiaomi blackboard is held in its run (`LiveSnapshot.boardPosition`): null for the Slate and before it streams. */
  boardPosition = $state<BoardPosition | null>(null);
  /** The positions the blackboard may be switched to now (`LiveSnapshot.boardPositions`; all four when absent). */
  boardPositions = $state.raw<readonly BoardPosition[]>(ALL_BOARD_POSITIONS);
  pageIndex = $state(0);
  pageCount = $state(1);
  /** How the tablet is held now. */
  orientation = $state<Orientation>('landscape');
  /** The layer the tablet writes on (M14: it may be a hidden one). */
  layer = $state(0);
  /**
   * The layer the computer's pen, its eraser and the selection act on (M14, the control window's chosen
   * layer). Absent from a snapshot (the share's viewers): the tablet's.
   */
  computerLayer = $state(0);
  style = $state.raw<LiveStyle>(DEFAULT_STYLE);
  view = $state.raw<LiveViewState>(DEFAULT_VIEW);
  presentation = $state.raw<MonitorInfo | null>(null);
  saved = $state.raw<string[]>([]);
  /** Pages that could not be written into the library, by page; cleared by the next `saved`. */
  saveErrors = $state.raw<SaveError[]>([]);
  /** Strokes of the page, those in progress included (`data-strokes`). */
  strokeCount = $state(0);
  /** The page: size, paper, layers (its strokes are `strokes`). */
  noteId = $state('');
  pageSize = $state.raw<PageSize>(DEFAULT_PAGE);
  background = $state.raw<Background>(DEFAULT_BACKGROUND);
  /** The background image of a resumed sheet: where it goes (its pixels are Rust's). Null elsewhere. */
  pageImage = $state.raw<BackgroundImage | null>(null);
  /**
   * M17: the page's own paper, what it has without the blackboard (the Page tab shows it; `background.paper` is slate
   * while the board is on). The share's viewers do not get it: `background.paper`.
   */
  pagePaper = $state.raw<Paper>('white');
  /** M17: the key and size of the pixels of the page's image (the windows key their paper on it); null without an image. */
  imagePixels = $state.raw<ImagePixels | null>(null);
  /** The share's viewers: the picture of the paper this page wants (`LiveSnapshot.paperKey`). */
  paperKey = $state<string | null>(null);
  layers = $state.raw<Layer[]>([]);
  /** The page's objects (text frames of a resumed sheet or of a projected note), drawn with their layers; never edited here. */
  objects = $state.raw<PageObject[]>([]);
  /**
   * What the indices of `strokes` mean: `api.liveEditStrokes` gives it back, and a selection made
   * at another revision is to be dropped. Not changed by the tablet's writing: a selection stays
   * valid while it writes. Stays 0 on the share's viewers, which edit nothing.
   */
  revision = $state(0);
  /** The page has an edit to undo (`api.liveUndoEdit`), or to redo (`api.liveRedoEdit`). */
  canUndo = $state(false);
  canRedo = $state(false);
  /** The current page turns with the tablet; false on a resumed sheet, which is never turned, and on a page written over an image. */
  canTurnPage = $state(true);
  /** The sheets that went on in a new note, in the order they were told; each stays until dismissed. */
  forks = $state.raw<SheetFork[]>([]);

  /** Finished strokes of the page, in arrival order (appended in place). */
  strokes: Stroke[] = [];
  /** Changes whenever `strokes` is replaced or edited rather than appended to. */
  generation = 0;
  /**
   * What the last edits touched, oldest first. A change of `generation` absent from it (the strokes were
   * replaced) tells a canvas that nothing but a full redraw is right.
   */
  edits: StrokeEdit[] = [];
  /** The tablet's stroke in progress (its points grow in place). */
  current: Stroke | null = null;
  /** The computer's stroke in progress, as the session sends it (its points grow in place). */
  computerCurrent: Stroke | null = null;
  /** The `id` of the last computer stroke the session ended (`strokeEnd`) or dropped (`strokeCancel`); 0 before any. */
  lastComputerDone = 0;
  /** Where the pen hovers (page units), or null. */
  hover: { x: number; y: number } | null = null;
  /**
   * The marks written over the presentation, for the page shown (the `marks` events; gone when
   * the page changes). Strokes that did not change keep their object from one list to the next.
   */
  marks: readonly Stroke[] = [];

  private unpainted: PendingSample[] = [];
  private unpaintedComputer: PendingSample[] = [];
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
  /** The forks told so far (`original|path`), dismissed or not: each is shown once. */
  private forksTold = new Set<string>();

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
    this.unpaintedComputer = [];
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
        const { points, ...style } = event.stroke;
        const stroke = { ...style, points: points.map(inkPoint) };
        if (event.pen === 'computer') {
          // A `begin` while a stroke is in progress ends that one first (kept).
          if (this.computerCurrent) this.finishComputer();
          this.computerCurrent = stroke;
          this.pending(points, arrival, true);
        } else {
          if (this.current) this.finishStroke();
          this.current = stroke;
          // The pen writes: it no longer hovers (no laser dot left behind).
          this.hover = null;
          this.pending(points, arrival);
        }
        this.countStrokes();
        break;
      }
      case 'points': {
        if (event.pageIndex !== this.pageIndex) break;
        const computer = event.pen === 'computer';
        let stroke = computer ? this.computerCurrent : this.current;
        if (!stroke) {
          // (supposé) Subscribed in the middle of a stroke: it goes on in the tablet's style, on its pen's layer.
          const { tool, color, width } = this.style;
          stroke = { tool, color, width, layer: computer ? this.computerLayer : this.layer, points: [] };
          if (computer) this.computerCurrent = stroke;
          else this.current = stroke;
          this.countStrokes();
        }
        const points = stroke.points;
        for (const p of event.points) points.push(inkPoint(p));
        this.pending(event.points, arrival, computer);
        break;
      }
      case 'strokeEnd':
        if (event.pen === 'computer') {
          if (event.pageIndex === this.pageIndex && this.computerCurrent) this.finishComputer();
          if (event.id !== undefined) this.lastComputerDone = event.id;
        } else if (event.pageIndex === this.pageIndex && this.current) {
          this.finishStroke();
        }
        break;
      case 'strokeCancel':
        // Sent for the computer (its `cancel`, « Clear the page »); a tablet's one drops the tablet's stroke.
        if (event.pen === 'computer') {
          if (event.pageIndex === this.pageIndex) this.computerCurrent = null;
          if (event.id !== undefined) this.lastComputerDone = event.id;
        } else if (event.pageIndex === this.pageIndex) {
          this.current = null;
        }
        this.countStrokes();
        break;
      case 'history':
        if (event.pageIndex === this.pageIndex) {
          this.canUndo = event.canUndo;
          this.canRedo = event.canRedo;
        }
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
      case 'strokesEdited':
        if (event.pageIndex === this.pageIndex) this.applyEdit(event);
        break;
      case 'sheetForked':
        this.addFork(event);
        break;
      case 'refused':
        this.refused = { code: event.code, message: event.message };
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

  /**
   * The list is changed in place, a pass over it: a stroke that is not named keeps its object, and
   * so its outline in the renderer; a recoloured one too (an outline does not depend on the colour:
   * `Ink.contours`). The new `generation`, with its entry in `edits`, makes the renderer repaint the area
   * of the strokes touched.
   */
  private applyEdit(edit: LiveStrokesEdited): void {
    const touched = applyStrokeEdit(this.strokes, edit);
    this.generation += 1;
    this.edits.push({ generation: this.generation, touched });
    if (this.edits.length > MAX_EDIT_LOG) this.edits.shift();
    this.countStrokes();
    if (edit.revision !== undefined) this.revision = edit.revision;
    if (edit.canUndo !== undefined) this.canUndo = edit.canUndo;
    if (edit.canRedo !== undefined) this.canRedo = edit.canRedo;
  }

  /** Keeps a fork once, even if it is told twice (and not again after it was dismissed); true when it is new. */
  private addFork({ pageIndex, reason, original, path }: SheetFork): boolean {
    const key = `${original}|${path}`;
    if (this.forksTold.has(key)) return false;
    this.forksTold.add(key);
    this.forks = [...this.forks, { pageIndex, reason, original, path }];
    return true;
  }

  /**
   * The forks `live_stop` answers with (the last write may have made one that no event told this
   * window): returns those this window had not been told yet, which are kept like the others.
   */
  adoptForks(forks: readonly SheetFork[]): SheetFork[] {
    return forks.filter((fork) => this.addFork(fork));
  }

  /** Forgets a refusal (the view starts again). */
  clearRefused(): void {
    this.refused = null;
  }

  /** Forgets the notice of a fork (the user dismissed it). */
  dismissFork(fork: SheetFork): void {
    this.forks = this.forks.filter((f) => f !== fork);
  }

  /** The tablet's stroke in progress joins the page (at the tail, as in the session). */
  private finishStroke(): void {
    if (this.current && this.current.points.length > 0) this.strokes.push(this.current);
    this.current = null;
    this.countStrokes();
  }

  /** The computer's stroke in progress joins the page, likewise. */
  private finishComputer(): void {
    if (this.computerCurrent && this.computerCurrent.points.length > 0) this.strokes.push(this.computerCurrent);
    this.computerCurrent = null;
    this.countStrokes();
  }

  private countStrokes(): void {
    this.strokeCount = this.strokes.length + (this.current ? 1 : 0) + (this.computerCurrent ? 1 : 0);
  }

  private pending(points: readonly LivePoint[], arrival: number, computer = false): void {
    const list = computer ? this.unpaintedComputer : this.unpainted;
    for (const p of points) list.push({ recv: p.recv, arrival });
    if (list.length > MAX_UNPAINTED) list.splice(0, list.length - MAX_UNPAINTED);
  }

  /**
   * The tablet's points received since the last call: the frame that just drew them takes them (the control
   * window's latency figure is the tablet's).
   */
  takeUnpainted(): PendingSample[] {
    const out = this.unpainted;
    this.unpainted = [];
    return out;
  }

  /** The computer's points received since the last call, likewise (the presentation measures their latency apart). */
  takeUnpaintedComputer(): PendingSample[] {
    const out = this.unpaintedComputer;
    this.unpaintedComputer = [];
    return out;
  }

  private mergeSaved(paths: readonly string[]): void {
    const missing = paths.filter((p) => !this.saved.includes(p));
    if (missing.length > 0) this.saved = [...this.saved, ...missing];
  }

  private applySnapshot(s: LiveSnapshot): void {
    const samePage = this.ready && s.pageIndex === this.pageIndex && s.page.id === this.noteId;
    // The strokes in progress are not in the page; the `strokeStart`s that follow bring them back
    // (the tablet's first). Kept here, those events would finish them as partial strokes of the page.
    this.current = null;
    this.computerCurrent = null;
    if (!samePage) this.marks = [];
    if (!samePage || !sameStrokes(this.strokes, s.page.strokes)) {
      this.strokes = [...s.page.strokes];
      this.generation += 1;
      this.edits = [];
    }
    this.ready = true;
    this.state = s.state;
    this.tablet = s.tablet ?? null;
    this.boardPosition = s.boardPosition ?? null;
    const positions = s.boardPositions ?? ALL_BOARD_POSITIONS;
    if (positions.join() !== this.boardPositions.join()) this.boardPositions = positions;
    this.pageIndex = s.pageIndex;
    this.pageCount = Math.max(1, s.pageCount);
    this.orientation = s.orientation;
    this.layer = s.layer;
    this.computerLayer = s.computerLayer ?? s.layer;
    this.noteId = s.page.id;
    if (this.pageSize.width !== s.page.page.width || this.pageSize.height !== s.page.page.height) {
      this.pageSize = s.page.page;
    }
    if (JSON.stringify(this.background) !== JSON.stringify(s.page.background)) this.background = s.page.background;
    const image = s.page.backgroundImage ?? null;
    if (JSON.stringify(this.pageImage) !== JSON.stringify(image)) this.pageImage = image;
    const ownPaper = s.pagePaper ?? s.page.background.paper;
    if (JSON.stringify(this.pagePaper) !== JSON.stringify(ownPaper)) this.pagePaper = ownPaper;
    const pixels = s.imagePixels ?? null;
    if (JSON.stringify(this.imagePixels) !== JSON.stringify(pixels)) this.imagePixels = pixels;
    this.paperKey = s.paperKey ?? null;
    if (JSON.stringify(this.layers) !== JSON.stringify(s.page.layers)) this.layers = s.page.layers;
    const objects = s.page.objects ?? [];
    if (JSON.stringify(this.objects) !== JSON.stringify(objects)) this.objects = objects;
    this.incomingStyle(s.style);
    this.incomingView(s.view);
    this.hover = s.hover;
    if (JSON.stringify(this.presentation) !== JSON.stringify(s.presentation)) this.presentation = s.presentation;
    this.mergeSaved(s.saved);
    this.countStrokes();
    this.revision = s.revision ?? 0;
    this.canUndo = s.canUndo ?? false;
    this.canRedo = s.canRedo ?? false;
    // Absent (the share's viewers): the page turns.
    this.canTurnPage = s.canTurnPage !== false;
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
