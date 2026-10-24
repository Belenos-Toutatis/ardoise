/**
 * The player of the editor's replay: a position in replay time (milliseconds), play and pause on
 * the animation frames, the options (speed, trimmed pauses) and the note as drawn at the position.
 * The mapping itself is `lib/replay.ts`; this class only moves the playhead. The clock is
 * injected so that tests can drive it.
 */
import { DEFAULT_REPLAY, ReplayTimeline, type ReplayOptions } from '../../lib/replay';
import type { Note } from '../../lib/note';
import { MAX_SPEED, MIN_SPEED, TRIMMED_PAUSE_MS } from '../../lib/video';

/** Time and animation frames. */
export interface FrameClock {
  /** Milliseconds, monotonic. */
  now(): number;
  /** Calls `callback` at the next frame; returns a handle for `cancel`. */
  request(callback: () => void): number;
  cancel(handle: number): void;
}

export const browserClock: FrameClock = {
  now: () => performance.now(),
  request: (callback) => requestAnimationFrame(() => callback()),
  cancel: (handle) => cancelAnimationFrame(handle),
};

/** A speed the timeline accepts. */
export function clampSpeed(speed: number): number {
  return Number.isFinite(speed) ? Math.min(MAX_SPEED, Math.max(MIN_SPEED, speed)) : 1;
}

export class ReplayPlayer {
  /** The note replayed (the editor's note when the replay was entered). */
  note = $state.raw<Note | null>(null);
  options = $state.raw<ReplayOptions>(DEFAULT_REPLAY);
  /** The replay goes on until at least this instant (a narration longer than the drawing), ms. */
  extent = $state(0);
  position = $state(0);
  playing = $state(false);

  readonly timeline = $derived(this.note ? new ReplayTimeline(this.note, this.options) : null);
  /** Length of the whole replay: the drawing, or the narration if it is longer. */
  readonly end = $derived(Math.max(this.timeline?.duration ?? 0, this.extent));
  /** The note as drawn at the position (null before a note is loaded). */
  readonly shown = $derived(this.timeline ? this.timeline.noteAt(this.position) : null);
  readonly trimmed = $derived(this.options.maxPauseMs !== null);

  readonly #clock: FrameClock;
  #handle: number | null = null;
  #last = 0;

  constructor(clock: FrameClock = browserClock) {
    this.#clock = clock;
  }

  /** Replays `note` from the start, keeping the options. */
  load(note: Note): void {
    this.pause();
    this.note = note;
    this.extent = 0;
    this.position = 0;
  }

  /** Plays from the position; from the start when it is at the end. */
  play(): void {
    if (this.playing || this.end <= 0) return;
    if (this.position >= this.end) this.position = 0;
    this.playing = true;
    this.#last = this.#clock.now();
    this.#handle = this.#clock.request(this.#frame);
  }

  pause(): void {
    this.playing = false;
    if (this.#handle !== null) this.#clock.cancel(this.#handle);
    this.#handle = null;
  }

  toggle(): void {
    if (this.playing) this.pause();
    else this.play();
  }

  /** Moves the playhead (clamped); playing goes on from there. */
  seek(ms: number): void {
    this.position = Math.min(this.end, Math.max(0, Number.isFinite(ms) ? ms : 0));
  }

  /**
   * New options. The playhead stays on the same moment of the note (what is on the page does not
   * jump), or at the end if it was there.
   */
  setOptions(next: ReplayOptions): void {
    const options = { speed: clampSpeed(next.speed), maxPauseMs: next.maxPauseMs };
    const before = this.timeline;
    if (!before) {
      this.options = options;
      return;
    }
    const atEnd = this.position >= before.duration;
    const moment = before.toNote(this.position);
    this.options = options;
    // `timeline` is derived: read it again after the options changed.
    const after = this.timeline;
    if (after && !atEnd) this.position = Math.min(this.end, after.toReplay(moment));
    else if (after) this.position = Math.min(this.end, Math.max(this.position, after.duration));
  }

  setSpeed(speed: number): void {
    this.setOptions({ ...this.options, speed });
  }

  /** "Supprimer les temps morts": pauses longer than `TRIMMED_PAUSE_MS` are cut. */
  setTrim(on: boolean): void {
    this.setOptions({ ...this.options, maxPauseMs: on ? TRIMMED_PAUSE_MS : null });
  }

  /** Stops the animation (the editor goes away). */
  dispose(): void {
    this.pause();
  }

  readonly #frame = (): void => {
    this.#handle = null;
    if (!this.playing) return;
    const now = this.#clock.now();
    const next = this.position + Math.max(0, now - this.#last);
    this.#last = now;
    if (next >= this.end) {
      this.position = this.end;
      this.playing = false;
      return;
    }
    this.position = next;
    this.#handle = this.#clock.request(this.#frame);
  };
}
