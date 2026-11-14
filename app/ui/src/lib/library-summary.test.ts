// The summary of a binder and the search of bookmarks: what the pure functions of `library-logic.ts` decide
// (SPEC §4.3 « Sommaire du classeur »). The dialog and the screens are in `components/library/Summary.test.ts`.
import { describe, expect, it } from 'vitest';
import {
  arrangeByBinderOrder,
  buildBinderTree,
  displayTitle,
  filterRows,
  listedTitles,
  matchesSearch,
  summaryRows,
  type SummaryChoices,
  type SummaryRow,
} from './library-logic';
import type { BookmarkRef, NoteSummary } from './note';

const STAMP = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function note(id: string, binder: string, created: number, labels: string[] = [], title = id): NoteSummary {
  const bookmarks: BookmarkRef[] = labels.map((label, i) => ({ id: `${id}-${i}`, label, x: 100 * i, y: 100 * i }));
  return {
    path: `C:/lib/${binder}/${id}.ardoise`,
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
  const ids = (rows: SummaryRow[]) => rows.map((r) => `${'.'.repeat(r.level)}${r.kind === 'binder' ? '#' : r.kind === 'note' ? '' : '⚑'}${r.label}`);
  const NOTES: SummaryChoices = { binders: true, notes: 'title', bookmarks: true };
  const BOOKMARKS_ONLY: SummaryChoices = { binders: true, notes: 'none', bookmarks: true };

  /** The notes of the binder `Classe` in its order, with these titles. */
  const book = [
    note('a', 'Classe', 1, ['Introduction', 'Bilan'], 'Cours'),
    note('b', 'Classe', 2, [], 'Sans signet'),
    note('c', 'Classe/3e B', 3, ['Loi d\'Ohm'], 'TP'),
    note('d', 'Classe/3e B/Devoirs', 4, [], 'DM'),
    note('e', 'Classe/3e C', 5, [], 'Vide'),
    note('f', 'Classe/3e C', 6, ['Zéro'], 'Dernier'),
  ];
  const titlesOf = (list: NoteSummary[]) => listedTitles(list, 'fr', false, 'Note sans titre');
  const rows = (choices: SummaryChoices, list = book) => summaryRows(list, 'Classe', choices, titlesOf(list));

  it('lists the notes and their bookmarks under the sub-binders that hold them, in the order given', () => {
    expect(ids(rows({ binders: true, notes: 'title', bookmarks: true }))).toEqual([
      'Cours', '.⚑Introduction', '.⚑Bilan', 'Sans signet',
      '#3e B', '.TP', '..⚑Loi d\'Ohm', '.#Devoirs', '..DM',
      '#3e C', '.Vide', '.Dernier', '..⚑Zéro',
    ]);
  });

  it('with the notes left out, puts each bookmark under its sub-binder, a note without bookmark gives no line and no heading', () => {
    expect(ids(rows(BOOKMARKS_ONLY))).toEqual([
      '⚑Introduction', '⚑Bilan',
      '#3e B', '.⚑Loi d\'Ohm',
      '#3e C', '.⚑Zéro',
    ]);
    // No sub-binder either: a flat list in the binder's order.
    expect(ids(rows({ ...BOOKMARKS_ONLY, binders: false }))).toEqual(['⚑Introduction', '⚑Bilan', '⚑Loi d\'Ohm', '⚑Zéro']);
  });

  it('with the bookmarks left out, lists the notes under their sub-binders, and without the sub-binders no level is left for them', () => {
    expect(ids(rows({ binders: true, notes: 'title', bookmarks: false }))).toEqual([
      'Cours', 'Sans signet', '#3e B', '.TP', '.#Devoirs', '..DM', '#3e C', '.Vide', '.Dernier',
    ]);
    expect(ids(rows({ binders: false, notes: 'title', bookmarks: true }))).toEqual([
      'Cours', '.⚑Introduction', '.⚑Bilan', 'Sans signet', 'TP', '.⚑Loi d\'Ohm', 'DM', 'Vide', 'Dernier', '.⚑Zéro',
    ]);
  });

  it('is empty when the choices leave nothing, the headings alone being nothing', () => {
    expect(rows({ binders: false, notes: 'none', bookmarks: false })).toEqual([]);
    expect(rows({ binders: true, notes: 'none', bookmarks: false })).toEqual([]);
    expect(rows(BOOKMARKS_ONLY, [note('x', 'Classe', 1)])).toEqual([]);
    expect(summaryRows([], 'Classe', NOTES, new Map())).toEqual([]);
  });

  it('names an untitled note by its title, « Note sans titre » (numbered), or its date, as asked', () => {
    const list = [note('u', 'Classe', Date.UTC(2026, 9, 1), ['Un'], ''), note('v', 'Classe', Date.UTC(2026, 9, 2), ['Deux'], '')];
    const by = (withDates: boolean) => summaryRows(list, 'Classe', NOTES, listedTitles(list, 'fr', withDates, 'Note sans titre'));
    expect(ids(by(false))).toEqual(['Note sans titre', '.⚑Un', 'Note sans titre (2)', '.⚑Deux']);
    const dated = ids(by(true));
    expect(dated[0]).toMatch(/2026/);
    // The bookmarks say which note they are in (the aria label), whatever the notes mode.
    expect(summaryRows(list, 'Classe', BOOKMARKS_ONLY, listedTitles(list, 'fr', false, 'Note sans titre'))[1].noteName).toBe('Note sans titre (2)');
  });

  it('follows the binder\'s order: its free order, the others by date, each sub-binder in its own order', () => {
    const base = [note('a', 'Classe', 1, ['A1']), note('b', 'Classe', 2, ['B1']), note('c', 'Classe', 3, ['C1']), note('d', 'Classe/3e B', 4, ['D1'])];
    const t = buildBinderTree(['Classe', 'Classe/3e B'], base);
    const byDate = arrangeByBinderOrder(base, 'Classe', { tree: t, orders: {} }, 'fr');
    expect(ids(summaryRows(byDate, 'Classe', BOOKMARKS_ONLY, titlesOf(byDate)))).toEqual(['⚑A1', '⚑B1', '⚑C1', '#3e B', '.⚑D1']);
    const ordered = arrangeByBinderOrder(base, 'Classe', { tree: t, orders: { Classe: ['c', 'b', 'a'] } }, 'fr');
    expect(ids(summaryRows(ordered, 'Classe', BOOKMARKS_ONLY, titlesOf(ordered)))).toEqual(['⚑C1', '⚑B1', '⚑A1', '#3e B', '.⚑D1']);
  });

  it('is narrowed by the words typed: any line, with what is above and below it, accents and case ignored', () => {
    const all = rows(NOTES);
    expect(filterRows(all, '  ')).toEqual(all);
    // A bookmark: itself, its note and its sub-binder.
    expect(ids(filterRows(all, 'OHM'))).toEqual(['#3e B', '.TP', '..⚑Loi d\'Ohm']);
    expect(ids(filterRows(all, 'zero'))).toEqual(['#3e C', '.Dernier', '..⚑Zéro']);
    // A note: its bookmarks too.
    expect(ids(filterRows(all, 'cours'))).toEqual(['Cours', '.⚑Introduction', '.⚑Bilan']);
    // A sub-binder: everything under it.
    expect(ids(filterRows(all, 'devoirs'))).toEqual(['#3e B', '.#Devoirs', '..DM']);
    // Every word on one line.
    expect(filterRows(all, 'intro bil')).toEqual([]);
    expect(filterRows(all, 'zzz')).toEqual([]);
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
