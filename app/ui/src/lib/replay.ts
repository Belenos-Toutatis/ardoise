/**
 * The replay of a note on screen (SPEC §4.5): a TypeScript port of `ardoise_media::replay::Timeline`
 * (`crates/ardoise-media/src/replay/timeline.rs`; semantics in docs/design/media.md, "Replay
 * semantics"). Note time is `Point.t`, milliseconds from `Note.created`; replay time is what the
 * viewer sees:
 *
 * 1. only the strokes every export draws count (visible layers; a stroke on a layer without entry
 *    is drawn on top), so the time spent on hidden strokes is a pause like any other;
 * 2. with `maxPauseMs`, every pause longer than it is cut after its first `maxPauseMs` (the time
 *    before the first stroke and the time between busy spans; the pen standing still inside a
 *    stroke is not a pause);
 * 3. the time kept is divided by `speed`.
 *
 * The mapping is piecewise linear: a list of kept stretches of note time. Note time inside a cut
 * maps to the instant of the cut; the other way, a replay instant maps to the latest note time
 * shown, so `toReplay(toNote(r)) == r`.
 */
import { drawingOrder } from './editor/hit';
import type { Note, Stroke } from './note';
import { MAX_SPEED, MIN_SPEED } from './video';

/** How the note is replayed. */
export interface ReplayOptions {
  /** 1 = as written, 2 = twice as fast; `MIN_SPEED` to `MAX_SPEED`. */
  speed: number;
  /** Longest pause kept, note milliseconds (before the speed applies); null keeps every pause. */
  maxPauseMs: number | null;
}

export const DEFAULT_REPLAY: ReplayOptions = { speed: 1, maxPauseMs: null };

/**
 * Slack, in milliseconds of kept note time, when a replay instant falls on the boundary of two
 * kept stretches: the later stretch must win there (as in the Rust timeline, where a round trip
 * through `Duration` may land a few nanoseconds short).
 */
const BOUNDARY_SLACK_MS = 1e-5;

/** A stretch of note time that is kept, mapped linearly to replay time. */
interface Segment {
  noteStart: number;
  noteEnd: number;
  /** Note time kept before this stretch. */
  keptBefore: number;
}

/** When the samples of each stroke appear, and what is kept of the note time (speed aside). */
interface Clock {
  /** Per stroke of the note: when each sample appears (running maximum of the sample times); empty if not drawn. */
  times: number[][];
  /** The strokes drawn, in drawing order (indices into `note.strokes`), all with samples. */
  order: number[];
  segments: Segment[];
  /** Note time of the last sample drawn (0 without any). */
  end: number;
  /** Total note time kept. */
  kept: number;
}

/** When each sample appears: its time, or the time of an earlier sample if that is later. */
function appearanceTimes(stroke: Stroke): number[] {
  let latest = 0;
  return stroke.points.map((p) => {
    latest = Math.max(latest, p.t);
    return latest;
  });
}

function makeClock(note: Note, maxPauseMs: number | null): Clock {
  const times: number[][] = note.strokes.map(() => []);
  const order: number[] = [];
  for (const i of drawingOrder(note)) {
    const slot = appearanceTimes(note.strokes[i]);
    times[i] = slot;
    if (slot.length > 0) order.push(i);
  }

  // When some stroke is being drawn: the union of the strokes' spans.
  const spans: [number, number][] = order.map((i) => [times[i][0], times[i][times[i].length - 1]]);
  spans.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const busy: [number, number][] = [];
  for (const [start, end] of spans) {
    const last = busy[busy.length - 1];
    if (last && start <= last[1]) last[1] = Math.max(last[1], end);
    else busy.push([start, end]);
  }
  const end = busy.length > 0 ? busy[busy.length - 1][1] : 0;

  // The pauses (before the first stroke, between busy spans) cut after `maxPauseMs`.
  const cuts: [number, number][] = [];
  if (maxPauseMs !== null) {
    const cap = Math.max(0, maxPauseMs);
    let idleFrom = 0;
    for (const [start, stop] of busy) {
      if (start - idleFrom > cap) cuts.push([idleFrom + cap, start]);
      idleFrom = stop;
    }
  }
  const segments: Segment[] = [];
  let noteStart = 0;
  let kept = 0;
  for (const [cutStart, cutEnd] of cuts) {
    segments.push({ noteStart, noteEnd: cutStart, keptBefore: kept });
    kept += cutStart - noteStart;
    noteStart = cutEnd;
  }
  segments.push({ noteStart, noteEnd: end, keptBefore: kept });
  kept += end - noteStart;
  return { times, order, segments, end, kept };
}

/** Index of the first element for which `keep` is false (the elements are partitioned). */
function partitionPoint<T>(items: readonly T[], keep: (item: T) => boolean): number {
  let low = 0;
  let high = items.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (keep(items[mid])) low = mid + 1;
    else high = mid;
  }
  return low;
}

/** Whether `speed` can be used: `MIN_SPEED` to `MAX_SPEED`. */
export function validSpeed(speed: number): boolean {
  return speed >= MIN_SPEED && speed <= MAX_SPEED;
}

/**
 * The options that make the replay of `note` last `targetMs` with pauses trimmed to `maxPauseMs`
 * (an export's "durée cible"). The speed stays within `MIN_SPEED` and `MAX_SPEED`, so the replay
 * can end up longer or shorter than asked; a note without any sample gets speed 1.
 */
export function forDuration(note: Note, maxPauseMs: number | null, targetMs: number): ReplayOptions {
  const { kept } = makeClock(note, maxPauseMs);
  let speed: number;
  if (kept <= 0) speed = 1;
  else if (!(targetMs > 0)) speed = MAX_SPEED;
  else speed = Math.min(MAX_SPEED, Math.max(MIN_SPEED, kept / targetMs));
  return { speed, maxPauseMs };
}

/**
 * The timeline of the replay of one note: its duration, replay time ↔ note time, and the note as
 * drawn at any replay instant. Immutable once built; build another when the note or the options
 * change.
 */
export class ReplayTimeline {
  readonly note: Note;
  readonly options: ReplayOptions;
  /** Length of the replay, milliseconds: up to the moment the last sample appears (0 without any). */
  readonly duration: number;
  private readonly clock: Clock;
  /** Partial strokes already cut (by stroke index), so that a still frame reuses its objects. */
  private readonly partial = new Map<number, { shown: number; stroke: Stroke }>();
  private last: { strokes: Stroke[]; note: Note } | null = null;

  /** Throws a `RangeError` when the speed is not within `MIN_SPEED` and `MAX_SPEED`. */
  constructor(note: Note, options: ReplayOptions = DEFAULT_REPLAY) {
    if (!validSpeed(options.speed)) {
      throw new RangeError(`replay speed ${options.speed} is not between ${MIN_SPEED} and ${MAX_SPEED}`);
    }
    this.note = note;
    this.options = options;
    this.clock = makeClock(note, options.maxPauseMs);
    this.duration = this.clock.kept / options.speed;
  }

  /** Note time (ms since `Note.created`) of the last sample drawn. */
  get noteEnd(): number {
    return this.clock.end;
  }

  /** The strokes drawn by the replay (visible layers, with samples): indices into `note.strokes`. */
  get drawingOrder(): readonly number[] {
    return this.clock.order;
  }

  /**
   * The replay instant at which note time `noteMs` is shown (clamped to the note's span; a time
   * inside a cut pause maps to the instant of the cut).
   */
  toReplay(noteMs: number): number {
    const { segments, end } = this.clock;
    const tau = Number.isNaN(noteMs) ? 0 : Math.min(end, Math.max(0, noteMs));
    const index = Math.max(0, partitionPoint(segments, (s) => s.noteStart <= tau) - 1);
    const segment = segments[index];
    if (!segment) return 0;
    const kept = segment.keptBefore + Math.max(0, Math.min(tau, segment.noteEnd) - segment.noteStart);
    return kept / this.options.speed;
  }

  /**
   * The latest note time shown at replay instant `replayMs`: everything written up to it is on
   * screen. From `duration` on, the time of the last sample.
   */
  toNote(replayMs: number): number {
    const { segments, end } = this.clock;
    if (replayMs >= this.duration) return end;
    const kept = (Number.isNaN(replayMs) ? 0 : replayMs) * this.options.speed;
    const index = Math.max(0, partitionPoint(segments, (s) => s.keptBefore <= kept + BOUNDARY_SLACK_MS) - 1);
    const segment = segments[index];
    if (!segment) return 0;
    return Math.min(segment.noteEnd, segment.noteStart + Math.max(0, kept - segment.keptBefore));
  }

  /** How many samples of stroke `index` are shown at note time `noteMs` (0 when not drawn or not begun). */
  samplesShown(index: number, noteMs: number): number {
    const times = this.clock.times[index];
    return times ? partitionPoint(times, (t) => t <= noteMs) : 0;
  }

  /** True when stroke `index` is drawn and all of it is shown at note time `noteMs`. */
  isComplete(index: number, noteMs: number): boolean {
    const times = this.clock.times[index];
    return !!times && times.length > 0 && times[times.length - 1] <= noteMs;
  }

  /**
   * The note as drawn at replay instant `replayMs`: the strokes of visible layers that have begun,
   * in note order, each cut after its last sample shown. A stroke shown in full is the note's own
   * object (the Canvas keeps its outline cached by object); only the strokes in progress are new,
   * and a still frame gets the same objects again. Everything else (page, background, layers…) is
   * the note's.
   */
  noteAt(replayMs: number): Note {
    const tau = this.toNote(replayMs);
    const strokes: Stroke[] = [];
    this.note.strokes.forEach((stroke, i) => {
      const shown = this.samplesShown(i, tau);
      if (shown === 0) return;
      if (shown >= stroke.points.length) {
        this.partial.delete(i);
        strokes.push(stroke);
        return;
      }
      let cut = this.partial.get(i);
      if (cut?.shown !== shown) {
        cut = { shown, stroke: { ...stroke, points: stroke.points.slice(0, shown) } };
        this.partial.set(i, cut);
      }
      strokes.push(cut.stroke);
    });
    const previous = this.last;
    if (previous && previous.strokes.length === strokes.length && previous.strokes.every((s, k) => s === strokes[k])) {
      return previous.note;
    }
    const note: Note = { ...this.note, strokes };
    this.last = { strokes, note };
    return note;
  }
}
