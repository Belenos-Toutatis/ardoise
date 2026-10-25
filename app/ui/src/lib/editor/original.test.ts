import { describe, expect, it } from 'vitest';
import type { NoteOriginal } from '../original';
import { revertToOriginal } from './original';
import { docFromNote, select } from './state';
import { hline, note } from './test-fixtures';

describe('back to the original', () => {
  it('takes the page, layers and strokes of the original and keeps the rest', () => {
    const edited = {
      ...note([hline(0, 1000, 100), hline(0, 1000, 200, 100, 1)], 2, { width: 21600, height: 29700 }),
      title: 'Cours',
      tags: ['physique'],
      favorite: true,
      background: { paper: 'cream' as const, template: 'seyes' as const, texture: true },
    };
    const doc = select(docFromNote(edited), [0, 1]);
    const original: NoteOriginal = {
      source: 'firstVersion',
      page: { width: 29700, height: 21600 },
      layers: [
        { name: 'Calque 1', visible: true },
        { name: 'Calque 2', visible: false },
      ],
      strokes: [hline(500, 900, 300)],
    };
    const back = revertToOriginal(doc, original);
    expect(back.note.page).toEqual(original.page);
    expect(back.note.layers).toEqual(original.layers);
    expect(back.note.strokes).toEqual(original.strokes);
    expect(back.selection).toEqual([]);
    // New ink goes on the top visible layer.
    expect(back.activeLayer).toBe(0);
    const { strokes: _s, layers: _l, page: _p, ...rest } = back.note;
    const { strokes: _s2, layers: _l2, page: _p2, ...kept } = edited;
    expect(rest).toEqual(kept);
  });

  it('changes nothing without an original', () => {
    const doc = docFromNote(note([hline(0, 1000, 100)]));
    expect(revertToOriginal(doc, { source: 'none' })).toBe(doc);
  });
});
