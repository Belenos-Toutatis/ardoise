/**
 * Sends the marks written over the presentation to Rust (`live_set_marks`), which shows them on
 * the viewers of the share. The presentation window keeps drawing its own marks; this only
 * reports them: the whole list at most every `MARKS_SEND_MS` while a mark is being written (a
 * viewer sees it grow), at once when the list becomes empty (page change, clearing), one message
 * at a time so that they arrive in order. Pure of the interface: the command is passed in.
 */
import type { LiveMark } from './live';
import { marksToWire } from './live-marks';
import type { Stroke } from './note';

/** Least time between two lists sent. */
export const MARKS_SEND_MS = 50;

export type SendMarks = (pageIndex: number, marks: LiveMark[]) => Promise<unknown>;

interface Pending {
  pageIndex: number;
  marks: readonly Stroke[];
  current: Stroke | null;
}

export class MarksSender {
  private pending: Pending | null = null;
  private busy = false;
  private lastAt = Number.NEGATIVE_INFINITY;
  private timer: ReturnType<typeof setTimeout> | undefined;
  /** Rust holds marks of the page `lastPage` as far as this sender knows. */
  private shared = false;
  private lastPage = 0;
  private closed = false;

  constructor(
    private readonly send: SendMarks,
    private readonly interval = MARKS_SEND_MS,
  ) {}

  /**
   * The window's marks on page `pageIndex` are now `marks`, and `current` is being written
   * (its points grow in place: they are read when the list is sent).
   */
  update(pageIndex: number, marks: readonly Stroke[], current: Stroke | null): void {
    if (this.closed) return;
    const empty = marks.length === 0 && !current;
    if (empty && !this.shared) {
      // Rust has none: nothing to say, and what waits for its turn is out of date.
      this.pending = null;
      clearTimeout(this.timer);
      this.timer = undefined;
      return;
    }
    this.pending = { pageIndex, marks, current };
    if (empty) {
      clearTimeout(this.timer);
      this.timer = undefined;
    }
    this.pump();
  }

  /** The window goes away: its marks go with it. */
  close(): void {
    if (this.closed) return;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.pending = null;
    if (this.shared) this.transmit(this.lastPage, []);
    this.closed = true;
  }

  private pump(): void {
    const pending = this.pending;
    if (!pending || this.busy || this.timer !== undefined) return;
    const empty = pending.marks.length === 0 && !pending.current;
    const wait = empty ? 0 : this.lastAt + this.interval - Date.now();
    if (wait > 0) {
      this.timer = setTimeout(() => {
        this.timer = undefined;
        this.pump();
      }, wait);
      return;
    }
    this.pending = null;
    const list = pending.current ? [...pending.marks, pending.current] : pending.marks;
    this.shared = list.length > 0;
    this.lastPage = pending.pageIndex;
    this.busy = true;
    this.lastAt = Date.now();
    this.transmit(pending.pageIndex, marksToWire(list)).finally(() => {
      this.busy = false;
      this.pump();
    });
  }

  /** Sends, never failing: a viewer missing a mark is no reason to bother the teacher. */
  private transmit(pageIndex: number, marks: LiveMark[]): Promise<void> {
    try {
      return Promise.resolve(this.send(pageIndex, marks)).then(
        () => undefined,
        () => undefined,
      );
    } catch {
      return Promise.resolve();
    }
  }
}
