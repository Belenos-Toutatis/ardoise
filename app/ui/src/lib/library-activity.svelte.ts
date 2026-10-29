/**
 * What the library screen shows of its longer operations (milestone M10): the progress of an
 * operation on several notes (stoppable between two notes), the notes it could not handle, and
 * for a few seconds afterwards a notice with « Annuler » that undoes it (turning the notes
 * back, restoring the trashed ones). `ActivityBanner.svelte` draws it; `LibraryState` owns one.
 */
import { api, errorMessage, type BinderDeleted } from './api';
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

export interface Offer {
  message: string;
  /** False for a plain confirmation (after an undo): no « Annuler » button. */
  undoable: boolean;
}

export class LibraryActivity {
  progress = $state<Progress | null>(null);
  /** The notes the last operation could not handle, under a heading that says what failed. */
  failures = $state<{ heading: string; items: BatchFailure[] } | null>(null);
  offer = $state<Offer | null>(null);

  #undo: (() => Promise<void>) | null = null;
  #stopAsked = false;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #held = false;

  /** `refresh` reloads the listing once an operation is over. */
  constructor(private readonly refresh: () => Promise<void>) {}

  get busy(): boolean {
    return this.progress !== null;
  }

  /** Stops the running operation after the note being handled. */
  stop(): void {
    if (!this.progress) return;
    this.#stopAsked = true;
    this.progress = { ...this.progress, stopping: true };
  }

  /**
   * Runs `work` on each target with the progress shown under `label`; failures are listed under
   * the plural message `failedKey`. Refuses to start while another operation runs.
   */
  private async batch<R>(
    label: string,
    failedKey: string,
    targets: BatchTarget[],
    work: (target: BatchTarget) => Promise<R>,
  ): Promise<BatchOutcome<R> | null> {
    if (this.busy || targets.length === 0) return null;
    this.dismissOffer();
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
    this.failures =
      done.failed.length > 0
        ? {
            heading: tn('library.binder.deleteFailed', done.failed.length),
            items: done.failed.map((f) => ({ path: f.path, title: baseName(f.path), message: errorMessage(f.error) })),
          }
        : null;
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

  /** Runs the undo of the notice shown, if any. */
  async undo(): Promise<void> {
    const undo = this.#undo;
    this.dismissOffer();
    if (undo) await undo();
  }

  /** Shows a notice for a few seconds; with `undo`, it has an « Annuler » button. */
  show(message: string, undo: (() => Promise<void>) | null): void {
    this.dismissOffer();
    this.offer = { message, undoable: undo !== null };
    this.#undo = undo;
    this.#arm();
  }

  dismissOffer(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    this.#held = false;
    this.offer = null;
    this.#undo = null;
  }

  dismissFailures(): void {
    this.failures = null;
  }

  /** The pointer or the focus is on the notice: it stays until released. */
  hold(): void {
    this.#held = true;
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
  }

  release(): void {
    if (!this.#held) return;
    this.#held = false;
    this.#arm();
  }

  #arm(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = this.offer && !this.#held ? setTimeout(() => this.dismissOffer(), undoNotice.durationMs) : null;
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
