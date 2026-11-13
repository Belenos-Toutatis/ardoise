// The free order of a binder (Q20): what the pure functions of `library-logic.ts` decide.
import { describe, expect, it } from 'vitest';
import {
  arrangeByBinderOrder,
  binderChain,
  binderPathBelow,
  buildBinderTree,
  inBinderOrder,
  moveByStep,
  moveToSpot,
  orderIn,
  sortChoices,
  visibleNotes,
  type BinderOrders,
  type LibraryView,
} from './library-logic';
import { copyTitle } from './mock-library';
import type { NoteSummary } from './note';

const STAMP = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function note(id: string, binder: string, created = 1_000, title = id): NoteSummary {
  return {
    path: `C:\\lib\\${binder}\\${id}.ardoise`,
    binder,
    fileName: `${id}.ardoise`,
    id,
    title,
    created,
    modified: created,
    tags: [],
    favorite: false,
    stamp: STAMP,
    conflict: null,
  };
}

const ids = (notes: NoteSummary[]) => notes.map((n) => n.id);
const paths = (...notes: NoteSummary[]) => notes.map((n) => n.path);

describe('the copy title of the browser stand-in follows the Rust library', () => {
  it('numbers the copy of a copy and gives an untitled note a copy named with the copy word only', () => {
    expect(copyTitle('Cours', 'copie')).toBe('Cours (copie)');
    expect(copyTitle('  Cours  ', 'copie')).toBe('Cours (copie)');
    expect(copyTitle('', 'copie')).toBe('(copie)');
    expect(copyTitle('Cours (copie)', 'copie')).toBe('Cours (copie 2)');
    expect(copyTitle('Cours (copie 2)', 'copie')).toBe('Cours (copie 3)');
    expect(copyTitle('(copie)', 'copie')).toBe('(copie 2)');
    expect(copyTitle('Cours (2)', 'copie')).toBe('Cours (2) (copie)');
    expect(copyTitle('Cours (copie 1)', 'copie')).toBe('Cours (copie 1) (copie)');
    expect(copyTitle('Cours (copy)', 'copy')).toBe('Cours (copy 2)');
  });
});

describe('the free order of a binder: the sort choice', () => {
  const binder: LibraryView = { kind: 'binder', binder: 'Classe' };
  it('is offered in a binder view only, and the other views fall back to the newest first', () => {
    expect(sortChoices(binder)).toEqual(['date-desc', 'date-asc', 'title-asc', 'title-desc', 'binder']);
    for (const view of [{ kind: 'all' }, { kind: 'favorites' }, { kind: 'tag', tag: 'x' }, { kind: 'trash' }] as LibraryView[]) {
      expect(sortChoices(view)).not.toContain('binder');
      expect(orderIn('binder', view)).toBe('date-desc');
    }
    expect(orderIn('binder', binder)).toBe('binder');
    expect(orderIn('title-asc', { kind: 'all' })).toBe('title-asc');
  });

  it('shows a view by date when the free order is chosen outside a binder', () => {
    const notes = [note('a', 'Classe', 1), note('b', 'Classe', 3), note('c', '', 2)];
    const shown = visibleNotes(notes, { kind: 'all' }, '', 'binder', 'fr', new Map(), {
      tree: buildBinderTree(['Classe'], notes),
      orders: { Classe: ['a', 'b'] },
    });
    expect(ids(shown)).toEqual(['b', 'c', 'a']);
  });
});

describe('the free order of a binder: the order of its notes', () => {
  it('puts the listed notes first, as listed, then the others oldest first', () => {
    const notes = [note('a', 'X', 5), note('b', 'X', 1), note('c', 'X', 9), note('d', 'X', 3)];
    expect(ids(inBinderOrder(notes, ['c', 'a'], 'fr'))).toEqual(['c', 'a', 'b', 'd']);
    // No order: by date, oldest first.
    expect(ids(inBinderOrder(notes, undefined, 'fr'))).toEqual(['b', 'd', 'a', 'c']);
    expect(ids(inBinderOrder(notes, [], 'fr'))).toEqual(['b', 'd', 'a', 'c']);
  });

  it('ignores ids that are no note of the binder and keeps the first place of a repeated id', () => {
    const notes = [note('a', 'X', 1), note('b', 'X', 2)];
    expect(ids(inBinderOrder(notes, ['gone', 'b', 'other', 'a', 'b'], 'fr'))).toEqual(['b', 'a']);
  });

  it('puts a copy right after its original: same date, the title tells them apart', () => {
    const notes = [note('o', 'X', 5, 'Cours'), note('z', 'X', 9, 'Zèbre'), note('c', 'X', 5, 'Cours (copie)'), note('a', 'X', 1, 'Avant')];
    expect(ids(inBinderOrder(notes, undefined, 'fr'))).toEqual(['a', 'o', 'c', 'z']);
  });

  it('keeps a conflict copy of a note (same id) next to it, by path', () => {
    const a = note('a', 'X', 1);
    const copy = { ...note('a', 'X', 1), path: 'C:\\lib\\X\\a (conflicted copy).ardoise' };
    const b = note('b', 'X', 2);
    expect(inBinderOrder([b, copy, a], ['a', 'b'], 'fr').map((n) => n.id)).toEqual(['a', 'a', 'b']);
  });
});

describe('the free order of a binder: a parent shows its sub-binders after its own notes', () => {
  // Sidebar order: Classe, then 3e A, then 3e B (with Devoirs), by name.
  const notes = [
    note('p1', 'Classe', 1),
    note('p2', 'Classe', 2),
    note('b1', 'Classe/3e B', 3),
    note('d1', 'Classe/3e B/Devoirs', 4),
    note('a1', 'Classe/3e A', 5),
    note('a2', 'Classe/3e A', 6),
    note('other', 'Perso', 7),
  ];
  const tree = buildBinderTree(['Classe', 'Classe/3e A', 'Classe/3e B', 'Classe/3e B/Devoirs', 'Perso'], notes);
  const orders = (extra: BinderOrders['orders'] = {}): BinderOrders => ({ tree, orders: extra });

  it('follows the sidebar for the chain of binders', () => {
    expect(binderChain(tree, 'Classe')).toEqual(['Classe', 'Classe/3e A', 'Classe/3e B', 'Classe/3e B/Devoirs']);
    expect(binderChain(tree, 'Classe/3e B')).toEqual(['Classe/3e B', 'Classe/3e B/Devoirs']);
    expect(binderChain(tree, 'Perso')).toEqual(['Perso']);
    // A binder the tree does not know is alone.
    expect(binderChain(tree, 'Inconnu')).toEqual(['Inconnu']);
  });

  it('shows its own notes first, then each sub-binder in that sub-binder own order', () => {
    const view: LibraryView = { kind: 'binder', binder: 'Classe' };
    const shown = visibleNotes(notes, view, '', 'binder', 'fr', new Map(), orders({ Classe: ['p2', 'p1'], 'Classe/3e A': ['a2', 'a1'] }));
    expect(ids(shown)).toEqual(['p2', 'p1', 'a2', 'a1', 'b1', 'd1']);
    // Without any order: each binder by date.
    expect(ids(visibleNotes(notes, view, '', 'binder', 'fr', new Map(), orders()))).toEqual(['p1', 'p2', 'a1', 'a2', 'b1', 'd1']);
    // A sub-binder's view is its own subtree.
    expect(ids(visibleNotes(notes, { kind: 'binder', binder: 'Classe/3e B' }, '', 'binder', 'fr', new Map(), orders()))).toEqual(['b1', 'd1']);
  });

  it('keeps the order of the notes the search leaves', () => {
    const view: LibraryView = { kind: 'binder', binder: 'Classe' };
    const found = visibleNotes(notes, view, 'p', 'binder', 'fr', new Map(), orders({ Classe: ['p2', 'p1'] }));
    expect(ids(found)).toEqual(['p2', 'p1']);
  });

  it('puts a note that arrived in a binder at its end', () => {
    const arrived = [...notes, note('new', 'Classe', 99)];
    const shown = arrangeByBinderOrder(arrived.filter((n) => n.binder === 'Classe'), 'Classe', orders({ Classe: ['p2', 'p1'] }), 'fr');
    expect(ids(shown)).toEqual(['p2', 'p1', 'new']);
  });

  it('says which binders a note is in below the exported one', () => {
    expect(binderPathBelow('Classe', 'Classe')).toEqual([]);
    expect(binderPathBelow('Classe', 'Classe/3e B')).toEqual(['3e B']);
    expect(binderPathBelow('Classe', 'Classe/3e B/Devoirs')).toEqual(['3e B', 'Devoirs']);
    expect(binderPathBelow('', 'Perso')).toEqual(['Perso']);
  });
});

describe('moving cards by the keyboard', () => {
  const a = note('a', 'X'), b = note('b', 'X'), c = note('c', 'X'), d = note('d', 'X');
  const displayed = [a, b, c, d];

  it('moves a card one place earlier or later and says where it is now', () => {
    expect(moveByStep(displayed, paths(c), 'earlier')).toEqual({ binder: 'X', ids: ['a', 'c', 'b', 'd'], position: 2, count: 4 });
    expect(moveByStep(displayed, paths(b), 'later')).toEqual({ binder: 'X', ids: ['a', 'c', 'b', 'd'], position: 3, count: 4 });
  });

  it('moves a card to the start or the end', () => {
    expect(moveByStep(displayed, paths(c), 'start')?.ids).toEqual(['c', 'a', 'b', 'd']);
    expect(moveByStep(displayed, paths(b), 'end')?.ids).toEqual(['a', 'c', 'd', 'b']);
  });

  it('changes nothing at the ends', () => {
    expect(moveByStep(displayed, paths(a), 'earlier')).toBeNull();
    expect(moveByStep(displayed, paths(d), 'later')).toBeNull();
    expect(moveByStep(displayed, paths(a), 'start')).toBeNull();
    expect(moveByStep(displayed, paths(d), 'end')).toBeNull();
  });

  it('moves several selected cards together, keeping their relative order', () => {
    expect(moveByStep(displayed, paths(b, d), 'earlier')?.ids).toEqual(['b', 'a', 'd', 'c']);
    expect(moveByStep(displayed, paths(a, c), 'later')?.ids).toEqual(['b', 'a', 'd', 'c']);
    expect(moveByStep(displayed, paths(d, b), 'start')?.ids).toEqual(['b', 'd', 'a', 'c']);
    // Already together at the start: nothing.
    expect(moveByStep(displayed, paths(a, b), 'earlier')).toBeNull();
  });

  it('moves among the notes of the card own binder in the grid of a parent binder', () => {
    const own = [note('p1', 'P'), note('p2', 'P')];
    const sub = [note('s1', 'P/S'), note('s2', 'P/S')];
    const grid = [...own, ...sub];
    expect(moveByStep(grid, paths(sub[1]), 'earlier')).toEqual({ binder: 'P/S', ids: ['s2', 's1'], position: 1, count: 2 });
    // The first card of a sub-binder cannot go "earlier" into the parent's notes.
    expect(moveByStep(grid, paths(sub[0]), 'earlier')).toBeNull();
    expect(moveByStep(grid, paths(own[1]), 'later')).toBeNull();
    expect(moveByStep(grid, paths(own[0]), 'later')?.ids).toEqual(['p2', 'p1']);
  });

  it('refuses cards of several binders and unknown cards', () => {
    const grid = [note('p1', 'P'), note('s1', 'P/S')];
    expect(moveByStep(grid, paths(grid[0], grid[1]), 'later')).toBeNull();
    expect(moveByStep(grid, ['nope'], 'later')).toBeNull();
    expect(moveByStep(grid, [], 'later')).toBeNull();
  });
});

describe('dropping cards between two cards', () => {
  const a = note('a', 'X'), b = note('b', 'X'), c = note('c', 'X'), d = note('d', 'X');
  const displayed = [a, b, c, d];
  const drop = (moved: NoteSummary[], on: NoteSummary, side: 'before' | 'after') => moveToSpot(displayed, paths(...moved), { path: on.path, side });

  it('lands before or after the card the spot is on', () => {
    expect(drop([d], b, 'before')).toEqual({ binder: 'X', ids: ['a', 'd', 'b', 'c'], position: 2, count: 4 });
    expect(drop([a], c, 'after')).toEqual({ binder: 'X', ids: ['b', 'c', 'a', 'd'], position: 3, count: 4 });
    expect(drop([b], a, 'before')?.ids).toEqual(['b', 'a', 'c', 'd']);
    expect(drop([c], d, 'after')?.ids).toEqual(['a', 'b', 'd', 'c']);
  });

  it('moves several cards together, in their own order, wherever the spot is', () => {
    expect(drop([d, b], c, 'before')?.ids).toEqual(['a', 'b', 'd', 'c']);
    expect(drop([a, c], d, 'after')?.ids).toEqual(['b', 'd', 'a', 'c']);
    expect(drop([b, c], a, 'before')?.ids).toEqual(['b', 'c', 'a', 'd']);
  });

  it('changes nothing when the cards land where they are', () => {
    expect(drop([b], b, 'before')).toBeNull();
    expect(drop([b], b, 'after')).toBeNull();
    expect(drop([b], a, 'after')).toBeNull();
    expect(drop([b], c, 'before')).toBeNull();
    expect(drop([a, b], c, 'before')).toBeNull();
  });

  it('refuses a spot that is not a card of the grid', () => {
    expect(moveToSpot(displayed, paths(a), { path: 'C:\\nowhere', side: 'before' })).toBeNull();
    expect(moveToSpot(displayed, [], { path: b.path, side: 'before' })).toBeNull();
  });

  describe('in the grid of a parent binder', () => {
    const own = [note('p1', 'P'), note('p2', 'P'), note('p3', 'P')];
    const sub = [note('s1', 'P/S'), note('s2', 'P/S')];
    const other = [note('t1', 'P/T')];
    const grid = [...own, ...sub, ...other];
    const dropOn = (moved: NoteSummary[], on: NoteSummary, side: 'before' | 'after') => moveToSpot(grid, paths(...moved), { path: on.path, side });

    it('moves a card among those of its own binder only', () => {
      expect(dropOn([own[2]], own[0], 'before')?.ids).toEqual(['p3', 'p1', 'p2']);
      expect(dropOn([sub[1]], sub[0], 'before')).toEqual({ binder: 'P/S', ids: ['s2', 's1'], position: 1, count: 2 });
    });

    it('lets a card go to the end of its binder from the first card of the next one', () => {
      // The gap between p3 and s1: p3 is a card of the binder, so the card goes after it.
      expect(dropOn([own[0]], sub[0], 'before')?.ids).toEqual(['p2', 'p3', 'p1']);
      // And to the start of its binder from the last card of the one before.
      expect(dropOn([sub[1]], own[2], 'after')?.ids).toEqual(['s2', 's1']);
    });

    it('refuses a spot with no card of the moved notes binder next to it', () => {
      expect(dropOn([own[0]], other[0], 'after')).toBeNull();
      expect(dropOn([own[0]], other[0], 'before')).toBeNull();
      expect(dropOn([other[0]], own[1], 'before')).toBeNull();
    });

    it('refuses cards of several binders', () => {
      expect(dropOn([own[0], sub[0]], own[2], 'after')).toBeNull();
    });
  });
});
