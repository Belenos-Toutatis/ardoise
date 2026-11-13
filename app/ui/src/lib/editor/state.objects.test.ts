/**
 * The selection holds strokes and text frames together (`lib/editor/state.ts`): chosen, moved, resized, deleted,
 * duplicated, copied and pasted at once, one undoable step each; the clipboard carries the frames
 * (`lib/editor/clipboard.ts`); the replay shows a frame all at once at its time (`lib/replay.ts`); the split takes them
 * by their time or their centre (`lib/editor/split.ts`); Escape ends the typing first (`lib/editor/escape.ts`); T is the
 * text tool (`lib/editor/keys.ts`).
 */
import { describe, expect, it } from 'vitest';
import type { Note, TextObject } from '../note';
import { ReplayTimeline } from '../replay';
import { decodeClipboard, decodeStrokes, encodeClipboard, encodeStrokes } from './clipboard';
import { escapeStep, type EscapeState } from './escape';
import { commit, createHistory, undo } from './history';
import { editorKeyAction } from './keys';
import { objectsOf, withObjects } from './objects';
import { previewAtTime, previewZone, applySplit, timeRange } from './split';
import {
  addLayer,
  applyStyle,
  clearSelection,
  crop,
  deleteSelection,
  docFromNote,
  duplicateSelection,
  hasSelection,
  objectSelection,
  pasteStrokes,
  rotate,
  select,
  selectAll,
  selectedObjects,
  selectionBounds,
  selectionCount,
  setActiveLayer,
  setLayerVisible,
  transformSelection,
} from './state';
import { RED, hline, note } from './test-fixtures';
import { scalingAround, translation } from './geometry';

function frame(fields: Partial<TextObject> = {}): TextObject {
  return {
    kind: 'text',
    id: 'f',
    layer: 0,
    t: 2000,
    x: 5000,
    y: 5000,
    scale: 1,
    quarterTurns: 0,
    width: 3000,
    height: 750,
    text: 'Bonjour',
    font: 'latinModern',
    bold: false,
    size: 600,
    color: { r: 0, g: 0, b: 0, a: 255 },
    ...fields,
  };
}

function page(layers = 1): Note {
  return withObjects(note([hline(1000, 3000, 1000)], layers), [frame({ id: 'a' }), frame({ id: 'b', x: 15_000, y: 12_000 })]);
}

describe('a selection of strokes and text frames', () => {
  it('is chosen with Ctrl+A on the chosen layer, counted and bounded together', () => {
    const doc = selectAll(docFromNote(page()));
    expect(doc.selection).toEqual([0]);
    expect(objectSelection(doc)).toEqual([0, 1]);
    expect(selectionCount(doc)).toBe(3);
    const box = selectionBounds(doc)!;
    expect(box.x).toBeLessThanOrEqual(1000);
    expect(box.x + box.width).toBeGreaterThanOrEqual(18_000);
    // Another layer: its own things only, and the selection goes.
    const other = setActiveLayer(docFromNote(addLayer(doc, 'L2').note), 1);
    expect(hasSelection(selectAll(other))).toBe(false);
    expect(hasSelection(setActiveLayer(doc, 0))).toBe(true);
  });

  it('moves and resizes as one', () => {
    const doc = select(docFromNote(page()), [0], [1]);
    const moved = transformSelection(doc, translation(100, -200));
    expect(objectsOf(moved.note)[1]).toMatchObject({ x: 15_100, y: 11_800, scale: 1 });
    expect(objectsOf(moved.note)[0]).toBe(objectsOf(doc.note)[0]);
    expect(moved.note.strokes[0].points[0]).toMatchObject({ x: 1100, y: 800 });
    const resized = transformSelection(doc, scalingAround({ x: 0, y: 0 }, 0.5));
    expect(objectsOf(resized.note)[1]).toMatchObject({ x: 7500, y: 6000, scale: 0.5 });
  });

  it('is deleted, duplicated and recoloured as one, one step each', () => {
    const doc = select(docFromNote(page()), [0], [0]);
    let history = createHistory(doc);
    history = commit(history, deleteSelection(history.present));
    expect(history.present.note.strokes.length).toBe(0);
    expect(objectsOf(history.present.note).map((o) => o.id)).toEqual(['b']);
    expect(hasSelection(history.present)).toBe(false);
    history = undo(history);
    expect(objectsOf(history.present.note).length).toBe(2);

    const twice = duplicateSelection(doc);
    expect(objectsOf(twice.note).length).toBe(3);
    expect(objectSelection(twice)).toEqual([2]);
    expect(objectsOf(twice.note)[2]).toMatchObject({ x: 5400, y: 5400 });
    expect(objectsOf(twice.note)[2].id).not.toBe('a');

    const red = applyStyle(doc, { color: RED });
    expect((objectsOf(red.note)[0] as TextObject).color).toEqual(RED);
    expect((objectsOf(red.note)[1] as TextObject).color).not.toEqual(RED);
    // The whole note: every frame too; the tool and the width are the ink's.
    const all = applyStyle(clearSelection(doc), { color: RED, width: 300 });
    expect(objectsOf(all.note).every((o) => (o as TextObject).color.r === RED.r)).toBe(true);
    expect('width' in objectsOf(all.note)[0] && objectsOf(all.note)[0].width).toBe(3000);
  });

  it('loses what a hidden layer holds, and a crop or a turn keeps no stale index', () => {
    const doc = select(docFromNote(page(2)), [0], [0, 1]);
    const hidden = setLayerVisible(doc, 0, false);
    expect(objectSelection(hidden)).toEqual([]);
    expect(hidden.selection).toEqual([]);
    expect(hasSelection(crop(doc, { x: 0, y: 0, width: 10_000, height: 10_000 }))).toBe(false);
    const turned = rotate(doc, 'cw');
    expect(objectsOf(turned.note)[0].quarterTurns).toBe(1);
  });
});

describe('the clipboard', () => {
  it('carries text frames with the strokes, without their id nor layer', () => {
    const doc = select(docFromNote(page()), [0], [0]);
    const text = encodeClipboard([doc.note.strokes[0]], selectedObjects(doc));
    const content = decodeClipboard(text)!;
    expect(content.strokes.length).toBe(1);
    expect(content.objects).toHaveLength(1);
    expect(content.objects[0]).toMatchObject({ kind: 'text', text: 'Bonjour', x: 5000, y: 5000, id: '', layer: 0 });
    // Frames alone are pasted too; strokes alone are read by older code as before.
    expect(decodeClipboard(encodeClipboard([], selectedObjects(doc)))!.objects).toHaveLength(1);
    expect(decodeStrokes(encodeClipboard([], selectedObjects(doc)))).toBeNull();
    expect(decodeStrokes(encodeStrokes([doc.note.strokes[0]]))).toHaveLength(1);
  });

  it('refuses a damaged or hostile frame, and keeps nothing of a list that has one', () => {
    const good = JSON.parse(encodeClipboard([], [frame()]));
    for (const bad of [
      { ...good.objects[0], font: 'comic' },
      { ...good.objects[0], size: -1 },
      { ...good.objects[0], text: 5 },
      { ...good.objects[0], quarterTurns: 7 },
      { ...good.objects[0], x: 1e12 },
      { ...good.objects[0], kind: 'equation' },
    ]) {
      expect(decodeClipboard(JSON.stringify({ ...good, objects: [bad] }))).toBeNull();
    }
    expect(decodeClipboard(JSON.stringify({ ...good, objects: 'x' }))).toBeNull();
    expect(decodeClipboard(JSON.stringify({ ...good, objects: [] }))).toBeNull();
  });

  it('pastes frames after the note, selected, one step', () => {
    const doc = docFromNote(page());
    const pasted = pasteStrokes(doc, [], undefined, [frame({ id: '', t: 0 })]);
    const copy = objectsOf(pasted.note)[2];
    expect(objectSelection(pasted)).toEqual([2]);
    // After the note's content (its last thing is a frame at 2000 ms), half a second later.
    expect(copy.t).toBe(2500);
    // Where it was, then further while an identical frame lies there.
    expect(copy).toMatchObject({ x: 5400, y: 5400 });
    expect(copy.id).not.toBe('');
  });
});

describe('the replay', () => {
  it('shows a frame all at once at its time, and lasts until it appears', () => {
    const n = withObjects(note([hline(0, 1000, 500)]), [frame({ t: 5000 })]);
    const timeline = new ReplayTimeline(n);
    expect(timeline.duration).toBe(5000);
    expect(objectsOf(timeline.noteAt(4999))).toHaveLength(0);
    expect(objectsOf(timeline.noteAt(5000))).toHaveLength(1);
    // A still frame gives the same note again.
    expect(timeline.noteAt(6000)).toBe(timeline.noteAt(7000));
    // A frame on a hidden layer is not waited for.
    const hidden = { ...withObjects(note([hline(0, 1000, 500)], 2), [frame({ t: 9000, layer: 1 })]) };
    hidden.layers = [hidden.layers[0], { ...hidden.layers[1], visible: false }];
    expect(new ReplayTimeline(hidden).duration).toBeLessThan(1000);
  });
});

describe('the split', () => {
  it('moves the frames that appear after the instant, or whose centre is in the zone', () => {
    const n = page();
    const later = withObjects(n, [frame({ id: 'early', t: 5 }), frame({ id: 'late', t: 9000 })]);
    expect([...previewAtTime(later, 1000).movedObjects]).toEqual([1]);
    expect(timeRange(later)).toEqual({ min: 0, max: 9000 });
    const zone = previewZone(n, [
      { x: 4000, y: 4000 },
      { x: 9000, y: 4000 },
      { x: 9000, y: 7000 },
      { x: 4000, y: 7000 },
    ]);
    expect([...zone.movedObjects]).toEqual([0]);
    expect(zone.movedCount).toBe(1);
    const parts = applySplit(n, { kind: 'zone', polygon: [[4000, 4000], [9000, 4000], [9000, 7000], [4000, 7000]] })!;
    expect(objectsOf(parts.kept).map((o) => o.id)).toEqual(['b']);
    expect(objectsOf(parts.extracted).map((o) => o.id)).toEqual(['a']);
  });
});

describe('keys', () => {
  it('ends the typing of a frame at the first Escape, before the field keeps its own', () => {
    const state: EscapeState = {
      leaving: false,
      owned: false,
      dragging: false,
      split: false,
      settings: false,
      typing: true,
      question: false,
      replaying: false,
      cropping: false,
      selected: 1,
      editingText: true,
    };
    expect(escapeStep(state)).toBe('endText');
    expect(escapeStep({ ...state, editingText: false })).toBe('ignore');
    expect(escapeStep({ ...state, dragging: true })).toBe('cancelDrag');
    expect(escapeStep({ ...state, editingText: false, typing: false })).toBe('deselect');
  });

  it('gives T to the text tool', () => {
    expect(editorKeyAction({ key: 't' }, { replaying: false })).toEqual({ kind: 'text' });
    expect(editorKeyAction({ key: 'T', shiftKey: true }, { replaying: false })).toEqual({ kind: 'text' });
    expect(editorKeyAction({ key: 't' }, { replaying: true })).toBeNull();
  });
});
