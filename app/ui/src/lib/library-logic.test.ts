import { describe, expect, it } from 'vitest';
import {
  ancestorsOf,
  buildBinderTree,
  flattenTree,
  isInBinder,
  matchesSearch,
  normalize,
  notesOfView,
  oldestFirst,
  originalBinder,
  parseTags,
  mergeTags,
  sameTag,
  sortNotes,
  tagsOfNotes,
  visibleNotes,
} from './library-logic';
import type { NoteSummary } from './note';

const stamp = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function note(partial: Partial<NoteSummary> & { path: string }): NoteSummary {
  return {
    binder: '',
    fileName: partial.path,
    id: partial.path,
    title: '',
    created: 0,
    modified: 0,
    tags: [],
    favorite: false,
    stamp,
    conflict: null,
    ...partial,
  };
}

const notes = [
  note({ path: 'a', title: 'Réunion parents', created: 300, tags: ['école'], binder: 'Perso' }),
  note({ path: 'b', title: 'cours de physique', created: 100, tags: ['Physique'], binder: 'Classe/3e B', favorite: true }),
  note({ path: 'c', title: 'Zèbre', created: 200 }),
  note({ path: 'd', title: '', created: 400, binder: 'Classe' }),
];

describe('search', () => {
  it('ignores case and accents', () => {
    expect(normalize('Réunion ÉCOLE')).toBe('reunion ecole');
    expect(matchesSearch(notes[0], 'reunion')).toBe(true);
    expect(matchesSearch(notes[0], 'RÉUNION')).toBe(true);
    expect(matchesSearch(notes[0], 'ecole')).toBe(true);
    expect(matchesSearch(notes[2], 'zebre')).toBe(true);
    expect(matchesSearch(notes[2], 'reunion')).toBe(false);
  });

  it('needs every word, in any order, in the title, tags or binder', () => {
    expect(matchesSearch(notes[1], 'physique cours')).toBe(true);
    expect(matchesSearch(notes[1], 'physique 3e')).toBe(true);
    expect(matchesSearch(notes[1], 'physique chimie')).toBe(false);
    expect(matchesSearch(notes[1], '   ')).toBe(true);
  });
});

describe('views', () => {
  it('filters favourites, tags and binders (a binder includes its sub-binders)', () => {
    expect(notesOfView(notes, { kind: 'favorites' }).map((n) => n.path)).toEqual(['b']);
    expect(notesOfView(notes, { kind: 'tag', tag: 'école' }).map((n) => n.path)).toEqual(['a']);
    expect(notesOfView(notes, { kind: 'binder', binder: 'Classe' }).map((n) => n.path)).toEqual(['b', 'd']);
    expect(notesOfView(notes, { kind: 'binder', binder: 'Classe/3e B' }).map((n) => n.path)).toEqual(['b']);
    expect(notesOfView(notes, { kind: 'trash' })).toEqual([]);
    expect(isInBinder('Classes', 'Classe')).toBe(false);
  });
});

describe('sort', () => {
  it('sorts by date in both directions', () => {
    expect(sortNotes(notes, 'date-asc', 'fr').map((n) => n.path)).toEqual(['b', 'c', 'a', 'd']);
    expect(sortNotes(notes, 'date-desc', 'fr').map((n) => n.path)).toEqual(['d', 'a', 'c', 'b']);
  });

  it('sorts by title without caring for case or accents; untitled notes sort by date text', () => {
    expect(sortNotes(notes, 'title-asc', 'fr').map((n) => n.path)).toEqual(['d', 'b', 'a', 'c']);
    expect(sortNotes(notes, 'title-desc', 'fr').map((n) => n.path)).toEqual(['c', 'a', 'b', 'd']);
  });

  it('combines view, search and order', () => {
    const shown = visibleNotes(notes, { kind: 'binder', binder: 'Classe' }, 'physique', 'title-asc', 'en');
    expect(shown.map((n) => n.path)).toEqual(['b']);
  });
});

describe('binder tree', () => {
  it('nests binders, counts notes including sub-binders and sorts siblings', () => {
    const tree = buildBinderTree(['Perso', 'Classe', 'Classe/3e B', 'Classe/2de'], notes);
    expect(tree.map((n) => [n.path, n.count])).toEqual([
      ['Classe', 2],
      ['Perso', 1],
    ]);
    expect(tree[0].children.map((n) => [n.name, n.depth, n.count])).toEqual([
      ['2de', 1, 0],
      ['3e B', 1, 1],
    ]);
    expect(flattenTree(tree).map((n) => n.path)).toEqual(['Classe', 'Classe/2de', 'Classe/3e B', 'Perso']);
  });

  it('creates the parents of a binder only known through a note', () => {
    const tree = buildBinderTree([], [note({ path: 'x', binder: 'A/B' })]);
    expect(tree[0].path).toBe('A');
    expect(tree[0].children[0].path).toBe('A/B');
    expect(tree[0].count).toBe(1);
  });
});

describe('small helpers', () => {
  it('lists ancestors', () => {
    expect(ancestorsOf('A/B/C')).toEqual(['A', 'A/B']);
    expect(ancestorsOf('A')).toEqual([]);
  });

  it('finds the folder of a trash entry whether it holds a file path or a folder', () => {
    expect(originalBinder('Classe/3e B/2026-10-20 Cours.ardoise')).toBe('Classe/3e B');
    expect(originalBinder('Classe\\2026-10-20 Cours.ardoise')).toBe('Classe');
    expect(originalBinder('2026-10-20 Cours.ardoise')).toBe('');
    expect(originalBinder('Perso')).toBe('Perso');
  });

  it('parses and counts tags', () => {
    expect(parseTags(' a, b ;a,, c ')).toEqual(['a', 'b', 'c']);
    expect(tagsOfNotes(notes).map((t) => t.tag)).toEqual(['école', 'Physique']);
  });
});

describe('tags ignore case', () => {
  it('filters, merges and compares tags whatever their case', () => {
    expect(sameTag('Physique', 'physique')).toBe(true);
    expect(sameTag('Physique', 'chimie')).toBe(false);
    expect(mergeTags(['physique'], ['Physique', 'chimie'])).toEqual(['physique', 'chimie']);
  });
});

describe('assembly order', () => {
  it('puts the oldest note first, breaking ties by path, without touching the input', () => {
    const tie = [note({ path: 'z', created: 100 }), note({ path: 'y', created: 100 }), note({ path: 'x', created: 50 })];
    expect(oldestFirst(tie).map((n) => n.path)).toEqual(['x', 'y', 'z']);
    expect(tie.map((n) => n.path)).toEqual(['z', 'y', 'x']);
  });
});
