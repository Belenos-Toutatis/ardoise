/**
 * What the library screen shows of its longer operations (milestone M10): the progress of an
 * operation on several notes (stoppable between two notes), the notes it could not handle, and
 * for a few seconds afterwards a notice with « Annuler » that undoes it (turning the notes
 * back, restoring the trashed ones). `ActivityBanner.svelte` draws it; `LibraryState` owns one.
 *
 * Operations on several notes run one at a time: one asked while another runs (an undo, say)
 * waits for it. Notices pile up (the last few), each with its own delay, so that a notice never
 * takes the place of another one still useful.
 */
import { api, errorMessage, type BinderDeleted, type FailedNote, type TrashedNotes } from './api';
import { t, tn } from './i18n.svelte';
import { opposite, runBatch, turnNote, type BatchFailure, type BatchOutcome, type BatchTarget, type TurnDirection } from './library-batch';
import type { FileStamp } from './note';

/** How long a notice stays, in milliseconds (paused while the pointer or the focus is on it); tests change it. */
export const undoNotice = { durationMs: 8000 };

export interface Progress {
  /** Translation key of the label, with `{done}` and `{total}`. */
  label: string;
  done: number;
  total: number;
  stopping: boolean;
}

/** Notices kept on display at most; older ones go first. */
const MAX_OFFERS = 3;

export interface Offer {
  id: number;
  message: string;
  /** False for a plain confirmation (after an undo): no « Annuler » button. */
  undoable: boolean;
}

export class LibraryActivity {
  progress = $state<Progress | null>(null);
  /** The notes the last operation could not handle, under a heading that says what failed. */
  failures = $state<{ heading: string; items: BatchFailure[] } | null>(null);
  /** The notices on display, oldest first. */
  offers = $state<Offer[]>([]);

  #undos = new Map<number, () => Promise<void>>();
  #timers = new Map<number, ReturnType<typeof setTimeout>>();
  #held = false;
  #ids = 0;
  #stopAsked = false;
  /** Settles when the operations asked so far are over. */
  #queue: Promise<unknown> = Promise.resolve();

  /** `refresh` reloads the listing once an operation is over. */
  constructor(private readonly refresh: () => Promise<void>) {}

  get busy(): boolean {
    return this.progress !== null;
  }

  /** The latest notice that can be undone (« Annuler », Ctrl+Z). */
  get latestUndoable(): Offer | null {
    return [...this.offers].reverse().find((offer) => offer.undoable) ?? null;
  }

  /** Stops the running operation after the note being handled. */
  stop(): void {
    if (!this.progress) return;
    this.#stopAsked = true;
    this.progress = { ...this.progress, stopping: true };
  }

  /**
   * Runs `work` on each target with the progress shown under `label`, once the operations asked
   * before are over; failures are listed under the plural message `failedKey`.
   */
  private batch<R>(
    label: string,
    failedKey: string,
    targets: BatchTarget[],
    work: (target: BatchTarget) => Promise<R>,
  ): Promise<BatchOutcome<R> | null> {
    if (targets.length === 0) return Promise.resolve(null);
    const run = this.#queue.then(() => this.run(label, failedKey, targets, work));
    this.#queue = run.catch(() => undefined);
    return run;
  }

  private async run<R>(
    label: string,
    failedKey: string,
    targets: BatchTarget[],
    work: (target: BatchTarget) => Promise<R>,
  ): Promise<BatchOutcome<R>> {
    this.failures = null;
    this.#stopAsked = false;
    this.progress = { label, done: 0, total: targets.length, stopping: false };
    try {
      const outcome = await runBatch(targets, work, {
        onprogress: (done, total) => {
          if (this.progress) this.progress = { ...this.progress, done, total };
        },
        shouldStop: () => this.#stopAsked,
      });
      if (outcome.failed.length > 0) {
        this.failures = { heading: tn(failedKey, outcome.failed.length), items: outcome.failed };
      }
      return outcome;
    } finally {
      this.progress = null;
      await this.refresh();
    }
  }

  /** Turns each note a quarter turn, then offers to turn them back. */
  async turn(targets: BatchTarget[], direction: TurnDirection): Promise<void> {
    const outcome = await this.batch('library.turn.progress', 'library.turn.failed', targets, (target) =>
      turnNote(target.path, direction),
    );
    if (!outcome || outcome.done.length === 0) return;
    const turned = outcome.done.map(({ path, title, result }) => ({ path, title, stamp: result }));
    let message = tn(direction === 'cw' ? 'library.turn.doneRight' : 'library.turn.doneLeft', turned.length);
    const untouched = targets.length - outcome.done.length - outcome.failed.length;
    if (outcome.stopped) message += ` ${tn('library.activity.stopped', untouched)}`;
    this.show(message, () => this.turnBack(turned, direction));
  }

  /** Turns the notes back, each only if it is still the version the turn wrote. */
  private async turnBack(turned: (BatchTarget & { stamp: FileStamp })[], direction: TurnDirection): Promise<void> {
    const stamps = new Map(turned.map((note) => [note.path, note.stamp]));
    const outcome = await this.batch('library.turn.undoProgress', 'library.turn.undoFailed', turned, (target) =>
      turnNote(target.path, opposite(direction), stamps.get(target.path)),
    );
    if (outcome && outcome.done.length > 0) this.show(tn('library.turn.undone', outcome.done.length), null);
  }

  /** After `trash_notes`: lists the notes that stayed, and offers to restore the others. */
  trashed(done: TrashedNotes, titleOf: (path: string) => string): void {
    this.failures = failuresOf(tn('library.trashUndo.trashFailed', done.failed.length), done.failed, titleOf);
    this.offerRestore(done.trashed.map(({ path, id }) => ({ path: id, title: titleOf(path) })));
  }

  /**
   * After notes went to the trash: offers to restore them. Each target's `path` is the id of its
   * trash entry, `title` the note's title.
   */
  offerRestore(entries: BatchTarget[], message = tn('library.trashUndo.done', entries.length)): void {
    if (entries.length === 0) return;
    this.show(message, () => this.restore(entries));
  }

  private async restore(entries: BatchTarget[], done = (count: number) => tn('library.trashUndo.restored', count)): Promise<void> {
    const outcome = await this.batch('library.trashUndo.progress', 'library.trashUndo.failed', entries, (entry) =>
      api.trashRestore(entry.path),
    );
    if (outcome && outcome.done.length > 0) this.show(done(outcome.done.length), null);
  }

  /**
   * After a binder was deleted (`done`, from `delete_binder`): lists the notes that could not go
   * to the trash, says which folders were kept for the other files they hold, and offers to bring
   * the binder back with its notes.
   */
  binderDeleted(binder: string, name: string, done: BinderDeleted): void {
    this.failures = failuresOf(tn('library.binder.deleteFailed', done.failed.length), done.failed, baseName);
    const entries = done.trashed.map((entry) => ({ path: entry.id, title: entry.title?.trim() || baseName(entry.original) }));
    let message =
      entries.length === 0 ? t('library.binder.deletedEmpty', { name }) : tn('library.binder.deleted', entries.length, { name });
    const kept = deepestFolders(done.kept);
    if (kept.length > 0) {
      const folders = kept.map((folder) => t('library.binder.folder', { name: folder })).join(', ');
      message += ` ${tn('library.binder.kept', kept.length, { folders })}`;
    }
    this.show(message, async () => {
      // The binder itself first: an empty one has no note to bring it back.
      await api.createBinder(binder).catch(() => undefined);
      if (entries.length === 0) {
        await this.refresh();
        this.show(t('library.binder.restoredEmpty', { name }), null);
      } else {
        await this.restore(entries, (count) => tn('library.binder.restored', count, { name }));
      }
    });
  }

  /** Runs the undo of the notice `id` (by default the latest that has one). */
  async undo(id = this.latestUndoable?.id): Promise<void> {
    if (id === undefined) return;
    const undo = this.#undos.get(id);
    this.dismissOffer(id);
    if (undo) await undo();
  }

  /** Shows a notice for a few seconds; with `undo`, it has an « Annuler » button. */
  show(message: string, undo: (() => Promise<void>) | null): void {
    const id = ++this.#ids;
    if (undo) this.#undos.set(id, undo);
    for (const old of this.offers.slice(0, Math.max(0, this.offers.length + 1 - MAX_OFFERS))) this.dismissOffer(old.id);
    this.offers = [...this.offers, { id, message, undoable: undo !== null }];
    this.#arm(id);
  }

  /** Removes the notice `id`, or every notice. */
  dismissOffer(id?: number): void {
    for (const offer of this.offers) {
      if (id !== undefined && offer.id !== id) continue;
      clearTimeout(this.#timers.get(offer.id));
      this.#timers.delete(offer.id);
      this.#undos.delete(offer.id);
    }
    this.offers = id === undefined ? [] : this.offers.filter((offer) => offer.id !== id);
    if (this.offers.length === 0) this.#held = false;
  }

  dismissFailures(): void {
    this.failures = null;
  }

  /** The pointer or the focus is on the notices: they stay until released. */
  hold(): void {
    this.#held = true;
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
  }

  release(): void {
    if (!this.#held) return;
    this.#held = false;
    for (const offer of this.offers) this.#arm(offer.id);
  }

  #arm(id: number): void {
    clearTimeout(this.#timers.get(id));
    if (this.#held) return;
    this.#timers.set(id, setTimeout(() => this.dismissOffer(id), undoNotice.durationMs));
  }
}

/** Last part of a path with either separator. */
function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** The kept folders that hold no other kept folder (those above them are kept because of them). */
export function deepestFolders(folders: readonly string[]): string[] {
  return folders.filter((folder) => !folders.some((other) => other.startsWith(`${folder}/`)));
}

/** The failures to list under `heading`, or null when there are none. */
function failuresOf(heading: string, failed: FailedNote[], titleOf: (path: string) => string) {
  if (failed.length === 0) return null;
  return { heading, items: failed.map((f) => ({ path: f.path, title: titleOf(f.path), message: errorMessage(f.error) })) };
}
