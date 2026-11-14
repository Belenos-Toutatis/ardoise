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
  arrangeByBinderOrder,
  binderChain,
  binderPathBelow,
  binderSummary,
  buildBinderTree,
  displayTitle,
  isInBinder,
  mergeTags,
  moveByStep,
  moveToSpot,
  oldestFirst,
  orderIn,
  parseTags,
  queryWords,
  sameTag,
  sameView,
  sortChoices,
  visibleNotes,
  type DropSpot,
  type LibraryView,
  type MoveStep,
  type OrderChange,
  type SortOrder,
  type SummaryNote,
} from './library-logic';
import type { ExportSource, FileStamp, LibraryListing, MergeLayout, NoteSummary, TrashEntry } from './note';

/** Thumbnails kept: at least this many, and enough for the whole library (up to the maximum). */
const THUMBNAIL_CACHE_MIN = 400;
const THUMBNAIL_CACHE_MAX = 2000;
/** The cached pictures are data URLs: past this many characters (about as many bytes) the oldest go. */
const THUMBNAIL_CACHE_CHARS = 64_000_000;

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
  /** The trash entry's menu asked « Vider la corbeille… »: the trash view, once shown, asks for the confirmation (and clears this). */
  emptyTrashAsked = $state(false);
  /** Message of the last failed operation, shown in a banner. */
  error = $state<string | null>(null);
  view = $state<LibraryView>({ kind: 'all' });
  #query = $state('');
  /** The order the user chose; what is shown is `effectiveOrder` (the free order of a binder only applies to one). */
  order = $state<SortOrder>('date-desc');
  /** Paths of the selected notes. */
  selected = $state<string[]>([]);
  /** True while cards are being dragged (sidebar drop targets light up). */
  dragging = $state(false);
  /** Notes can be dragged out of the app to other programs (drag-out.ts); Rust decides. */
  dragOutSupported = $state(false);
  /** Confirmation of the last operation that created a note, shown in a banner. */
  notice = $state<string | null>(null);
  /**
   * The export, print or assemble dialog, with the notes it was opened for (displayed order). With `binder`, it is
   * « Exporter le classeur en PDF » or « Imprimer le classeur » of that binder (`''` never): the notes are the binder's,
   * in its order, and the dialog offers its sub-binders as an option (`binderNotes`).
   */
  dialog = $state<{ kind: 'export' | 'print' | 'assemble'; notes: NoteSummary[]; binder?: string } | null>(null);
  /** « Changer la date… » is open on this note. */
  dateNote = $state<NoteSummary | null>(null);
  /** The summary of this binder is open (`''` never: only a binder has one). */
  summaryBinder = $state<string | null>(null);
  /** The place between two cards a drag would drop at, and the one a screen reader was last told about. */
  insertion = $state<DropSpot | null>(null);
  /** What a screen reader is told after a card moved (an `aria-live` region of the grid). */
  announcement = $state('');
  /** Operations on several notes: progress, failures, « Annuler » (`library-activity.svelte.ts`). */
  readonly activity = new LibraryActivity(() => this.refresh());
  /** Colours of the tags (`_ardoise/tags.json`), loaded by the sidebar (`tag-colors.svelte.ts`). */
  readonly tagColors = new TagColorState((message) => (this.error = message));

  /** The writes of the order of a binder under way, and the last of them (`applyOrder`). */
  #orderWrites = 0;
  #orderChain: Promise<void> = Promise.resolve();
  /** Changes at each move and each write of an order: a listing read before the change is older than what the grid shows. */
  #orderEpoch = 0;

  /** Paths being dragged; kept here because a drag event's data is not readable until the drop. */
  private dragPaths: string[] = [];
  private anchor: string | null = null;
  private sequence = 0;
  private thumbnails = new Map<string, Promise<string | null>>();
  /** Length of each cached picture once known, and their total. */
  private thumbnailChars = new Map<string, number>();
  private thumbnailTotal = 0;

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
  /** The order shown: the user's, except that the free order of a binder falls back to the date outside a binder. */
  readonly effectiveOrder = $derived(orderIn(this.order, this.view));
  /** The orders the sort menu offers in this view. */
  readonly sortChoices = $derived(sortChoices(this.view));
  readonly visible = $derived(
    visibleNotes(this.notes, this.view, this.#query, this.order, currentLocale(), this.hits, {
      tree: this.tree,
      orders: this.listing?.orders ?? {},
    }),
  );
  /**
   * Cards can be moved among each other: the free order of a binder is shown and nothing narrows the notes (a search
   * would hide cards whose places matter).
   */
  readonly canReorder = $derived(this.effectiveOrder === 'binder' && this.view.kind === 'binder' && this.#query.trim() === '');
  /** `selected` as a set: every card asks whether it is selected. */
  readonly #selectedSet = $derived(new Set(this.selected));
  readonly selectedNotes = $derived(this.notes.filter((n) => this.#selectedSet.has(n.path)));
  /** The selected notes as the grid shows them; those the search hides come last. */
  readonly selectedDisplayed = $derived.by(() => {
    const shown = this.visible.filter((n) => this.#selectedSet.has(n.path));
    const shownPaths = new Set(shown.map((n) => n.path));
    const rest = this.selectedNotes.filter((n) => !shownPaths.has(n.path));
    return [...shown, ...rest];
  });
  readonly #selectedPaths = $derived(this.selectedDisplayed.map((n) => n.path));

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
    const epoch = this.#orderEpoch;
    try {
      const [listing, trash] = await Promise.all([api.libraryList(), api.trashList()]);
      if (mine !== this.sequence) return;
      // A move made while the listing was being read: the orders it holds are older than the grid's, which wins.
      this.listing = epoch === this.#orderEpoch || !this.listing ? listing : { ...listing, orders: this.listing.orders };
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

  /** Opens « Exporter le classeur en PDF » or « Imprimer le classeur » over `binder`. */
  openBinderDialog(kind: 'export' | 'print', binder: string): void {
    this.dialog = { kind, notes: this.binderNotes(binder, false), binder };
  }

  /** Whether `binder` has a sub-binder (the dialogs offer to include them only then). */
  hasSubBinders(binder: string): boolean {
    return binderChain(this.tree, binder).length > 1;
  }

  /** What exporting or printing `binder` takes: its notes (`binderNotes`) as sources, each with the bookmark it gets in a PDF. */
  binderSources(binder: string, includeSub: boolean): ExportSource[] {
    const locale = currentLocale();
    return this.binderNotes(binder, includeSub).map((note) => ({
      path: note.path,
      outline: { path: binderPathBelow(binder, note.binder), title: displayTitle(note, locale) },
    }));
  }

  /**
   * The notes of `binder` in its free order, with those of its sub-binders after them (each in its own order, the
   * sub-binders as the sidebar shows them) when `includeSub`: what exporting or printing a binder takes, whatever sort
   * is on screen.
   */
  binderNotes(binder: string, includeSub: boolean): NoteSummary[] {
    const chain = includeSub ? binderChain(this.tree, binder) : [binder];
    const shown = this.notes.filter((n) => chain.includes(n.binder));
    return arrangeByBinderOrder(shown, binder, { tree: this.tree, orders: this.listing?.orders ?? {} }, currentLocale());
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
    return this.#selectedSet.has(path);
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

  /**
   * Selects the notes from the last toggled one to `path`, in display order. With no such note (nothing was ticked
   * or Ctrl-clicked, or it is no longer shown), the range starts at the first note shown, as in the Explorer.
   */
  selectRange(path: string): void {
    const paths = this.visible.map((n) => n.path);
    const to = paths.indexOf(path);
    if (to < 0) {
      this.selected = [path];
      this.anchor = path;
      return;
    }
    let from = this.anchor ? paths.indexOf(this.anchor) : -1;
    if (from < 0) {
      from = 0;
      this.anchor = paths[0];
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
    return this.isSelected(path) ? [...this.#selectedPaths] : [path];
  }

  /** How many notes a drag out from the card of `path` carries. */
  dragOutCount(path: string): number {
    return this.isSelected(path) ? this.#selectedPaths.length : 1;
  }

  endDrag(): void {
    this.dragPaths = [];
    this.dragging = false;
    this.insertion = null;
  }

  /**
   * A drag is over the left or right half of the card `path`: the mark shows where the dragged cards would land.
   * Returns whether they may land there (the free order is shown, and the cards of the dragged notes' binder are next
   * to the spot); the card then accepts the drop.
   */
  hoverSpot(path: string, side: 'before' | 'after'): boolean {
    const spot = this.canReorder && this.dragging ? { path, side } : null;
    if (!spot || !moveToSpot(this.visible, this.dragPaths, spot)) {
      this.insertion = null;
      return false;
    }
    if (this.insertion?.path !== path || this.insertion.side !== side) this.insertion = spot;
    return true;
  }

  /** The pointer left the card `path`: its mark goes (the next card sets its own). */
  leaveSpot(path: string): void {
    if (this.insertion?.path === path) this.insertion = null;
  }

  /** The dragged cards are dropped at the place the mark shows: they move there, in their binder's order. */
  async dropAtSpot(): Promise<void> {
    const spot = this.insertion;
    const paths = this.dragPaths;
    const change = spot ? moveToSpot(this.visible, paths, spot) : null;
    this.endDrag();
    if (change) this.applyOrder(change, paths);
  }

  /**
   * The keyboard's way to reorder: the card `path` (or the whole selection when it is part of it) goes one place
   * earlier or later among the notes of its binder, or to its start or end. Said aloud through `announcement`.
   * Returns whether anything moved.
   */
  async moveCards(path: string, step: MoveStep): Promise<boolean> {
    if (!this.canReorder) return false;
    const paths = this.isSelected(path) ? [...this.selected] : [path];
    const change = moveByStep(this.visible, paths, step);
    if (!change) {
      this.announcement = t(step === 'earlier' || step === 'start' ? 'library.order.atStart' : 'library.order.atEnd');
      return false;
    }
    this.applyOrder(change, paths);
    return true;
  }

  /**
   * Stores a new order of one binder: the grid follows at once (so the caller can put the focus back on the card without
   * waiting for the file), and the file is written behind it.
   */
  private applyOrder(change: OrderChange, paths: string[]): void {
    this.error = null;
    const titles = paths.map((p) => this.titleOf(p));
    this.announcement =
      titles.length === 1
        ? t('library.order.moved', { title: titles[0], position: change.position, count: change.count })
        : t('library.order.movedMany', { count: titles.length, position: change.position, total: change.count });
    if (this.listing) this.listing = { ...this.listing, orders: { ...this.listing.orders, [change.binder]: change.ids } };
    // One write after the other, in the order the moves were made: a key held down makes moves faster than the files
    // are written, and two writes at once could end with the older order. The listing is read again once the last
    // write is over (reading it between two would bring the older order back for a moment).
    this.#orderWrites += 1;
    this.#orderEpoch += 1;
    const write = this.#orderChain.then(async () => {
      try {
        await api.setBinderOrder(change.binder, change.ids);
      } catch (error) {
        this.error = errorMessage(error);
        this.announcement = '';
      }
    });
    this.#orderChain = write;
    void write.then(async () => {
      this.#orderWrites -= 1;
      this.#orderEpoch += 1;
      if (this.#orderWrites === 0) await this.refresh();
    });
  }

  /** Moves the dragged notes into `binder` (`''` is the library root). */
  async dropOn(binder: string): Promise<void> {
    const paths = this.dragPaths.filter((p) => this.notes.find((n) => n.path === p)?.binder !== binder);
    this.endDrag();
    if (paths.length > 0) await this.moveNotes(paths, binder);
  }

  /** The dragged notes are dropped on « Favoris »: they become favourites (those that are already stay so). */
  async dropOnFavorites(): Promise<void> {
    const notes = this.draggedNotes();
    this.endDrag();
    if (notes.length > 0) await this.setFavorite(notes, true);
  }

  /** The dragged notes are dropped on a tag of the sidebar: they get that tag (it is never taken off this way). */
  async dropOnTag(tag: string): Promise<void> {
    const notes = this.draggedNotes();
    this.endDrag();
    if (notes.length > 0) await this.addTags(notes, tag);
  }

  private draggedNotes(): NoteSummary[] {
    return this.dragPaths.map((p) => this.notes.find((n) => n.path === p)).filter((n): n is NoteSummary => n !== undefined);
  }

  // --- Thumbnails ----------------------------------------------------------------------------

  thumbnail(path: string, stamp: FileStamp): Promise<string | null> {
    const key = `${path}|${stamp.modified.secs_since_epoch}.${stamp.modified.nanos_since_epoch}|${stamp.len}`;
    let found = this.thumbnails.get(key);
    if (!found) {
      found = api
        .noteThumbnail(path, stamp)
        .catch(() => null)
        .then((url) => {
          // Only what is still cached counts (the entry may have been evicted meanwhile).
          if (url !== null && this.thumbnails.has(key) && !this.thumbnailChars.has(key)) {
            this.thumbnailChars.set(key, url.length);
            this.thumbnailTotal += url.length;
            this.trimThumbnails();
          }
          return url;
        });
      this.thumbnails.set(key, found);
      this.trimThumbnails();
    }
    return found;
  }

  /** Drops the oldest pictures beyond what the library needs and the memory budget allows. */
  private trimThumbnails(): void {
    const limit = Math.min(Math.max(THUMBNAIL_CACHE_MIN, this.notes.length + 100), THUMBNAIL_CACHE_MAX);
    while (
      this.thumbnails.size > 1 &&
      (this.thumbnails.size > limit || this.thumbnailTotal > THUMBNAIL_CACHE_CHARS)
    ) {
      const oldest = this.thumbnails.keys().next().value;
      if (oldest === undefined) break;
      this.thumbnails.delete(oldest);
      this.thumbnailTotal -= this.thumbnailChars.get(oldest) ?? 0;
      this.thumbnailChars.delete(oldest);
    }
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
    const titles = new Map(paths.map((path) => [path, this.titleOf(path)]));
    const done = await this.run(() => api.trashNotes(paths));
    if (!done) return;
    // The notes that could not go stay selected, ready for another try.
    const gone = new Set(done.trashed.map((t) => t.path));
    this.selected = this.selected.filter((p) => !gone.has(p));
    this.activity.trashed(done, (path) => titles.get(path) ?? path);
  }

  /**
   * Writes a copy of each note next to it (same binder and date, `(copie)` after the title) one after the other, with
   * progress; offers to delete the copies.
   */
  async duplicateNotes(notes: NoteSummary[]): Promise<void> {
    this.error = null;
    await this.activity.duplicate(
      notes.map((note) => ({ path: note.path, title: displayTitle(note, currentLocale()) })),
      t('library.duplicate.word'),
    );
  }

  /**
   * The summary of `binder` (SPEC §4.3): its notes that have bookmarks, in the binder's order with those of its sub-binders
   * after (`binderNotes`), each with its bookmarks in reading order.
   */
  binderSummary(binder: string): SummaryNote[] {
    return binderSummary(this.binderNotes(binder, true), binder);
  }

  /** Opens the summary of `binder`. */
  openSummary(binder: string): void {
    this.summaryBinder = binder;
  }

  closeSummary(): void {
    this.summaryBinder = null;
  }

  /** Opens « Changer la date… » on `note`. */
  openDate(note: NoteSummary): void {
    this.dateNote = note;
  }

  closeDate(): void {
    this.dateNote = null;
  }

  /**
   * Sets the creation date of `note` (Unix milliseconds) and offers to put the old one back. The error, if any, goes
   * back to the caller (the dialog shows it).
   */
  async setCreated(note: NoteSummary, created: number): Promise<void> {
    await this.activity.setCreated(note.path, displayTitle(note, currentLocale()), note.created, created, note.stamp);
  }

  /** Turns each note a quarter turn, as the editor does; offers to turn them back. */
  async turnNotes(notes: NoteSummary[], direction: TurnDirection): Promise<void> {
    this.error = null;
    await this.activity.turn(
      notes.map((note) => ({ path: note.path, title: displayTitle(note, currentLocale()) })),
      direction,
    );
  }

  // --- Binders (milestone M10) ----------------------------------------------------------------

  /**
   * Renames a binder; the view follows it. Returns its new path; the error (a name taken, say)
   * goes back to the caller, which shows it by the name field.
   */
  async renameBinder(binder: string, name: string): Promise<string> {
    this.error = null;
    try {
      const renamed = await api.renameBinder(binder, name.trim());
      const view = this.view;
      if (view.kind === 'binder' && isInBinder(view.binder, binder)) {
        this.view = { kind: 'binder', binder: renamed + view.binder.slice(binder.length) };
      }
      return renamed;
    } finally {
      await this.refresh();
    }
  }

  /** Deletes a binder: its notes go to the trash (restorable to it), then its empty folders go. */
  async deleteBinder(binder: string, name: string): Promise<void> {
    this.error = null;
    try {
      const done = await api.deleteBinder(binder);
      if (this.view.kind === 'binder' && isInBinder(this.view.binder, binder)) this.view = { kind: 'all' };
      await this.refresh();
      this.activity.binderDeleted(binder, name, done);
    } catch (error) {
      this.error = errorMessage(error);
      await this.refresh();
    }
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
