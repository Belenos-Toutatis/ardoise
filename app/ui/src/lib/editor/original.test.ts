import { describe, expect, it } from 'vitest';
import type { StagedBackground } from '../background';
import type { Note } from '../note';
import type { NoteOriginal } from '../original';
import { importedImage, pixelsOf, setBackgroundImage, setImageFit, setImageOpacity } from './image';
import { revertToOriginal } from './original';
import { crop, docFromNote, rotate, select, type Doc } from './state';
import { hline, note } from './test-fixtures';

const PAGE2: StagedBackground = { key: 'pdf-2', name: 'fiche.pdf', pdfPage: 2, width: 3508, height: 2480 };
const PHOTO: StagedBackground = { key: 'photo', name: 'photo.jpg', width: 400, height: 300 };

/** A landscape A4 note with a stroke and `staged` under it (contained). */
function withImage(staged: StagedBackground): Doc {
  const doc = docFromNote(note([hline(1000, 4000, 3000)]));
  return setBackgroundImage(doc, importedImage(doc.note.page, staged), pixelsOf(staged));
}

/** What `note_original` gives for the note as it is in `doc` (saved once, then edited). */
function firstVersion(n: Note): NoteOriginal {
  const original: NoteOriginal = {
    source: 'firstVersion',
    page: n.page,
    layers: n.layers,
    strokes: n.strokes,
    paper: n.background.paper,
  };
  if (n.backgroundImage) original.backgroundImage = structuredClone(n.backgroundImage);
  return original;
}

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

  it('goes back to a blank worksheet: no stroke, the image where it was, the paper it had', () => {
    const blank = withImage(PAGE2);
    blank.note.strokes = [];
    blank.note.background = { paper: 'kraft', template: 'blank', texture: false };
    const original = firstVersion(blank.note);
    // Written on in live mode, under the blackboard: chalk, a layer of its own, slate paper.
    const written = docFromNote({
      ...blank.note,
      layers: [...blank.note.layers, { name: 'Calque 2', visible: true }],
      strokes: [hline(0, 1000, 100, 100, 1)],
      background: { ...blank.note.background, paper: 'slate' },
    });
    const back = revertToOriginal({ ...written, backgroundPixels: blank.backgroundPixels }, original);
    expect(back.note.strokes).toEqual([]);
    expect(back.note.layers).toEqual(blank.note.layers);
    expect(back.note.background).toEqual(blank.note.background);
    expect(back.note.backgroundImage).toEqual(blank.note.backgroundImage);
    expect(back.backgroundPixels).toEqual(blank.backgroundPixels);
    expect(back.activeLayer).toBe(0);
  });

  it('leaves the paper as it is but for that slate', () => {
    const saved = docFromNote({ ...note([hline(0, 1000, 100)]), background: { paper: 'cream', template: 'seyes', texture: true } });
    const original = firstVersion(saved.note);
    // Another paper chosen since stays.
    const kraft = docFromNote({ ...saved.note, background: { ...saved.note.background, paper: 'kraft' } });
    expect(revertToOriginal(kraft, original).note.background.paper).toBe('kraft');
    // Slate over a first version on slate stays too, and a tablet page says nothing of its paper.
    const slate = docFromNote({ ...saved.note, background: { ...saved.note.background, paper: 'slate' } });
    expect(revertToOriginal(slate, { ...original, paper: 'slate' } as NoteOriginal).note.background.paper).toBe('slate');
    const { paper: _paper, ...tablet } = original as Extract<NoteOriginal, { source: 'firstVersion' | 'tablet' }>;
    expect(revertToOriginal(slate, { ...tablet, source: 'tablet' }).note.background.paper).toBe('slate');
    // Its template and texture are not concerned.
    expect(revertToOriginal(slate, original).note.background).toEqual(saved.note.background);
  });

  it('changes nothing without an original', () => {
    const doc = docFromNote(note([hline(0, 1000, 100)]));
    expect(revertToOriginal(doc, { source: 'none' })).toBe(doc);
  });

  it('puts the same image back where it was after a rotation', () => {
    const saved = withImage(PAGE2);
    const original = firstVersion(saved.note);
    const turned = setImageOpacity(rotate(saved, 'cw'), 0.5);
    expect(turned.note.backgroundImage?.quarterTurns).toBe(1);
    const back = revertToOriginal(turned, original);
    expect(back.note.page).toEqual(saved.note.page);
    expect(back.note.backgroundImage).toEqual({ ...saved.note.backgroundImage, opacity: 0.5 });
    // The pixels stay.
    expect(back.backgroundPixels).toEqual(saved.backgroundPixels);
  });

  it('puts the same image back where it was after a crop', () => {
    const saved = withImage(PAGE2);
    const original = firstVersion(saved.note);
    const cropped = crop(saved, { x: 3000, y: 2000, width: 10000, height: 8000 });
    expect(cropped.note.backgroundImage?.rect.x).toBe(-3000);
    const back = revertToOriginal(cropped, original);
    expect(back.note.backgroundImage).toEqual(saved.note.backgroundImage);
  });

  it('a tablet page: the image comes back upright, placed again on the restored page', () => {
    const saved = withImage(PAGE2);
    const { page, layers, strokes } = saved.note;
    const tablet: NoteOriginal = { source: 'tablet', page, layers, strokes };
    // Turned (portrait page), then stretched over it.
    const turned = setImageFit(rotate(saved, 'cw'), 'stretch');
    const back = revertToOriginal(turned, tablet);
    expect(back.note.page).toEqual({ width: 29700, height: 21600 });
    expect(back.note.backgroundImage).toEqual({
      ...turned.note.backgroundImage,
      quarterTurns: 0,
      rect: { x: 0, y: 0, width: 29700, height: 21600 },
    });
    // Contained when it was moved by a crop.
    const cropped = crop(rotate(saved, 'cw'), { x: 1000, y: 1000, width: 15000, height: 20000 });
    const contained = revertToOriginal(cropped, tablet).note.backgroundImage;
    expect(contained?.quarterTurns).toBe(0);
    // 3508 × 2480 px in 29700 × 21600: as wide as the page, centred.
    expect(contained?.rect.x).toBeCloseTo(0, 3);
    expect(contained?.rect.width).toBeCloseTo(29700, 3);
    expect(contained?.rect.y).toBeCloseTo((21600 - (29700 * 2480) / 3508) / 2, 3);
  });

  it('another image than the original one is placed again when the page changes', () => {
    // The original had a photo; the edited note turned the page and holds a PDF page.
    const photo = withImage(PHOTO);
    const original = firstVersion(photo.note);
    const turned = rotate(photo, 'cw');
    const other = setBackgroundImage(turned, importedImage(turned.note.page, PAGE2, 'cover'), pixelsOf(PAGE2));
    const back = revertToOriginal(other, original).note.backgroundImage;
    expect(back?.name).toBe('fiche.pdf');
    expect(back?.quarterTurns).toBe(0);
    // Still covering the page it is now on.
    expect(back?.rect.height).toBeCloseTo(21600, 0);
    expect(back?.rect.width).toBeGreaterThanOrEqual(29700 - 1);
    // The same page and no turn: it stays where it is.
    const unchanged = setBackgroundImage(photo, importedImage(photo.note.page, PAGE2, 'cover'), pixelsOf(PAGE2));
    expect(revertToOriginal(unchanged, original).note.backgroundImage).toEqual(unchanged.note.backgroundImage);
  });

  it('an image removed since stays removed, and none appears', () => {
    const saved = withImage(PAGE2);
    const original = firstVersion(saved.note);
    const removed = docFromNote({ ...rotate(saved, 'cw').note, backgroundImage: undefined });
    const back = revertToOriginal(removed, original);
    expect(back.note.backgroundImage).toBeUndefined();
  });
});
