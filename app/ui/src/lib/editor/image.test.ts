import { describe, expect, it } from 'vitest';
import type { StagedBackground } from '../background';
import { commit, createHistory, redo, undo } from './history';
import {
  importedImage,
  pixelsOf,
  placedImage,
  removeBackgroundImage,
  setBackgroundImage,
  setImageFit,
  setImageOpacity,
} from './image';
import { addLayer, docFromNote, mergeDown, moveLayer, rotate, type Doc } from './state';
import { hline, note } from './test-fixtures';

const RED: StagedBackground = { key: 'red', name: 'rouge.png', width: 400, height: 300 };
const PAGE2: StagedBackground = { key: 'pdf-2', name: 'fiche.pdf', pdfPage: 2, width: 2480, height: 3508 };

function start(): Doc {
  return docFromNote(note([hline(1000, 4000, 3000)]));
}

function withImage(doc: Doc, staged: StagedBackground): Doc {
  return setBackgroundImage(doc, importedImage(doc.note.page, staged), pixelsOf(staged));
}

describe('the background image in the editor', () => {
  it('imports an image placed whole in the page, opaque, with its pixels', () => {
    const doc = withImage(start(), PAGE2);
    expect(doc.note.backgroundImage).toEqual({
      name: 'fiche.pdf',
      pdfPage: 2,
      rect: expect.objectContaining({ y: 0, height: 21600 }),
      quarterTurns: 0,
      opacity: 1,
    });
    expect(doc.backgroundPixels).toEqual({ key: 'pdf-2', width: 2480, height: 3508 });
    expect(placedImage(doc)).toEqual({ image: doc.note.backgroundImage, key: 'pdf-2' });
    expect(placedImage(start())).toBeUndefined();
    expect('pdfPage' in importedImage(start().note.page, RED)).toBe(false);
  });

  it('keeps every image with its own pixels through undo and redo', () => {
    let history = createHistory(start());
    history = commit(history, withImage(history.present, RED));
    history = commit(history, withImage(history.present, PAGE2));
    history = commit(history, setImageOpacity(history.present, 0.4));
    history = commit(history, removeBackgroundImage(history.present));
    expect(history.present.note.backgroundImage).toBeUndefined();
    expect(history.present.backgroundPixels).toBeUndefined();
    history = undo(history);
    expect(history.present.note.backgroundImage?.opacity).toBe(0.4);
    expect(history.present.backgroundPixels?.key).toBe('pdf-2');
    history = undo(undo(history));
    // The first image comes back with the first image's pixels.
    expect(history.present.note.backgroundImage?.name).toBe('rouge.png');
    expect(history.present.backgroundPixels?.key).toBe('red');
    history = undo(history);
    expect(placedImage(history.present)).toBeUndefined();
    history = redo(redo(history));
    expect(history.present.backgroundPixels?.key).toBe('pdf-2');
  });

  it('keeps the pixels through the other operations of the doc', () => {
    let doc = withImage(start(), RED);
    doc = addLayer(doc, 'L2');
    expect(doc.backgroundPixels?.key).toBe('red');
    doc = moveLayer(doc, 1, 0);
    expect(doc.backgroundPixels?.key).toBe('red');
    doc = mergeDown(doc, 1);
    expect(doc.backgroundPixels?.key).toBe('red');
    doc = rotate(doc, 'cw');
    expect(doc.backgroundPixels?.key).toBe('red');
    expect(doc.note.backgroundImage?.quarterTurns).toBe(1);
  });

  it('places the image again and clamps its opacity', () => {
    const doc = withImage(start(), PAGE2);
    const covered = setImageFit(doc, 'cover');
    expect(covered.note.backgroundImage?.rect.width).toBe(29700);
    expect(covered.note.backgroundImage?.rect.y).toBeLessThan(0);
    expect(setImageFit(covered, 'cover')).toBe(covered);
    expect(setImageFit(covered, 'stretch').note.backgroundImage?.rect).toEqual({ x: 0, y: 0, width: 29700, height: 21600 });
    // Without its pixels, it cannot be placed again.
    const unknown: Doc = { ...doc, backgroundPixels: undefined };
    expect(setImageFit(unknown, 'cover')).toBe(unknown);
    expect(setImageOpacity(doc, 3).note.backgroundImage?.opacity).toBe(1);
    expect(setImageOpacity(doc, 1)).toBe(doc);
    expect(setImageOpacity(doc, -1).note.backgroundImage?.opacity).toBe(0);
    expect(setImageOpacity(start(), 0.5)).toEqual(start());
    expect(removeBackgroundImage(start())).toEqual(start());
  });
});
