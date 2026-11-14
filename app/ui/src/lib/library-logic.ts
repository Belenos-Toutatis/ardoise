/** Pure helpers of the library screen: search, sort, binder tree, display text. */
import type { HandwritingHit } from './handwriting';
import type { BookmarkRef, NoteSummary, TagCount } from './note';

/** What the content area shows. */
export type LibraryView =
  | { kind: 'all' }
  | { kind: 'favorites' }
  | { kind: 'binder'; binder: string }
  | { kind: 'tag'; tag: string }
  | { kind: 'trash' };

/**
 * `binder`: the free order of a binder (`_ardoise-order.json`), only in the view of a binder; the other
 * views show the notes by date (`effectiveOrder` in `library-state.svelte.ts`).
 */
export type SortOrder = 'date-desc' | 'date-asc' | 'title-asc' | 'title-desc' | 'binder';

export const SORT_ORDERS: SortOrder[] = ['date-desc', 'date-asc', 'title-asc', 'title-desc', 'binder'];

/** The orders offered in `view`: the binder's own order only when a binder is shown. */
export function sortChoices(view: LibraryView): SortOrder[] {
  return SORT_ORDERS.filter((order) => order !== 'binder' || view.kind === 'binder');
}

/** What `order` gives in `view`: the free order outside a binder falls back to the newest first. */
export function orderIn(order: SortOrder, view: LibraryView): SortOrder {
  return order === 'binder' && view.kind !== 'binder' ? 'date-desc' : order;
}

/** Lower case, without accents (`'Réunion'` → `'reunion'`), for case- and accent-blind matching. */
export function normalize(text: string): string {
  return text.normalize('NFD').replace(/\p{M}+/gu, '').toLowerCase();
}

/** The words of a search query: lower case, without accents, split at white space. */
export function queryWords(query: string): string[] {
  return normalize(query).split(/\s+/).filter(Boolean);
}

/**
 * True when every word of `query` appears in the note's title, tags or binder, or in its
 * handwriting: `hit` holds the query words the desktop app found in the recognised text (where a
 * recognised word begins with them; see `handwriting/search.rs`).
 */
export function matchesSearch(note: NoteSummary, query: string, hit?: HandwritingHit | null): boolean {
  const words = queryWords(query);
  if (words.length === 0) return true;
  // The typed text of the note's text frames (`typedText`) and its bookmarks' labels (`bookmarks`), both from `note.json`,
  // are found like its title.
  const haystack = normalize(
    [note.title, ...note.tags, note.binder, note.typedText ?? '', ...(note.bookmarks ?? []).map((b) => b.label)].join('\n'),
  );
  return words.every((word) => haystack.includes(word) || (hit?.words.includes(word) ?? false));
}

/** The shown notes that the handwriting search found, each with its hit (in display order). */
export function handwritingMatches(
  notes: NoteSummary[],
  hits: ReadonlyMap<string, HandwritingHit>,
): { note: NoteSummary; hit: HandwritingHit }[] {
  return notes.flatMap((note) => {
    const hit = hits.get(note.path);
    return hit ? [{ note, hit }] : [];
  });
}

export function sameView(a: LibraryView, b: LibraryView): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'binder' && b.kind === 'binder') return a.binder === b.binder;
  if (a.kind === 'tag' && b.kind === 'tag') return a.tag === b.tag;
  return true;
}

/** True when `binder` is `ancestor` or lies inside it. The root (`''`) contains everything. */
export function isInBinder(binder: string, ancestor: string): boolean {
  return ancestor === '' || binder === ancestor || binder.startsWith(ancestor + '/');
}

/** Notes belonging to a view (a binder includes its sub-binders). `trash` has no notes. */
export function notesOfView(notes: NoteSummary[], view: LibraryView): NoteSummary[] {
  switch (view.kind) {
    case 'all':
      return notes;
    case 'favorites':
      return notes.filter((n) => n.favorite);
    case 'binder':
      return notes.filter((n) => isInBinder(n.binder, view.binder));
    case 'tag':
      return notes.filter((n) => n.tags.some((t) => sameTag(t, view.tag)));
    case 'trash':
      return [];
  }
}

/** Text a note is sorted by: its title, or its creation date (ISO) when untitled. */
function sortTitle(note: NoteSummary): string {
  return note.title.trim() || new Date(note.created).toISOString();
}

export function sortNotes(notes: NoteSummary[], order: SortOrder, locale: string): NoteSummary[] {
  const collator = new Intl.Collator(locale, { sensitivity: 'base', numeric: true });
  const sign = order.endsWith('desc') ? -1 : 1;
  const byDate = (a: NoteSummary, b: NoteSummary) => a.created - b.created || collator.compare(a.title, b.title);
  const byTitle = (a: NoteSummary, b: NoteSummary) =>
    collator.compare(sortTitle(a), sortTitle(b)) || a.created - b.created;
  const compare = order.startsWith('date') ? byDate : byTitle;
  return [...notes].sort((a, b) => sign * compare(a, b) || a.path.localeCompare(b.path));
}

/** What the free order of a binder is made from: the binders in the sidebar's order, and each one's listed ids. */
export interface BinderOrders {
  tree: BinderNode[];
  orders: Readonly<Record<string, readonly string[]>>;
}

/**
 * The notes shown for a view, search (with what the handwriting search found) and order. `binderOrders`
 * (the tree of binders and the orders of the listing) serves the order `binder` of a binder's view.
 */
export function visibleNotes(
  notes: NoteSummary[],
  view: LibraryView,
  query: string,
  order: SortOrder,
  locale: string,
  hits: ReadonlyMap<string, HandwritingHit> = new Map(),
  binderOrders?: BinderOrders,
): NoteSummary[] {
  const shown = notesOfView(notes, view).filter((n) => matchesSearch(n, query, hits.get(n.path)));
  if (order === 'binder' && view.kind === 'binder' && binderOrders) {
    return arrangeByBinderOrder(shown, view.binder, binderOrders, locale);
  }
  return sortNotes(shown, orderIn(order, view), locale);
}

// --- The free order of a binder -----------------------------------------------------------------

/** `binder` then every binder below it, in the order the sidebar shows them (a binder before its sub-binders). */
export function binderChain(tree: BinderNode[], binder: string): string[] {
  const find = (nodes: BinderNode[]): BinderNode | undefined => {
    for (const node of nodes) {
      const found = node.path === binder ? node : find(node.children);
      if (found) return found;
    }
    return undefined;
  };
  const chain: string[] = [];
  const walk = (node: BinderNode) => {
    chain.push(node.path);
    node.children.forEach(walk);
  };
  const node = find(tree);
  if (node) walk(node);
  else chain.push(binder);
  return chain;
}

/**
 * The notes of one binder in its own order: the notes whose id the order lists, as it lists them, then the
 * others (a note that arrived, a note the order does not know) oldest first, a copy after its original. An id of
 * the order that is no note of the binder (deleted, moved) is ignored.
 */
export function inBinderOrder(notes: NoteSummary[], ids: readonly string[] | undefined, locale: string): NoteSummary[] {
  const place = new Map<string, number>();
  (ids ?? []).forEach((id, index) => {
    if (!place.has(id)) place.set(id, index);
  });
  const listed = notes.filter((n) => place.has(n.id));
  const others = notes.filter((n) => !place.has(n.id));
  listed.sort((a, b) => (place.get(a.id) ?? 0) - (place.get(b.id) ?? 0) || a.path.localeCompare(b.path));
  return [...listed, ...sortNotes(others, 'date-asc', locale)];
}

/**
 * The notes of a binder's view in its free order: the notes of the binder itself in its order, then those of each
 * sub-binder in that sub-binder's order (the sub-binders in the sidebar's order). `shown` is what the view shows.
 */
export function arrangeByBinderOrder(
  shown: NoteSummary[],
  binder: string,
  { tree, orders }: BinderOrders,
  locale: string,
): NoteSummary[] {
  const chain = binderChain(tree, binder);
  const known = new Set(chain);
  const out: NoteSummary[] = [];
  for (const path of chain) {
    out.push(...inBinderOrder(shown.filter((n) => n.binder === path), orders[path], locale));
  }
  // A note of a binder the tree does not know (cannot happen: the tree holds every note's binder): by date, last.
  out.push(...sortNotes(shown.filter((n) => !known.has(n.binder)), 'date-asc', locale));
  return out;
}

// --- The summary of a binder (SPEC §4.3 « Sommaire du classeur ») ---------------------------------

/** One note of a binder's summary: the note, where it is below the summarised binder, and its bookmarks in reading order. */
export interface SummaryNote {
  note: NoteSummary;
  /** The binders it is in below the summarised one (none for a note of that binder itself). */
  below: string[];
  bookmarks: BookmarkRef[];
}

/**
 * The summary of `binder`: the notes of `notes` (the binder's notes in its order, those of its sub-binders after, what
 * `LibraryState.binderNotes` gives) that have bookmarks, each with them. The order of the notes is the one given; the
 * bookmarks of a note are in reading order (the listing's).
 */
export function binderSummary(notes: NoteSummary[], binder: string): SummaryNote[] {
  return notes
    .filter((note) => (note.bookmarks?.length ?? 0) > 0)
    .map((note) => ({ note, below: binderPathBelow(binder, note.binder), bookmarks: note.bookmarks ?? [] }));
}

/**
 * `summary` narrowed to the bookmarks whose label has every word of `query` (case and accents ignored); a note whose title
 * has them all keeps every bookmark. A blank query keeps everything.
 */
export function filterSummary(summary: SummaryNote[], query: string): SummaryNote[] {
  const words = queryWords(query);
  if (words.length === 0) return summary;
  const has = (text: string) => {
    const folded = normalize(text);
    return words.every((word) => folded.includes(word));
  };
  return summary.flatMap((entry) => {
    if (has(entry.note.title)) return [entry];
    const bookmarks = entry.bookmarks.filter((b) => has(b.label));
    return bookmarks.length > 0 ? [{ ...entry, bookmarks }] : [];
  });
}

/** The binders a note is in below the exported binder `binder` (none for a note of that binder itself). */
export function binderPathBelow(binder: string, noteBinder: string): string[] {
  const below = noteBinder === binder ? '' : noteBinder.slice(binder === '' ? 0 : binder.length + 1);
  return below.split('/').filter(Boolean);
}

/** A place between two cards of the grid: just before or just after the card at `path`. */
export interface DropSpot {
  path: string;
  side: 'before' | 'after';
}

/** The new order of one binder: its notes' ids, first to last. */
export interface OrderChange {
  binder: string;
  ids: string[];
  /** Where the first moved note stands in the binder now (1 = first) and how many notes it holds, to say it aloud. */
  position: number;
  count: number;
}

/** Cards of one binder in `displayed`, as ids. */
function idsOf(displayed: NoteSummary[], binder: string): string[] {
  return displayed.filter((n) => n.binder === binder).map((n) => n.id);
}

/** The binder all of `moved` (paths) belong to, and those notes in the grid's order; null for none or several binders. */
function movedOf(displayed: NoteSummary[], moved: readonly string[]): { binder: string; notes: NoteSummary[] } | null {
  const wanted = new Set(moved);
  const notes = displayed.filter((n) => wanted.has(n.path));
  if (notes.length === 0 || notes.some((n) => n.binder !== notes[0].binder)) return null;
  return { binder: notes[0].binder, notes };
}

function change(binder: string, ids: string[], before: string[], movedIds: string[]): OrderChange | null {
  if (ids.length === before.length && ids.every((id, i) => id === before[i])) return null;
  return { binder, ids, position: ids.indexOf(movedIds[0]) + 1, count: ids.length };
}

/**
 * The new order when the cards `moved` are dropped at `spot` of the grid `displayed`: those of one binder move
 * among the notes of that binder, keeping their relative order, next to the nearest card of their binder on
 * the side of the spot (the grid of a parent binder shows its own notes, then each sub-binder's: a drop is
 * refused where no card of the moved notes' binder is next to it). Null when it is refused or changes nothing.
 */
export function moveToSpot(displayed: NoteSummary[], moved: readonly string[], spot: DropSpot): OrderChange | null {
  const group = movedOf(displayed, moved);
  const at = displayed.findIndex((n) => n.path === spot.path);
  if (!group || at < 0) return null;
  const gap = spot.side === 'before' ? at : at + 1;
  const wanted = new Set(group.notes.map((n) => n.path));
  const usable = (note: NoteSummary | undefined) => (note && note.binder === group.binder && !wanted.has(note.path) ? note : undefined);
  const next = usable(displayed[gap]);
  const previous = usable(displayed[gap - 1]);
  const anchor = next ?? previous;
  if (!anchor) return null;
  const before = idsOf(displayed, group.binder);
  const movedIds = group.notes.map((n) => n.id);
  const rest = before.filter((id) => !movedIds.includes(id));
  const index = rest.indexOf(anchor.id) + (anchor === next ? 0 : 1);
  const ids = [...rest.slice(0, index), ...movedIds, ...rest.slice(index)];
  return change(group.binder, ids, before, movedIds);
}

/** How far a keyboard move goes: one place earlier or later, or to the start or the end of the binder. */
export type MoveStep = 'earlier' | 'later' | 'start' | 'end';

/**
 * The new order when the cards `moved` go one place earlier or later among the notes of their binder, or to its
 * start or end; they keep their relative order. Null when they are not all in one binder, or already there.
 */
export function moveByStep(displayed: NoteSummary[], moved: readonly string[], step: MoveStep): OrderChange | null {
  const group = movedOf(displayed, moved);
  if (!group) return null;
  const before = idsOf(displayed, group.binder);
  const movedIds = group.notes.map((n) => n.id);
  const isMoved = new Set(movedIds);
  let ids = [...before];
  if (step === 'start' || step === 'end') {
    const rest = before.filter((id) => !isMoved.has(id));
    ids = step === 'start' ? [...movedIds, ...rest] : [...rest, ...movedIds];
  } else if (step === 'earlier') {
    for (let i = 1; i < ids.length; i++) {
      if (isMoved.has(ids[i]) && !isMoved.has(ids[i - 1])) [ids[i - 1], ids[i]] = [ids[i], ids[i - 1]];
    }
  } else {
    for (let i = ids.length - 2; i >= 0; i--) {
      if (isMoved.has(ids[i]) && !isMoved.has(ids[i + 1])) [ids[i + 1], ids[i]] = [ids[i], ids[i + 1]];
    }
  }
  return change(group.binder, ids, before, movedIds);
}

export interface BinderNode {
  /** Full relative path, `/`-separated. */
  path: string;
  /** Last segment. */
  name: string;
  depth: number;
  /** Notes in this binder and below. */
  count: number;
  children: BinderNode[];
}

/** Builds the tree of binders with note counts (own notes plus those of sub-binders). */
export function buildBinderTree(binders: string[], notes: NoteSummary[]): BinderNode[] {
  const nodes = new Map<string, BinderNode>();
  const roots: BinderNode[] = [];
  const ensure = (path: string): BinderNode => {
    const found = nodes.get(path);
    if (found) return found;
    const segments = path.split('/');
    const node: BinderNode = {
      path,
      name: segments[segments.length - 1],
      depth: segments.length - 1,
      count: 0,
      children: [],
    };
    nodes.set(path, node);
    if (segments.length === 1) roots.push(node);
    else ensure(segments.slice(0, -1).join('/')).children.push(node);
    return node;
  };
  for (const binder of binders.filter(Boolean)) ensure(binder);
  for (const note of notes) {
    if (!note.binder) continue;
    ensure(note.binder);
    const segments = note.binder.split('/');
    for (let i = 1; i <= segments.length; i++) {
      const node = nodes.get(segments.slice(0, i).join('/'));
      if (node) node.count += 1;
    }
  }
  const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });
  const sortLevel = (level: BinderNode[]) => {
    level.sort((a, b) => collator.compare(a.name, b.name));
    level.forEach((node) => sortLevel(node.children));
  };
  sortLevel(roots);
  return roots;
}

/** Every binder of the tree in display order, flattened (for pickers). */
export function flattenTree(tree: BinderNode[]): BinderNode[] {
  return tree.flatMap((node) => [node, ...flattenTree(node.children)]);
}

/** `'A/B/C'` → `['A', 'A/B']`. */
export function ancestorsOf(binder: string): string[] {
  const segments = binder.split('/').filter(Boolean);
  return segments.slice(0, -1).map((_, i) => segments.slice(0, i + 1).join('/'));
}

/** Last file name of a path with either separator. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** Folder part of a trash entry's `original` (a file path or already a folder). */
export function originalBinder(original: string): string {
  const normalized = original.replace(/\\/g, '/');
  if (!normalized.toLowerCase().endsWith('.ardoise')) return normalized;
  return normalized.split('/').slice(0, -1).join('/');
}

export function formatDate(ms: number, locale: string, withTime = true): string {
  const options: Intl.DateTimeFormatOptions = withTime
    ? { dateStyle: 'medium', timeStyle: 'short' }
    : { dateStyle: 'medium' };
  return new Intl.DateTimeFormat(locale, options).format(new Date(ms));
}

/** A note's title, or its date when it has none. */
export function displayTitle(note: { title: string; created: number }, locale: string): string {
  return note.title.trim() || formatDate(note.created, locale);
}

/**
 * The names `notes` go by in a binder's summary or in the bookmarks of its PDF, by path. With `withDates` an untitled
 * note is named by its date (`displayTitle`). Without, it is `untitled`, then `untitled (2)`, `untitled (3)`… when
 * several of `notes` have no title (the numbers follow the order given), so that two bookmarks never read alike.
 */
export function listedTitles(
  notes: readonly { path: string; title: string; created: number }[],
  locale: string,
  withDates: boolean,
  untitled: string,
): Map<string, string> {
  const names = new Map<string, string>();
  let blanks = 0;
  for (const note of notes) {
    if (note.title.trim() || withDates) {
      names.set(note.path, displayTitle(note, locale));
    } else {
      blanks += 1;
      names.set(note.path, blanks === 1 ? untitled : `${untitled} (${blanks})`);
    }
  }
  return names;
}

/** The notes oldest first (by creation date, then by path), as an assembly puts them. */
export function oldestFirst(notes: NoteSummary[]): NoteSummary[] {
  return [...notes].sort((a, b) => a.created - b.created || a.path.localeCompare(b.path));
}

/** Tags of `notes` with how many of the notes carry each. */
export function tagsOfNotes(notes: NoteSummary[]): TagCount[] {
  const counts = new Map<string, number>();
  for (const note of notes) for (const tag of note.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  return [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => a.tag.localeCompare(b.tag));
}

/** Splits user input such as `a, b ; c` into trimmed, non-empty, unique tags. */
export function parseTags(text: string): string[] {
  return [...new Set(text.split(/[,;]/).map((s) => s.trim()).filter(Boolean))];
}

/** Tags differing only by case are the same tag (the Rust listing merges them that way). */
export function sameTag(a: string, b: string): boolean {
  return a.toLocaleLowerCase() === b.toLocaleLowerCase();
}

/** `tags` then `added`, without case-insensitive duplicates (the first spelling wins). */
export function mergeTags(tags: string[], added: string[]): string[] {
  const merged: string[] = [];
  for (const tag of [...tags, ...added]) {
    if (!merged.some((t) => sameTag(t, tag))) merged.push(tag);
  }
  return merged;
}
