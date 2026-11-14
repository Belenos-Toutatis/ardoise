// The summary of a binder and the search of bookmarks: what the pure functions of `library-logic.ts` decide
// (SPEC §4.3 « Sommaire du classeur »). The dialog and the screens are in `components/library/Summary.test.ts`.
import { describe, expect, it } from 'vitest';
import { arrangeByBinderOrder, binderSummary, buildBinderTree, displayTitle, filterSummary, listedTitles, matchesSearch } from './library-logic';
import type { BookmarkRef, NoteSummary } from './note';

const STAMP = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function note(id: string, binder: string, created: number, labels: string[] = [], title = id): NoteSummary {
  const bookmarks: BookmarkRef[] = labels.map((label, i) => ({ id: `${id}-${i}`, label, x: 100 * i, y: 100 * i }));
  return {
    path: `C:\lib\${binder}\${id}.ardoise`,
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
    ...(bookmarks.length > 0 ? { bookmarks } : {}),
  };
}

const notes = [
  note('a', 'Classe', 1, ['Introduction', 'Bilan']),
  note('b', 'Classe', 2),
  note('c', 'Classe', 3, ['Corrigé']),
  note('d', 'Classe/3e B', 4, ['Loi d\'Ohm'], 'Cours'),
];

describe('the names of the notes of a summary or of a PDF outline', () => {
  const named = (title: string, created: number, path = title) => ({ path, title, created });
  const list = [named('Cours', 1), named('', Date.UTC(2026, 9, 1), 'u1'), named('  ', Date.UTC(2026, 9, 2), 'u2'), named('', Date.UTC(2026, 9, 3), 'u3')];

  it('puts « Note sans titre » for an untitled note without the dates, numbered from the second', () => {
    const names = listedTitles(list, 'fr', false, 'Note sans titre');
    expect([...names.values()]).toEqual(['Cours', 'Note sans titre', 'Note sans titre (2)', 'Note sans titre (3)']);
  });

  it('puts the date of an untitled note with the dates (the title of a titled note never changes)', () => {
    const names = listedTitles(list, 'fr', true, 'Note sans titre');
    expect(names.get('Cours')).toBe('Cours');
    expect(names.get('u1')).toBe(displayTitle(list[1], 'fr'));
    expect(names.get('u1')).toMatch(/2026/);
  });
});

describe('the summary of a binder', () => {
  it('keeps the notes that have bookmarks, in the order given, and says where each is below the binder', () => {
    const tree = buildBinderTree(['Classe', 'Classe/3e B'], notes);
    // Without a free order: by date, the sub-binders after.
    const byDate = arrangeByBinderOrder(notes, 'Classe', { tree, orders: {} }, 'fr');
    expect(binderSummary(byDate, 'Classe').map((e) => [e.note.id, e.below, e.bookmarks.length])).toEqual([
      ['a', [], 2],
      ['c', [], 1],
      ['d', ['3e B'], 1],
    ]);
    // With a free order: its order for the binder's notes, the others after by date, the sub-binder last.
    const ordered = arrangeByBinderOrder(notes, 'Classe', { tree, orders: { Classe: ['c', 'b', 'a'] } }, 'fr');
    expect(binderSummary(ordered, 'Classe').map((e) => e.note.id)).toEqual(['c', 'a', 'd']);
    // And its own order for each sub-binder.
    const more = [...notes, note('e', 'Classe/3e B', 1, ['Premier'])];
    const both = arrangeByBinderOrder(more, 'Classe', { tree: buildBinderTree(['Classe', 'Classe/3e B'], more), orders: { 'Classe/3e B': ['d', 'e'] } }, 'fr');
    expect(binderSummary(both, 'Classe').map((e) => e.note.id)).toEqual(['a', 'c', 'd', 'e']);
    expect(binderSummary([], 'Classe')).toEqual([]);
  });

  it('is narrowed by the words typed: the labels, or a note\'s title, accents and case ignored', () => {
    const summary = binderSummary(notes, 'Classe');
    expect(filterSummary(summary, '  ').length).toBe(3);
    expect(filterSummary(summary, 'CORRIGE').map((e) => e.note.id)).toEqual(['c']);
    expect(filterSummary(summary, 'intro bil').map((e) => e.note.id)).toEqual([]);
    expect(filterSummary(summary, 'bilan').map((e) => e.bookmarks.map((b) => b.label))).toEqual([['Bilan']]);
    // The words of a note's title keep all of its bookmarks.
    expect(filterSummary(summary, 'cours')[0].bookmarks).toHaveLength(1);
    expect(filterSummary(summary, 'a')[0].bookmarks.map((b) => b.label)).toEqual(['Introduction', 'Bilan']);
  });
});

describe('the search of the library', () => {
  it('finds a note by the labels of its bookmarks', () => {
    const [a, b] = notes;
    expect(matchesSearch(a, 'introduction')).toBe(true);
    expect(matchesSearch(a, 'BILAN introduction')).toBe(true);
    expect(matchesSearch(a, 'corrige')).toBe(false);
    expect(matchesSearch(b, 'introduction')).toBe(false);
    expect(matchesSearch(notes[3], "OHM")).toBe(true);
  });
});
