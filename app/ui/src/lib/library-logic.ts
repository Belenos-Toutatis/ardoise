/** Pure helpers of the library screen: search, sort, binder tree, display text. */
import type { NoteSummary, TagCount } from './note';

/** What the content area shows. */
export type LibraryView =
  | { kind: 'all' }
  | { kind: 'favorites' }
  | { kind: 'binder'; binder: string }
  | { kind: 'tag'; tag: string }
  | { kind: 'trash' };

export type SortOrder = 'date-desc' | 'date-asc' | 'title-asc' | 'title-desc';

export const SORT_ORDERS: SortOrder[] = ['date-desc', 'date-asc', 'title-asc', 'title-desc'];

/** Lower case, without accents (`'Réunion'` → `'reunion'`), for case- and accent-blind matching. */
export function normalize(text: string): string {
  return text.normalize('NFD').replace(/\p{M}+/gu, '').toLowerCase();
}

/** True when every word of `query` appears in the note's title, tags or binder. */
export function matchesSearch(note: NoteSummary, query: string): boolean {
  const words = normalize(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const haystack = normalize([note.title, ...note.tags, note.binder].join('\n'));
  return words.every((word) => haystack.includes(word));
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
      return notes.filter((n) => n.tags.includes(view.tag));
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

/** The notes shown for a view, search and order. */
export function visibleNotes(
  notes: NoteSummary[],
  view: LibraryView,
  query: string,
  order: SortOrder,
  locale: string,
): NoteSummary[] {
  return sortNotes(
    notesOfView(notes, view).filter((n) => matchesSearch(n, query)),
    order,
    locale,
  );
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
