/**
 * Drafts of the note being edited (M10; docs/design/interface.md, "M10: drafts and session
 * end"). While a note has unsaved changes, its present state is handed to Rust a few seconds
 * after the last change (`draft_write`), which keeps it outside the library, in
 * `%LOCALAPPDATA%\Ardoise\drafts\<copy>\`; Windows ending the session asks for it at once
 * (`session-ending`). A crash, a restart for Windows Update or an empty battery then loses at
 * most those seconds.
 *
 * - `DraftKeeper`: the timing (a write `DRAFT_DELAY_MS` after the last change, no later than
 *   `DRAFT_MAX_DELAY_MS` after the first unwritten one), one operation on the files at a time,
 *   and what Rust is told for Windows' shutdown screen (`set_unsaved_work`).
 * - `recoveryOf`: what to offer when a note is opened and a draft of it exists: restore it over
 *   the note when the note on disk is still the version the edits started from, else only as a
 *   new note (another PC changed it meanwhile, through Nextcloud), never over a newer version.
 */
import type { FileStamp, Note } from '../note';
import { sameStamp } from './disk';

/** A draft as the library screen lists it (`DraftSummary` in `drafts.rs`). */
export interface DraftSummary {
  /** The id of the note in its file: the key of its draft. */
  id: string;
  /** The note file when the draft was written. */
  path: string;
  title: string;
  /** Unix ms. */
  savedAt: number;
}

/** A draft with its note (`Draft` in `drafts.rs`). */
export interface Draft extends DraftSummary {
  /** The stamp of the file the edits started from. */
  baseStamp: FileStamp | null;
  /** A background image was kept with it (`draft_stage_image` stages it again). */
  hasImage: boolean;
  note: Note;
}

/** What the editor hands over. */
export interface DraftSnapshot {
  path: string;
  noteId: string;
  note: Note;
  baseStamp: FileStamp | null;
  /** The staged pixels of the note's background image. */
  imageKey: string | null;
}

/** Where drafts go (`draft_write`, `draft_discard`; tests stand in for them). */
export interface DraftSink {
  write(snapshot: DraftSnapshot): Promise<void>;
  discard(noteId: string): Promise<void>;
  /** Tells Rust whether work is unsaved (the reason Windows shows while it ends the session). */
  unsaved(unsaved: boolean): Promise<void>;
}

export interface DraftTimers {
  now(): number;
  set(run: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

/** A draft is written this long after the last change, milliseconds… */
export const DRAFT_DELAY_MS = 2000;
/** …and no later than this after the first change it holds, while changes keep coming. */
export const DRAFT_MAX_DELAY_MS = 10_000;

const realTimers: DraftTimers = {
  now: () => Date.now(),
  set: (run, ms) => setTimeout(run, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

/**
 * Keeps the draft of the note being edited. `update` at every change of the note (its snapshot
 * while it has unsaved changes, null once it has none), `flush` when the session ends,
 * `discard` when the user throws the changes away, `dispose` when the editor goes away (what
 * is pending is written: it was not saved).
 */
export class DraftKeeper {
  readonly #sink: DraftSink;
  readonly #timers: DraftTimers;
  readonly #onerror: (error: unknown) => void;
  #pending: DraftSnapshot | null = null;
  /** When the oldest change not written yet was made. */
  #since: number | null = null;
  #timer: unknown = null;
  /** The note whose draft this keeper wrote (null: none, or it was removed). */
  #written: string | null = null;
  /** While set, nothing is written (a draft of an earlier session is waiting for the user's choice). */
  #held = false;
  #unsaved = false;
  /** One operation on the draft files at a time, in order. */
  #queue: Promise<void> = Promise.resolve();

  constructor(sink: DraftSink, options: { timers?: DraftTimers; onerror?: (error: unknown) => void } = {}) {
    this.#sink = sink;
    this.#timers = options.timers ?? realTimers;
    this.#onerror = options.onerror ?? (() => {});
  }

  /** The note's snapshot while it has unsaved changes; null once it has none (saved, or undone). */
  update(snapshot: DraftSnapshot | null): void {
    this.#report(snapshot !== null);
    if (snapshot === null) {
      this.#cancel();
      this.#pending = null;
      this.#since = null;
      // Only a draft this keeper wrote goes: one left by an earlier session waits for the user.
      if (this.#written !== null) {
        const id = this.#written;
        this.#written = null;
        void this.#run(() => this.#sink.discard(id));
      }
      return;
    }
    this.#pending = snapshot;
    const now = this.#timers.now();
    this.#since ??= now;
    this.#schedule(Math.min(DRAFT_DELAY_MS, this.#since + DRAFT_MAX_DELAY_MS - now));
  }

  /**
   * The draft of note `noteId` (left by an earlier session) was put over the note: it is now this
   * keeper's, replaced by the next write and removed once nothing is unsaved.
   */
  adopt(noteId: string): void {
    this.#written = noteId;
  }

  /** Holds the writes back (true) or lets them go again (false, writing what is pending soon). */
  hold(held: boolean): void {
    this.#held = held;
    if (!held && this.#pending) this.#schedule(0);
  }

  /** Writes what is pending now (Windows is ending the session). */
  flush(): Promise<void> {
    this.#cancel();
    const snapshot = this.#pending;
    if (!snapshot || this.#held) return this.#queue;
    this.#pending = null;
    this.#since = null;
    this.#written = snapshot.noteId;
    return this.#run(() => this.#sink.write(snapshot));
  }

  /** The user threw the changes of note `noteId` away: its draft goes, whoever wrote it. */
  discard(noteId: string): Promise<void> {
    this.#cancel();
    this.#pending = null;
    this.#since = null;
    const ids = new Set([noteId]);
    if (this.#written !== null) ids.add(this.#written);
    this.#written = null;
    this.#report(false);
    return this.#run(async () => {
      for (const id of ids) await this.#sink.discard(id);
    });
  }

  /** The editor goes away: what is pending is written (it was not saved), then nothing is unsaved here. */
  async dispose(): Promise<void> {
    await this.flush();
    this.#report(false);
  }

  /**
   * The editor moves on to another note: what is pending is written and that draft is left
   * alone (the next note being clean must not remove it).
   */
  detach(): Promise<void> {
    const flushed = this.flush();
    this.#written = null;
    return flushed;
  }

  #report(unsaved: boolean) {
    if (unsaved === this.#unsaved) return;
    this.#unsaved = unsaved;
    void this.#sink.unsaved(unsaved).catch(() => {});
  }

  #schedule(ms: number) {
    this.#cancel();
    this.#timer = this.#timers.set(
      () => {
        this.#timer = null;
        void this.flush();
      },
      Math.max(0, ms),
    );
  }

  #cancel() {
    if (this.#timer !== null) this.#timers.clear(this.#timer);
    this.#timer = null;
  }

  #run(task: () => Promise<void>): Promise<void> {
    const next = this.#queue.then(task).catch((error: unknown) => this.#onerror(error));
    this.#queue = next;
    return next;
  }
}

/** What to offer for a draft when its note is opened. */
export type Recovery =
  /** Nothing: the draft holds what the file holds (a save whose draft could not be removed). */
  | 'none'
  /** Over the note: the file is still the version the edits started from. */
  | 'restore'
  /** As a new note only: the file changed since (another PC), or its version is unknown. */
  | 'copy';

/** The note as compared: everything but its modification time. */
function content(note: Note): string {
  return JSON.stringify({ ...note, modified: 0 });
}

export function recoveryOf(draft: Draft, opened: { note: Note; stamp: FileStamp }): Recovery {
  if (content(draft.note) === content(opened.note)) return 'none';
  return draft.baseStamp && sameStamp(draft.baseStamp, opened.stamp) ? 'restore' : 'copy';
}

/** The draft of the note just opened and what to offer for it; null when there is nothing to offer. */
export interface Offer {
  draft: Draft;
  recovery: Exclude<Recovery, 'none'>;
}

/**
 * Looks for a draft of the note `noteId` just opened; a draft that holds nothing more than the
 * file is removed quietly. Failures to look are not the user's concern (null).
 */
export async function findOffer(
  find: (noteId: string) => Promise<Draft | null>,
  discard: (noteId: string) => Promise<void>,
  noteId: string,
  opened: { note: Note; stamp: FileStamp },
): Promise<Offer | null> {
  const draft = await find(noteId).catch(() => null);
  if (!draft) return null;
  const recovery = recoveryOf(draft, opened);
  if (recovery === 'none') {
    await discard(noteId).catch(() => {});
    return null;
  }
  return { draft, recovery };
}

