/**
 * The time of what is written in the editor while a narration is recorded (« Pendant que j'écris », milestone M17;
 * docs/design/paper-projection-narration.md §3.2): note time `Point::t`, milliseconds since `Note::created`.
 *
 * - The recording starts after the ink the note already holds: its origin `T0` is the last sample of the note, or the
 *   time of its last page object (a text frame), hidden layers included, plus `WRITING_GAP_MS`, or 0 for a note without
 *   either (`writingOrigin`).
 * - Once `narration_start` has answered, a `WritingClock` maps a pointer event's `timeStamp` (the page's monotonic
 *   clock, the one of `performance.now()`) to note time: `T0 + (timeStamp − P0)`. Rust places the sound with
 *   `noteZeroWallMs`, the wall clock at which note time would be 0 (`narration_stop`'s fourth argument): a stroke drawn
 *   and the sound heard at the same wall instant then share their note time.
 * - After a recording, a new stroke goes after the end of a note-time narration too (`nextStrokeBase`, O14), so that
 *   it is not replayed during the sound; a replay narration (in replay time) keeps the editor's rule.
 */
import type { Note } from '../note';
import type { NarrationInfo } from '../video';

/**
 * Note time between the last sample of the note and the start of a writing recording, ms: live mode's resume uses the
 * same gap (`RESUME_GAP_MS`, `app/src-tauri/src/live/resume.rs`).
 */
export const WRITING_GAP_MS = 1000;

/** Note time between the end of a note and a stroke drawn in the editor outside a recording, ms (the editor's rule). */
export const STROKE_GAP_MS = 500;

/** The time of the last sample of the note, or of its last page object (a text frame appears at its time; a bookmark is not part of the writing), every layer counted; -1 without any. */
export function lastSampleTime(note: Note): number {
  let last = -1;
  for (const stroke of note.strokes) for (const p of stroke.points) if (p.t > last) last = p.t;
  for (const object of note.objects ?? []) if (object.kind !== 'bookmark' && object.t > last) last = object.t;
  return last;
}

/** Note time at which a writing recording starts: after the note's ink (hidden layers included), or 0 without ink. */
export function writingOrigin(note: Note): number {
  const last = lastSampleTime(note);
  return last < 0 ? 0 : last + WRITING_GAP_MS;
}

/** The note time at which a narration in note time ends; null for a replay narration (or none). */
export function noteSoundEnd(narration: NarrationInfo | null | undefined): number | null {
  return narration && narration.base === 'note' ? narration.startMs + narration.durationMs : null;
}

/**
 * The note time of the first sample of a stroke drawn now, outside a recording: `STROKE_GAP_MS` after the note's last
 * sample, and after the end of its narration when that one is in note time (O14). Without such a narration it is
 * exactly the editor's rule of before (`lastTime + 500`, 499 on an empty page).
 */
export function nextStrokeBase(note: Note, narration: NarrationInfo | null | undefined): number {
  const end = noteSoundEnd(narration);
  return Math.max(lastSampleTime(note), end ?? -Infinity) + STROKE_GAP_MS;
}

/** What `Canvas` asks of a clock: the note time of a pointer event. */
export interface StrokeClock {
  noteTime(timeStamp: number): number;
}

/** The clock of one writing recording, from the moment `narration_start` answered. */
export class WritingClock implements StrokeClock {
  /** `T0`: the note time of the start. */
  readonly origin: number;
  /** `P0`: `performance.now()` at the start (the clock of the pointer events' `timeStamp`). */
  readonly startedAt: number;
  /** The wall clock (Unix ms) at which note time would be 0: `narration_stop`'s `replayStartedAtMs`. */
  readonly noteZeroWallMs: number;

  /**
   * `wallAtStart`: `Date.now()` at the same moment as `startedAt`. Rust stamps the sound's first sample with the wall
   * clock (`SystemTime`), so the wall anchor is read from the wall clock at the start rather than derived from
   * `performance.timeOrigin`, which is fixed when the page loads and drifts from the wall clock in a window left open
   * for hours (sleep, clock adjustments); over a recording both clocks run at the same rate.
   */
  constructor(origin: number, startedAt: number, wallAtStart: number = Date.now()) {
    this.origin = origin;
    this.startedAt = startedAt;
    this.noteZeroWallMs = wallAtStart - origin;
  }

  /** The note time of a pointer event's `timeStamp` (or of `performance.now()`). */
  noteTime(timeStamp: number): number {
    return this.origin + (timeStamp - this.startedAt);
  }
}
