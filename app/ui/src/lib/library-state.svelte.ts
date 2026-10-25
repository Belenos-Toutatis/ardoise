/**
 * State and actions of the library screen: the listing, the current view, search, order,
 * selection, and every operation on notes (each followed by a refresh). Components read it and
 * call its methods; the pure parts are in `library-logic.ts`.
 */
import { api, errorMessage, onLibraryChanged } from './api';
import { dragOutSupported } from './drag-out';
import { currentLocale, t } from './i18n.svelte';
import {
  buildBinderTree,
  displayTitle,
  mergeTags,
  oldestFirst,
  parseTags,
  sameTag,
  sameView,
  visibleNotes,
  type LibraryView,
  type SortOrder,
} from './library-logic';
import type { FileStamp, LibraryListing, MergeLayout, NoteSummary, TrashEntry } from './note';

const THUMBNAIL_CACHE_SIZE = 400;

export class LibraryState {
  listing = $state<LibraryListing | null>(null);
  trash = $state<TrashEntry[]>([]);
  /** Message of the last failed operation, shown in a banner. */
  error = $state<string | null>(null);
  view = $state<LibraryView>({ kind: 'all' });
  query = $state('');
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
  dialog = $state<{ kind: 'export' | 'assemble'; notes: NoteSummary[] } | null>(null);

  /** Paths being dragged; kept here because a drag event's data is not readable until the drop. */
  private dragPaths: string[] = [];
  private anchor: string | null = null;
  private sequence = 0;
  private thumbnails = new Map<string, Promise<string | null>>();

  readonly notes = $derived(this.listing?.notes ?? []);
  readonly tree = $derived(buildBinderTree(this.listing?.binders ?? [], this.notes));
  readonly visible = $derived(visibleNotes(this.notes, this.view, this.query, this.order, currentLocale()));
  readonly selectedNotes = $derived(this.notes.filter((n) => this.selected.includes(n.path)));
  /** The selected notes as the grid shows them; those the search hides come last. */
  readonly selectedDisplayed = $derived.by(() => {
    const shown = this.visible.filter((n) => this.selected.includes(n.path));
    const rest = this.selectedNotes.filter((n) => !shown.includes(n));
    return [...shown, ...rest];
  });

  // --- Loading -------------------------------------------------------------------------------

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
    return () => {
      stopped = true;
      unlisten?.();
    };
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

  /** Opens the export or assemble dialog over the notes selected now. */
  openDialog(kind: 'export' | 'assemble'): void {
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
    await this.run(() => api.trashNotes(paths));
    this.selected = this.selected.filter((p) => !paths.includes(p));
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
