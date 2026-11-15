// Undoing a move of notes to another binder: what the free order of the source binder held, and where the notes go back.
import { describe, expect, it } from 'vitest';
import { fileNameOf, placeBack, rememberOrders, withoutIds } from './library-move';
import type { NoteSummary } from './note';

const STAMP = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function note(id: string, binder: string, created = 1_000): NoteSummary {
  return {
    path: `C:\\lib\\${binder}\\${id}.ardoise`,
    binder,
    fileName: `${id}.ardoise`,
    id,
    title: id,
    created,
    modified: created,
    tags: [],
    favorite: false,
    stamp: STAMP,
    conflict: null,
  };
}

const L = 'fr';
/** Notes a, b, c, d, e of `Classe`, oldest first. */
const classe = ['a', 'b', 'c', 'd', 'e'].map((id, i) => note(id, 'Classe', 1_000 + i));

describe('rememberOrders', () => {
  it('remembers the order of each source binder that has one, as the grid shows it', () => {
    const all = [...classe, note('p', 'Perso')];
    // `e` is not listed: it comes after the listed notes. `zz` is a note that is gone.
    const memory = rememberOrders([classe[1], all[5]], all, { Classe: ['d', 'zz', 'b', 'a', 'c'] }, L);
    expect([...memory.keys()]).toEqual(['Classe']);
    expect(memory.get('Classe')).toEqual({ listed: ['d', 'zz', 'b', 'a', 'c'], shown: ['d', 'b', 'a', 'c', 'e'] });
  });

  it('remembers nothing for binders without a free order', () => {
    expect(rememberOrders([classe[0]], classe, {}, L).size).toBe(0);
  });
});

describe('placeBack', () => {
  const memory = { listed: ['a', 'b', 'c', 'd', 'e'], shown: ['a', 'b', 'c', 'd', 'e'] };
  const without = (...gone: string[]) => classe.filter((n) => !gone.includes(n.id));
  const back = (returned: string[], held: string[] | undefined, now: NoteSummary[], from = memory) => placeBack(from, returned, held, now, L);

  it('has nothing to write when the file still lists the notes (their place is intact)', () => {
    expect(back(['b'], ['a', 'b', 'c', 'd', 'e'], classe)).toEqual({ kind: 'none' });
  });

  it('has nothing to write without a memory, without a free order any more, or for notes that were not listed', () => {
    expect(placeBack(undefined, ['b'], ['a'], classe, L)).toEqual({ kind: 'none' });
    expect(back(['b'], undefined, classe)).toEqual({ kind: 'none' });
    expect(back(['b'], ['a', 'c', 'd', 'e'], classe, { listed: ['a', 'c', 'd', 'e'], shown: ['a', 'c', 'd', 'e', 'b'] })).toEqual({ kind: 'none' });
  });

  it('puts a note back after the neighbour it had when the others kept their order', () => {
    expect(back(['c'], ['a', 'b', 'd', 'e'], classe)).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'd', 'e'], atEnd: [] });
    // First place: before its next neighbour.
    expect(back(['a'], ['b', 'c', 'd', 'e'], classe)).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'd', 'e'], atEnd: [] });
    // Last place.
    expect(back(['e'], ['a', 'b', 'c', 'd'], classe)).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'd', 'e'], atEnd: [] });
  });

  it('keeps several notes that left together side by side', () => {
    expect(back(['b', 'c'], ['a', 'd', 'e'], classe)).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'd', 'e'], atEnd: [] });
    // Their neighbour is gone too (a deleted note): the next one that is there anchors them.
    expect(back(['c', 'd'], ['a', 'e'], classe.filter((n) => n.id !== 'b'))).toEqual({ kind: 'write', ids: ['a', 'c', 'd', 'e'], atEnd: [] });
  });

  it('keeps the notes that arrived since where the file puts them', () => {
    const now = [...classe, note('new', 'Classe', 9_000)];
    expect(back(['c'], ['a', 'b', 'd', 'e', 'new'], now)).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'd', 'e', 'new'], atEnd: [] });
  });

  it('puts the note at the end, and says which, when the others were rearranged meanwhile', () => {
    expect(back(['c'], ['e', 'a', 'd', 'b'], classe)).toEqual({ kind: 'write', ids: ['e', 'a', 'd', 'b', 'c'], atEnd: ['c'] });
  });

  it('does not count the notes that went (the target) as a rearrangement', () => {
    // `d` went elsewhere for good: the others are in the order they were.
    expect(back(['c'], ['a', 'b', 'e'], without('d'))).toEqual({ kind: 'write', ids: ['a', 'b', 'c', 'e'], atEnd: [] });
  });
});

describe('withoutIds', () => {
  it('drops the ids of the notes that left, or says there is nothing to write', () => {
    expect(withoutIds(['a', 'b', 'c'], ['b', 'x'])).toEqual(['a', 'c']);
    expect(withoutIds(['a', 'c'], ['b'])).toBeNull();
    expect(withoutIds(undefined, ['b'])).toBeNull();
  });
});

describe('fileNameOf', () => {
  it('takes the last part of a path with either separator', () => {
    expect(fileNameOf('C:\\lib\\Perso\\2026-10-24 Cours.ardoise')).toBe('2026-10-24 Cours.ardoise');
    expect(fileNameOf('Perso/Cours.ardoise')).toBe('Cours.ardoise');
    expect(fileNameOf('Cours.ardoise')).toBe('Cours.ardoise');
  });
});
