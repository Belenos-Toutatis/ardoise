/**
 * The computer's hand in live mode (SPEC §4.2 « Corriger pendant le direct »; docs/design/tablet-live-classroom.md,
 * "M14", "The window's queue, pen and eraser"): what the control window sends while the teacher writes or
 * erases with the computer's pen or mouse on the chosen layer.
 *
 * - `ComputerQueue`: the window's commands that act on the page, one at a time and in order
 *   (`live_computer_pen`, `live_layers`, `live_edit_strokes`, `live_undo_edit`, `live_redo_edit`). Tauri runs
 *   `async` commands on a pool of threads: two calls in flight could reach the session in either order, and a
 *   Ctrl+Z pressed just after the pen is lifted must reach it after the stroke's `end`. `live_command` (pages,
 *   clearing) stays outside: a clearing or a page change that overtakes the end of a stroke drops or ends it, as
 *   the session says.
 * - `ComputerPen`: one gesture of the Pen tool. `begin` leaves at once, then one call of the gesture at a time:
 *   the samples that come meanwhile go in the next call (at most `MAX_PEN_SAMPLES` a call), `end` with the
 *   last ones, every call still needed being queued when the pen is lifted (a Ctrl+Z comes after the `end`).
 *   The window going away mid-stroke cancels the gesture. The window draws its own stroke (`locals`: no round
 *   trip), and Rust's copy is not drawn meanwhile; the local stroke goes when the session says it is done (`LiveState.lastComputerDone`, the
 *   finished stroke being then in `strokes`), when the page or the note changes, or `LOCAL_GRACE_MS` after
 *   `end` or `cancel` left (an answer lost). A `false` answer or a refusal stops the gesture and drops the local
 *   stroke; `liveEditStale` is silent (the page changed under the pen), other refusals go to `onerror`.
 * - `ComputerEraser`: the pen's eraser end, whole strokes of the chosen layer, whatever the tool. A gesture has a
 *   `group`: each move that touches new strokes sends a delete of that group, so that one Ctrl+Z gives the
 *   whole gesture back. The strokes are kept as objects and their indices read when the call leaves the queue,
 *   with the revision of that moment (a delete moves indices); until the window's list has caught up with the
 *   eraser's own deletes, the next one is made at the revision the session answered, on the list less those
 *   strokes. A stale call is tried again once the window's list moved (at once if it already has), at most
 *   `STALE_RETRIES` times; those retries count as work under way, and an undo or a redo queued drops them.
 *
 * Free of the DOM and of the desktop runtime (`api` is passed in), so that it runs in the tests.
 */
import type { api as Api } from './api';
import type { Vec } from './editor/geometry';
import { onLayer, strokesTouched } from './editor/hit';
import type { InkPoint } from './ink';
import type { ComputerSample, LiveComputerPenInput, LiveStyle } from './live';
import type { Layer, Stroke } from './note';

/** Most samples one `live_computer_pen` call may carry (`live/pen.rs`; more is refused, `liveEditInvalid`). */
export const MAX_PEN_SAMPLES = 4096;
/** How long a local stroke is drawn after its `end` or `cancel` left, should the session's answer be lost. */
export const LOCAL_GRACE_MS = 2000;
/** How often a stale delete of the eraser is tried again (after a change of the session each time). */
export const STALE_RETRIES = 3;
/** How long a stale delete waits for the window's list to catch up before it gives up. */
export const STALE_WAIT_MS = 2000;

/** The commands used (the real `api`, or a stand-in in tests). */
export type ComputerCommands = Pick<typeof Api, 'liveComputerPen' | 'liveEditStrokes'>;

function codeOf(error: unknown): unknown {
  return (error as { code?: unknown } | null)?.code;
}

/** Whether a refusal says that the page changed under the gesture: nothing to tell the user. */
export function isStale(error: unknown): boolean {
  return codeOf(error) === 'liveEditStale';
}

/** The commands of the control window that act on the page, one at a time, in the order they were asked for. */
export class ComputerQueue {
  private tail: Promise<void> = Promise.resolve();
  private count = 0;

  /**
   * Runs `task` once every task asked for before it has settled; its answer (or its error) comes back to the
   * caller only, and never stops the tasks after it.
   */
  run<T>(task: () => Promise<T>): Promise<T> {
    this.count += 1;
    const result = this.tail.then(task);
    this.tail = result.then(
      () => {
        this.count -= 1;
      },
      () => {
        this.count -= 1;
      },
    );
    return result;
  }

  /** Tasks asked for and not settled yet. */
  get pending(): number {
    return this.count;
  }

  /** Settles once the queue is empty, tasks asked for meanwhile included (tests). */
  async idle(): Promise<void> {
    while (this.count > 0) await this.tail;
  }
}

/** Where a gesture is made: the page on screen when it began. */
export interface GesturePlace {
  pageIndex: number;
  noteId: string;
  /** The chosen layer (the computer's): the local stroke is drawn on it. */
  layer: number;
}

/** What the window knows of the session for its local strokes. */
export interface LocalView {
  pageIndex: number;
  noteId: string;
  /** The window's number of the last computer stroke the session ended or dropped. */
  lastComputerDone: number;
}

interface PenGesture {
  id: number;
  place: GesturePlace;
  style: LiveStyle;
  /** Drawn by the window; null once cancelled (Rust's copy is still hidden until the session says it is gone). */
  local: Stroke | null;
  /** Samples not sent yet. */
  pending: ComputerSample[];
  /** `begin` left. */
  begun: boolean;
  /** Calls of the gesture queued and not left yet: each takes what is pending when it leaves. */
  queued: number;
  /** The pen was lifted: `end` goes with the last samples. */
  ending: boolean;
  /** Esc, a lost capture, a second finger: `cancel` goes instead of what is pending. */
  cancelling: boolean;
  /** Nothing more is sent: the session ended it, dropped it, refused it, or `end` or `cancel` left. */
  stopped: boolean;
  /** Refused, answered `false`, or gone with another page: the session will record no step for it. */
  dropped: boolean;
  /** When `end` or `cancel` left (`now()`), for `LOCAL_GRACE_MS`. */
  finishedAt: number | null;
  /** The first sample's `ms`: the local stroke's times count from it. */
  t0: number;
}

export interface PenOptions {
  queue: ComputerQueue;
  commands: Pick<ComputerCommands, 'liveComputerPen'>;
  /** The computer's ink when a gesture begins. */
  style: () => LiveStyle;
  /** A refusal other than `liveEditStale`. */
  onerror: (error: unknown) => void;
  /** A local stroke went (or its time ran out): the canvas paints again. */
  onchange?: () => void;
  /** `performance.now()` (the clock of the samples' `ms`). */
  now?: () => number;
}

function inkPoint(sample: ComputerSample, t0: number): InkPoint {
  return { x: sample.x, y: sample.y, pressure: sample.p, t: Math.max(0, Math.round(sample.ms - t0)) };
}

/** The computer's pen: one gesture at a time, its calls in order through the queue. */
export class ComputerPen {
  private readonly options: PenOptions;
  private readonly now: () => number;
  /** The window's number of the next gesture (a counter from 1). */
  private next = 1;
  /** The gesture of the pointer that is down (null when none is). */
  private current: PenGesture | null = null;
  /** The gestures whose local stroke is drawn, or whose Rust copy is hidden, oldest first. */
  private drawn: PenGesture[] = [];
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(options: PenOptions) {
    this.options = options;
    this.now = options.now ?? (() => performance.now());
  }

  /** A gesture is under way (the pen or the mouse is down on the preview). */
  get active(): boolean {
    return this.current !== null;
  }

  /** Begins a stroke at `sample` on `place`; `begin` leaves at once. */
  down(place: GesturePlace, sample: ComputerSample): void {
    if (this.current) this.cancel();
    const style = this.options.style();
    const gesture: PenGesture = {
      id: this.next++,
      place,
      style,
      local: { tool: style.tool, color: { ...style.color }, width: style.width, layer: place.layer, points: [inkPoint(sample, sample.ms)] },
      pending: [sample],
      begun: false,
      queued: 0,
      ending: false,
      cancelling: false,
      stopped: false,
      dropped: false,
      finishedAt: null,
      t0: sample.ms,
    };
    this.current = gesture;
    this.drawn.push(gesture);
    this.pump(gesture);
  }

  /** More samples of the gesture under way (coalesced events included, already thinned out by the canvas). */
  move(samples: readonly ComputerSample[]): void {
    const gesture = this.current;
    if (!gesture || gesture.stopped || samples.length === 0) return;
    for (const sample of samples) {
      gesture.pending.push(sample);
      gesture.local?.points.push(inkPoint(sample, gesture.t0));
    }
    this.pump(gesture);
  }

  /**
   * The pen was lifted: `end` leaves with the last samples. Every call still needed (the chunks of what is pending,
   * then `end`) is queued now, so that a command asked for after the pen was lifted (a Ctrl+Z) comes after the `end`,
   * even when more than `MAX_PEN_SAMPLES` samples wait (an IPC call that took long).
   */
  up(samples: readonly ComputerSample[] = []): void {
    const gesture = this.current;
    if (!gesture) return;
    this.move(samples);
    this.current = null;
    if (gesture.stopped) return;
    gesture.ending = true;
    // `begin` (if it has not left) takes a first chunk; the rest goes in chunks, the last one with `end`.
    const left = gesture.begun ? gesture.pending.length : Math.max(0, gesture.pending.length - MAX_PEN_SAMPLES);
    const needed = (gesture.begun ? 0 : 1) + Math.max(1, Math.ceil(left / MAX_PEN_SAMPLES));
    for (let n = gesture.queued; n < needed; n++) this.enqueue(gesture);
  }

  /** The stroke of a lifted pen whose `end` the session has not recorded yet (Ctrl+Z then undoes it once it has). */
  ending(view: LocalView): boolean {
    return this.drawn.some((gesture) => gesture.ending && !gesture.dropped && view.lastComputerDone < gesture.id);
  }

  /** Drops the gesture under way (Esc, `pointercancel`, a lost capture, a second finger, a page change); true if there was one. */
  cancel(): boolean {
    const gesture = this.current;
    if (!gesture) return false;
    this.current = null;
    gesture.local = null;
    this.options.onchange?.();
    if (gesture.stopped) return true;
    gesture.cancelling = true;
    this.pump(gesture);
    return true;
  }

  /**
   * The strokes the window draws instead of Rust's copy: undefined when there is no gesture to show (Rust's copy
   * is then drawn, as in the other windows), else the local strokes (an empty list hides Rust's copy of a
   * cancelled gesture until the session says it is gone). Gestures done, of another page or note, or past their
   * time are forgotten here.
   */
  locals(view: LocalView): readonly Stroke[] | undefined {
    if (this.drawn.length === 0) return undefined;
    const now = this.now();
    this.drawn = this.drawn.filter((gesture) => {
      if (gesture.place.pageIndex !== view.pageIndex || gesture.place.noteId !== view.noteId) {
        // The page changed under the pen: the session ended (kept) its stroke there.
        if (gesture === this.current) this.cancel();
        return false;
      }
      if (view.lastComputerDone >= gesture.id) {
        // The session ended or dropped it: the finished stroke is in `strokes` (or gone). Still under way (a
        // limit, a stop): nothing more is sent for it.
        gesture.stopped = true;
        return false;
      }
      return gesture.finishedAt === null || now - gesture.finishedAt < LOCAL_GRACE_MS;
    });
    if (this.drawn.length === 0) return undefined;
    return this.drawn.flatMap((gesture) => (gesture.local ? [gesture.local] : []));
  }

  /**
   * The window goes. A gesture still under way is cancelled, not ended: the pen was not lifted, and the window going
   * away mid-stroke is an interruption like a lost pointer capture, which cancels too; without it the projector and
   * the share would show the half stroke until the next one or a page change. The calls already asked for (the `end`
   * of a lifted pen) still leave. Then every gesture is forgotten.
   */
  dispose(): void {
    if (this.current) this.cancel();
    clearTimeout(this.timer);
    this.current = null;
    this.drawn = [];
  }

  private drop(gesture: PenGesture): void {
    gesture.stopped = true;
    gesture.dropped = true;
    gesture.local = null;
    this.drawn = this.drawn.filter((g) => g !== gesture);
    this.options.onchange?.();
  }

  /** Asks for the gesture's next call, unless one is already queued (it will take what is pending when it leaves). */
  private pump(gesture: PenGesture): void {
    if (gesture.stopped || gesture.queued > 0) return;
    this.enqueue(gesture);
  }

  private enqueue(gesture: PenGesture): void {
    gesture.queued += 1;
    void this.options.queue.run(() => this.call(gesture));
  }

  /** What the gesture sends now, built when the call leaves the queue (its samples and `now` of that moment). */
  private input(gesture: PenGesture): LiveComputerPenInput | null {
    if (gesture.stopped) return null;
    if (gesture.cancelling) {
      gesture.stopped = true;
      gesture.finishedAt = this.now();
      return gesture.begun ? { kind: 'cancel', stroke: gesture.id } : null;
    }
    if (!gesture.begun) {
      gesture.begun = true;
      const samples = gesture.pending.splice(0, MAX_PEN_SAMPLES);
      return { kind: 'begin', stroke: gesture.id, pageIndex: gesture.place.pageIndex, style: gesture.style, samples, now: this.now() };
    }
    if (gesture.pending.length === 0 && !gesture.ending) return null;
    const samples = gesture.pending.splice(0, MAX_PEN_SAMPLES);
    if (gesture.ending && gesture.pending.length === 0) {
      gesture.stopped = true;
      gesture.finishedAt = this.now();
      return { kind: 'end', stroke: gesture.id, samples, now: this.now() };
    }
    return { kind: 'points', stroke: gesture.id, samples, now: this.now() };
  }

  private async call(gesture: PenGesture): Promise<void> {
    gesture.queued -= 1;
    const input = this.input(gesture);
    if (!input) {
      // Cancelled before `begin` left: the session never heard of it.
      if (gesture.cancelling && !gesture.begun) this.drawn = this.drawn.filter((g) => g !== gesture);
      return;
    }
    if (input.kind === 'end' || input.kind === 'cancel') this.wake();
    let goesOn: boolean;
    try {
      goesOn = await this.options.commands.liveComputerPen(input);
    } catch (error) {
      if (input.kind !== 'cancel') this.drop(gesture);
      if (!isStale(error)) this.options.onerror(error);
      return;
    }
    if (input.kind === 'cancel' || input.kind === 'end') {
      // `false` to an `end`: the session had already ended or dropped the stroke; its events say which.
      if (!goesOn && input.kind === 'end') this.drop(gesture);
      return;
    }
    if (!goesOn) {
      // Ended or dropped by the session (page change, quarter turn, clearing, stop, the limit of points).
      this.drop(gesture);
      return;
    }
    if (gesture.pending.length > 0 || gesture.ending || gesture.cancelling) this.pump(gesture);
  }

  /** Paints again once a local stroke's time may have run out. */
  private wake(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.options.onchange?.(), LOCAL_GRACE_MS + 50);
  }
}

/** What the eraser reads of the session (`LiveState`). */
export interface EraserSource {
  readonly strokes: Stroke[];
  readonly layers: Layer[];
  readonly computerLayer: number;
  readonly revision: number;
  readonly pageIndex: number;
  readonly noteId: string;
  /** Calls `listener` after every change; returns the function that stops it. */
  subscribe(listener: () => void): () => void;
}

/** Group numbers of the eraser's gestures, from a value drawn at each load of the window: a window reloaded in the middle
 * of a session must not join its first gesture to the last one of the window before (steps next to each other naming the
 * same group are undone together). Whole `u32` numbers (`live/edits.rs`, `BadGroup`). */
let groupCounter = 1 + Math.floor(Math.random() * 0x7fff_ffff);

/** A new group number. */
export function nextGroup(): number {
  groupCounter = groupCounter >= 0xffff_fffe ? 1 : groupCounter + 1;
  return groupCounter;
}

interface EraserGesture {
  group: number;
  pageIndex: number;
  noteId: string;
  radius: number;
  last: Vec;
  /** The strokes already sent (or on their way): a stroke is deleted once. */
  handled: WeakSet<Stroke>;
}

export interface EraserOptions {
  queue: ComputerQueue;
  commands: Pick<ComputerCommands, 'liveEditStrokes'>;
  live: EraserSource;
  /** A refusal other than `liveEditStale`. */
  onerror: (error: unknown) => void;
  /** Group numbers (tests). */
  group?: () => number;
}

/** The deletes of the eraser the session went through, as far as the window's list has not caught up with them. */
interface EraserChain {
  pageIndex: number;
  noteId: string;
  steps: { before: number; after: number; removed: readonly Stroke[] }[];
}

/** The pen's eraser end on the preview: deletes whole strokes of the chosen layer that it touches. */
export class ComputerEraser {
  private readonly options: EraserOptions;
  private current: EraserGesture | null = null;
  private chain: EraserChain | null = null;
  private retries = 0;
  /** Bumped when an undo or a redo is queued: the retries asked for before it are dropped. */
  private epoch = 0;
  /** Where the eraser end is (page units), for its ring on the preview; null when it is away. */
  ring: Vec | null = null;

  constructor(options: EraserOptions) {
    this.options = options;
  }

  /** A gesture is under way (the eraser end is down). */
  get active(): boolean {
    return this.current !== null;
  }

  /** The eraser end touches the page at `p`; `radius` in page units. */
  down(p: Vec, radius: number): void {
    const { live } = this.options;
    this.current = {
      group: (this.options.group ?? nextGroup)(),
      pageIndex: live.pageIndex,
      noteId: live.noteId,
      radius,
      last: p,
      handled: new WeakSet(),
    };
    this.ring = p;
    this.erase([p]);
  }

  move(p: Vec): void {
    this.ring = p;
    const gesture = this.current;
    if (!gesture) return;
    this.erase([gesture.last, p]);
    gesture.last = p;
  }

  /** The eraser end hovers at `p` (or went away: null): only the ring follows. */
  hover(p: Vec | null): void {
    if (this.current) return;
    this.ring = p;
  }

  /** The gesture ends (what it deleted stays deleted; one Ctrl+Z gives it all back). */
  up(): void {
    this.current = null;
  }

  /** Stops the gesture (a page change, a lost capture, Esc): nothing more is deleted; true if there was one. */
  cancel(): boolean {
    const was = this.current !== null;
    this.current = null;
    this.ring = null;
    return was;
  }

  private erase(path: readonly Vec[]): void {
    const gesture = this.current;
    const { live } = this.options;
    if (!gesture || live.pageIndex !== gesture.pageIndex || live.noteId !== gesture.noteId) return;
    const found = strokesTouched(onLayer(live, live.computerLayer), path, gesture.radius);
    const strokes: Stroke[] = [];
    for (const index of found) {
      const stroke = live.strokes[index];
      if (!stroke || gesture.handled.has(stroke)) continue;
      gesture.handled.add(stroke);
      strokes.push(stroke);
    }
    if (strokes.length > 0) void this.send(gesture, strokes, 0);
  }

  /** Retries of stale deletes not sent yet (waiting for the session's change, or queued): Ctrl+Z waits for them. */
  get retrying(): number {
    return this.retries;
  }

  /**
   * An undo or a redo is about to be queued: the retries not sent yet are dropped. Sent after it, a retry would delete
   * a stroke the undo just gave back, record a new step and empty the redo steps.
   */
  dropRetries(): void {
    this.epoch += 1;
    this.retries = 0;
  }

  /**
   * The revision to send and the strokes to leave out of the window's list. The session answers each delete with its
   * new revision; until the window's list has caught up (its `strokesEdited`), the next delete of the gesture is made
   * at that revision, on the window's list less the strokes the eraser deleted. Anything else moved the revision (an
   * undo, a clearing, another page): the window's own revision and list, as they are.
   */
  private base(): { revision: number; without: ReadonlySet<Stroke> } {
    const { live } = this.options;
    const chain = this.chain;
    const plain = { revision: live.revision, without: new Set<Stroke>() };
    if (!chain || chain.steps.length === 0 || chain.pageIndex !== live.pageIndex || chain.noteId !== live.noteId) {
      this.chain = null;
      return plain;
    }
    const last = chain.steps[chain.steps.length - 1].after;
    const known = chain.steps.some((step) => step.before === live.revision || step.after === live.revision);
    if (live.revision === last || !known) {
      this.chain = null;
      return plain;
    }
    return { revision: last, without: new Set(chain.steps.flatMap((step) => step.removed)) };
  }

  /** A delete went through at `before`: the session is now at `after`, without `removed`. */
  private advance(before: number, after: number, removed: readonly Stroke[]): void {
    const { live } = this.options;
    const chain = this.chain;
    const follows = chain && chain.pageIndex === live.pageIndex && chain.noteId === live.noteId && chain.steps.at(-1)?.after === before;
    if (follows) chain.steps.push({ before, after, removed });
    else this.chain = { pageIndex: live.pageIndex, noteId: live.noteId, steps: [{ before, after, removed }] };
  }

  /**
   * Deletes `strokes` through the queue: their indices and the revision are read when the call leaves it (`base`).
   * `retry`: the epoch of a retry (it is dropped once an undo or a redo was queued since).
   */
  private send(gesture: EraserGesture, strokes: readonly Stroke[], tries: number, retry?: number): Promise<void> {
    const { live, queue, commands } = this.options;
    return queue.run(async () => {
      if (retry !== undefined) {
        if (retry !== this.epoch) return;
        this.retries -= 1;
      }
      if (live.pageIndex !== gesture.pageIndex || live.noteId !== gesture.noteId) return;
      const { revision, without } = this.base();
      const places = new Map<Stroke, number>();
      let place = 0;
      for (const stroke of live.strokes) if (!without.has(stroke)) places.set(stroke, place++);
      const indices: number[] = [];
      const removed: Stroke[] = [];
      for (const stroke of strokes) {
        const index = places.get(stroke);
        // Gone meanwhile (a clearing, an undo), or on a layer hidden since: Rust would refuse the whole delete.
        if (index !== undefined && live.layers[stroke.layer]?.visible !== false) {
          indices.push(index);
          removed.push(stroke);
        }
      }
      if (indices.length === 0) return;
      indices.sort((a, b) => a - b);
      const seen = live.revision;
      try {
        const after = await commands.liveEditStrokes(live.pageIndex, revision, { kind: 'delete', indices, group: gesture.group });
        this.advance(revision, after, removed);
      } catch (error) {
        if (!isStale(error)) {
          this.options.onerror(error);
          return;
        }
        this.chain = null;
        if (tries >= STALE_RETRIES) return;
        // The window's list was behind the session's: the same strokes again once it caught up, at once if it
        // already did meanwhile (the change that would wake the retry may have come already).
        this.retries += 1;
        const epoch = this.epoch;
        const caughtUp = live.revision !== seen ? Promise.resolve(true) : this.afterChange();
        void caughtUp.then((changed) => {
          if (epoch !== this.epoch) return;
          if (!changed) {
            this.retries -= 1;
            return;
          }
          void this.send(gesture, strokes, tries + 1, epoch);
        });
      }
    });
  }

  /** True after the next change of the session, false if none came within `STALE_WAIT_MS`. */
  private afterChange(): Promise<boolean> {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        stop();
        resolve(false);
      }, STALE_WAIT_MS);
      const stop = this.options.live.subscribe(() => {
        clearTimeout(timer);
        stop();
        resolve(true);
      });
    });
  }
}

export interface ComputerOptions {
  live: EraserSource & LocalView;
  commands: ComputerCommands;
  /** The computer's ink. */
  style: () => LiveStyle;
  /** A refusal other than `liveEditStale`, from the pen or from the eraser (each says it in its own words). */
  onerror: (error: unknown, from: 'pen' | 'eraser') => void;
  /** Something the canvas draws changed (a local stroke went, the eraser's ring moved). */
  onchange?: () => void;
  now?: () => number;
  group?: () => number;
}

/** The control window's queue, pen and eraser, as `LiveCanvas` and the touch-ups use them. */
export class LiveComputer {
  readonly queue = new ComputerQueue();
  readonly pen: ComputerPen;
  readonly eraser: ComputerEraser;
  private readonly live: LocalView;
  /** Set by the canvas that draws the preview: it paints again. */
  onchange: (() => void) | undefined;

  constructor(options: ComputerOptions) {
    this.live = options.live;
    this.onchange = options.onchange;
    const changed = () => this.onchange?.();
    this.pen = new ComputerPen({
      queue: this.queue,
      commands: options.commands,
      style: options.style,
      onerror: (error) => options.onerror(error, 'pen'),
      onchange: changed,
      now: options.now,
    });
    this.eraser = new ComputerEraser({
      queue: this.queue,
      commands: options.commands,
      live: options.live,
      onerror: (error) => options.onerror(error, 'eraser'),
      group: options.group,
    });
  }

  /**
   * A gesture of the pen or of the eraser is under way, or a stale delete of the eraser waits to be sent again: Ctrl+Z
   * and Ctrl+Y wait for them (a step committed after the undo would be undone by none).
   */
  get busy(): boolean {
    return this.pen.active || this.eraser.active || this.eraser.retrying > 0;
  }

  /** A lifted pen's `end` is on its way: its stroke will be a step to undo, though `canUndo` does not say so yet. */
  get strokeEnding(): boolean {
    return this.pen.ending(this.live);
  }

  /** An undo, a redo or « Undo the clearing » is about to be queued: the eraser's retries not sent yet are dropped. */
  historyQueued(): void {
    this.eraser.dropRetries();
  }

  /** The strokes the preview draws instead of Rust's computer stroke in progress (`ComputerPen.locals`). */
  locals(): readonly Stroke[] | undefined {
    return this.pen.locals(this.live);
  }

  /** Drops the gestures under way (Esc, a page change); true if there was one. */
  cancel(): boolean {
    const pen = this.pen.cancel();
    const eraser = this.eraser.cancel();
    if (eraser) this.onchange?.();
    return pen || eraser;
  }

  dispose(): void {
    this.pen.dispose();
    this.eraser.cancel();
  }
}
