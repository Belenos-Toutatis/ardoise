/**
 * State and actions of the library screen: the listing, the current view, search, order,
 * selection, and every operation on notes (each followed by a refresh). Components read it and
 * call its methods; the pure parts are in `library-logic.ts`.
 */
import { api, errorMessage, followHandwritingStatus, onLibraryChanged } from './api';
import { dragOutSupported } from './drag-out';
import type { HandwritingHit, IndexStatus } from './handwriting';
import { currentLocale, t } from './i18n.svelte';
import { LibraryActivity } from './library-activity.svelte';
import type { TurnDirection } from './library-batch';
import { TagColorState } from './tag-colors.svelte';
import {
  buildBinderTree,
  displayTitle,
  mergeTags,
  oldestFirst,
  parseTags,
  queryWords,
  sameTag,
  sameView,
  visibleNotes,
  type LibraryView,
  type SortOrder,
} from './library-logic';
import type { FileStamp, LibraryListing, MergeLayout, NoteSummary, TrashEntry } from './note';

const THUMBNAIL_CACHE_SIZE = 400;

/** Pause after a keystroke before the handwriting is searched; tests set it to 0. */
export const handwritingSearch = { delayMs: 120 };

const NO_HITS: ReadonlyMap<string, HandwritingHit> = new Map();

/** Whether two queries have the same words (case, accents and spaces aside). */
function sameWords(a: string, b: string): boolean {
  return queryWords(a).join(' ') === queryWords(b).join(' ');
}

export class LibraryState {
  listing = $state<LibraryListing | null>(null);
  trash = $state<TrashEntry[]>([]);
  /** Message of the last failed operation, shown in a banner. */
  error = $state<string | null>(null);
  view = $state<LibraryView>({ kind: 'all' });
  #query = $state('');
  order = $state<SortOrder>('date-desc');
  /** Paths of the selected notes. */
  selected = $state<string[]>([]);
  /** True while cards are being dragged (sidebar drop targets light up). */
  dragging = $state(false);
  /** Notes can be dragged out of the app to other programs (drag-out.ts); Rust decides. */
  dragOutSupported = $state(false);
  /** Confirmation of the last operation that created a note, shown in a banner. */
  notice = $state<string | null>(null);
  /** The export or assemble dialog, with the notes it was opened for (displayed order). */
  dialog = $state<{ kind: 'export' | 'print' | 'assemble'; notes: NoteSummary[] } | null>(null);
  /** Operations on several notes: progress, failures, « Annuler » (`library-activity.svelte.ts`). */
  readonly activity = new LibraryActivity(() => this.refresh());
  /** Colours of the tags (`_ardoise/tags.json`), loaded by the sidebar (`tag-colors.svelte.ts`). */
  readonly tagColors = new TagColorState((message) => (this.error = message));

  /** Paths being dragged; kept here because a drag event's data is not readable until the drop. */
  private dragPaths: string[] = [];
  private anchor: string | null = null;
  private sequence = 0;
  private thumbnails = new Map<string, Promise<string | null>>();

  // Handwriting search (milestone M7): the desktop app says which query words the handwriting
  // of each note has; the search of titles, tags and binders combines them (`matchesSearch`).
  /** The background indexing of the handwriting; null until known. */
  handwriting = $state<IndexStatus | null>(null);
  #hitsFound = $state.raw<ReadonlyMap<string, HandwritingHit>>(NO_HITS);
  #hitsQuery = $state('');
  #searchTimer: ReturnType<typeof setTimeout> | null = null;
  #searchSequence = 0;
  /** What the handwriting search found for the current query, by path (none while it runs). */
  readonly hits = $derived(sameWords(this.#hitsQuery, this.#query) ? this.#hitsFound : NO_HITS);

  readonly notes = $derived(this.listing?.notes ?? []);
  readonly tree = $derived(buildBinderTree(this.listing?.binders ?? [], this.notes));
  readonly visible = $derived(visibleNotes(this.notes, this.view, this.#query, this.order, currentLocale(), this.hits));
  readonly selectedNotes = $derived(this.notes.filter((n) => this.selected.includes(n.path)));
  /** The selected notes as the grid shows them; those the search hides come last. */
  readonly selectedDisplayed = $derived.by(() => {
    const shown = this.visible.filter((n) => this.selected.includes(n.path));
    const rest = this.selectedNotes.filter((n) => !shown.includes(n));
    return [...shown, ...rest];
  });

  // --- Loading -------------------------------------------------------------------------------

  /** The search text; changing it searches the handwriting too, after a short pause. */
  get query(): string {
    return this.#query;
  }

  set query(value: string) {
    this.#query = value;
    this.searchHandwritingSoon();
  }

  /** Loads the library and starts following changes on disk. Returns the function that stops it. */
  start(): () => void {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    void this.refresh();
    void dragOutSupported().then((supported) => (this.dragOutSupported = supported));
    void onLibraryChanged(() => void this.refresh()).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });
    // More notes become searchable as the indexing goes on.
    const unfollow = followHandwritingStatus((status) => {
      this.handwriting = status;
      if (queryWords(this.#query).length > 0) this.searchHandwritingSoon(false);
    });
    return () => {
      stopped = true;
      unlisten?.();
      unfollow();
      if (this.#searchTimer) clearTimeout(this.#searchTimer);
    };
  }

  /**
   * Asks the desktop app, after a short pause, what the handwriting has of the query. A typed
   * query restarts the pause; a change of the library or of the indexing (`restart` false) keeps
   * a search already planned, so that frequent changes cannot put it off for ever.
   */
  private searchHandwritingSoon(restart = true): void {
    if (this.#searchTimer && !restart) return;
    if (this.#searchTimer) clearTimeout(this.#searchTimer);
    this.#searchTimer = null;
    const query = this.#query;
    if (queryWords(query).length === 0) {
      this.#searchSequence += 1;
      this.#hitsFound = NO_HITS;
      this.#hitsQuery = '';
      return;
    }
    this.#searchTimer = setTimeout(() => {
      this.#searchTimer = null;
      void this.searchHandwriting(this.#query);
    }, handwritingSearch.delayMs);
  }

  private async searchHandwriting(query: string): Promise<void> {
    const mine = ++this.#searchSequence;
    try {
      const hits = await api.handwritingSearch(query);
      if (mine !== this.#searchSequence) return;
      this.#hitsFound = new Map(hits.map((hit) => [hit.path, hit]));
      this.#hitsQuery = query;
    } catch {
      // No recognition: the search keeps to titles, tags and binders (the notice says why).
    }
  }

  async refresh(): Promise<void> {
    const mine = ++this.sequence;
    try {
      const [listing, trash] = await Promise.all([api.libraryList(), api.trashList()]);
      if (mine !== this.sequence) return;
      this.listing = listing;
      this.trash = trash;
      const known = new Set(listing.notes.map((n) => n.path));
      if (this.selected.some((path) => !known.has(path))) {
        this.selected = this.selected.filter((path) => known.has(path));
      }
      const view = this.view;
      if (view.kind === 'binder' && !this.hasBinder(view.binder)) this.view = { kind: 'all' };
      if (view.kind === 'tag' && !listing.tags.some((t) => sameTag(t.tag, view.tag))) this.view = { kind: 'all' };
      // Notes may have moved or changed: their handwriting hits too.
      if (queryWords(this.#query).length > 0) this.searchHandwritingSoon(false);
    } catch (error) {
      if (mine === this.sequence) this.error = errorMessage(error);
    }
  }

  private hasBinder(binder: string): boolean {
    return binder === '' || (this.listing?.binders ?? []).some((b) => b === binder);
  }

  /** Runs an operation, shows its error, and refreshes whatever happened. */
  private async run<T>(action: () => Promise<T>): Promise<T | undefined> {
    this.error = null;
    try {
      return await action();
    } catch (error) {
      this.error = errorMessage(error);
      return undefined;
    } finally {
      await this.refresh();
    }
  }

  /** Applies `action` to each note, keeps going after a failure and reports the first one. */
  private async each(notes: NoteSummary[], action: (note: NoteSummary) => Promise<unknown>): Promise<void> {
    this.error = null;
    let first: string | null = null;
    for (const note of notes) {
      try {
        await action(note);
      } catch (error) {
        first ??= errorMessage(error);
      }
    }
    if (first) this.error = first;
    await this.refresh();
  }

  dismissError(): void {
    this.error = null;
  }

  dismissNotice(): void {
    this.notice = null;
  }

  /** Opens the export, print or assemble dialog over the notes selected now. */
  openDialog(kind: 'export' | 'print' | 'assemble'): void {
    this.dialog = { kind, notes: [...this.selectedDisplayed] };
  }

  closeDialog(): void {
    this.dialog = null;
  }

  /**
   * Assembles notes (oldest first) into a new note, which becomes the selection. The error, if
   * any, goes back to the caller (the dialog shows it).
   */
  async assemble(notes: NoteSummary[], layout: MergeLayout, title: string): Promise<NoteSummary> {
    this.notice = null;
    const created = await api.mergeNotes(
      oldestFirst(notes).map((n) => n.path),
      layout,
      title.trim(),
    );
    await this.refresh();
    this.selected = [created.path];
    this.anchor = created.path;
    this.notice = t('library.assemble.done', { title: displayTitle(created, currentLocale()) });
    return created;
  }

  // --- View ----------------------------------------------------------------------------------

  setView(view: LibraryView): void {
    if (sameView(view, this.view)) return;
    this.view = view;
    this.clearSelection();
  }

  /** The binder new notes go to: the one being viewed, else the root. */
  get currentBinder(): string {
    return this.view.kind === 'binder' ? this.view.binder : '';
  }

  // --- Selection -----------------------------------------------------------------------------

  isSelected(path: string): boolean {
    return this.selected.includes(path);
  }

  /** True when a range selection can start from the last toggled note. */
  get hasAnchor(): boolean {
    return this.anchor !== null;
  }

  clearSelection(): void {
    this.selected = [];
    this.anchor = null;
  }

  toggle(path: string): void {
    this.selected = this.isSelected(path) ? this.selected.filter((p) => p !== path) : [...this.selected, path];
    this.anchor = path;
  }

  /** Selects the notes from the last toggled one to `path`, in display order. */
  selectRange(path: string): void {
    const paths = this.visible.map((n) => n.path);
    const from = this.anchor ? paths.indexOf(this.anchor) : -1;
    const to = paths.indexOf(path);
    if (from < 0 || to < 0) {
      this.selected = [path];
      this.anchor = path;
      return;
    }
    const range = paths.slice(Math.min(from, to), Math.max(from, to) + 1);
    this.selected = [...new Set([...this.selected, ...range])];
  }

  selectAll(): void {
    this.selected = this.visible.map((n) => n.path);
  }

  // --- Drag and drop -------------------------------------------------------------------------

  /** A drag starts on `path`: it carries the whole selection when the card is part of it. */
  beginDrag(path: string): string[] {
    this.dragPaths = this.isSelected(path) ? [...this.selected] : [path];
    this.dragging = true;
    return this.dragPaths;
  }

  /** The notes a drag out to another program carries from the card of `path`: the selection if it is part of it. */
  dragOutPaths(path: string): string[] {
    return this.isSelected(path) ? this.selectedDisplayed.map((n) => n.path) : [path];
  }

  endDrag(): void {
    this.dragPaths = [];
    this.dragging = false;
  }

  /** Moves the dragged notes into `binder` (`''` is the library root). */
  async dropOn(binder: string): Promise<void> {
    const paths = this.dragPaths.filter((p) => this.notes.find((n) => n.path === p)?.binder !== binder);
    this.endDrag();
    if (paths.length > 0) await this.moveNotes(paths, binder);
  }

  // --- Thumbnails ----------------------------------------------------------------------------

  thumbnail(path: string, stamp: FileStamp): Promise<string | null> {
    const key = `${path}|${stamp.modified.secs_since_epoch}.${stamp.modified.nanos_since_epoch}|${stamp.len}`;
    let found = this.thumbnails.get(key);
    if (!found) {
      found = api.noteThumbnail(path, stamp).catch(() => null);
      this.thumbnails.set(key, found);
      if (this.thumbnails.size > THUMBNAIL_CACHE_SIZE) {
        const oldest = this.thumbnails.keys().next().value;
        if (oldest !== undefined) this.thumbnails.delete(oldest);
      }
    }
    return found;
  }

  // --- Operations on notes -------------------------------------------------------------------

  /** Creates a note in the current binder and returns it. */
  async createNote(): Promise<NoteSummary | undefined> {
    return this.run(() => api.createNote(this.currentBinder, ''));
  }

  async createBinder(parent: string, name: string): Promise<boolean> {
    const trimmed = name.trim();
    if (!trimmed) return false;
    const done = await this.run(async () => {
      await api.createBinder(parent ? `${parent}/${trimmed}` : trimmed);
      return true;
    });
    return done === true;
  }

  async moveNotes(paths: string[], binder: string): Promise<void> {
    await this.run(() => api.moveNotes(paths, binder));
    this.clearSelection();
  }

  async moveSelected(binder: string): Promise<void> {
    await this.moveNotes([...this.selected], binder);
  }

  async trashNotes(paths: string[]): Promise<void> {
    const titles = paths.map((path) => this.titleOf(path));
    const ids = await this.run(() => api.trashNotes(paths));
    this.selected = this.selected.filter((p) => !paths.includes(p));
    if (ids) this.activity.offerRestore(ids.map((id, i) => ({ path: id, title: titles[i] ?? id })));
  }

  /** Turns each note a quarter turn, as the editor does; offers to turn them back. */
  async turnNotes(notes: NoteSummary[], direction: TurnDirection): Promise<void> {
    this.error = null;
    await this.activity.turn(
      notes.map((note) => ({ path: note.path, title: displayTitle(note, currentLocale()) })),
      direction,
    );
  }

  private titleOf(path: string): string {
    const note = this.notes.find((n) => n.path === path);
    return note ? displayTitle(note, currentLocale()) : path;
  }

  async toggleFavorite(note: NoteSummary): Promise<void> {
    await this.run(() => api.setNoteFavorite(note.path, !note.favorite, note.stamp));
  }

  async setFavorite(notes: NoteSummary[], favorite: boolean): Promise<void> {
    await this.each(
      notes.filter((n) => n.favorite !== favorite),
      (note) => api.setNoteFavorite(note.path, favorite, note.stamp),
    );
  }

  /** Adds `tags` (text such as `a, b`) to each note. */
  async addTags(notes: NoteSummary[], text: string): Promise<void> {
    const added = parseTags(text);
    if (added.length === 0) return;
    await this.each(notes, (note) => {
      const tags = mergeTags(note.tags, added);
      return tags.length === note.tags.length ? Promise.resolve() : api.setNoteTags(note.path, tags, note.stamp);
    });
  }

  async removeTag(notes: NoteSummary[], tag: string): Promise<void> {
    await this.each(
      notes.filter((n) => n.tags.some((t) => sameTag(t, tag))),
      (note) =>
        api.setNoteTags(
          note.path,
          note.tags.filter((t) => !sameTag(t, tag)),
          note.stamp,
        ),
    );
  }

  /** Renames a note; returns its new path. The selection follows it. */
  async retitle(note: NoteSummary, title: string): Promise<string | undefined> {
    const result = await this.run(() => api.retitleNote(note.path, title.trim(), note.stamp));
    if (!result) return undefined;
    this.selected = this.selected.map((p) => (p === note.path ? result.path : p));
    if (this.anchor === note.path) this.anchor = result.path;
    return result.path;
  }

  // --- Trash ---------------------------------------------------------------------------------

  async restore(id: string): Promise<void> {
    await this.run(() => api.trashRestore(id));
  }

  async purge(id: string): Promise<void> {
    await this.run(() => api.trashPurge(id));
  }

  /**
   * Deletes for good the entries the user confirmed, one by one: a note trashed meanwhile
   * (on another PC, say) stays in the trash instead of being deleted unseen.
   */
  async emptyTrash(ids: string[]): Promise<void> {
    this.error = null;
    let first: string | null = null;
    for (const id of ids) {
      try {
        await api.trashPurge(id);
      } catch (error) {
        first ??= errorMessage(error);
      }
    }
    if (first) this.error = first;
    await this.refresh();
  }
}
