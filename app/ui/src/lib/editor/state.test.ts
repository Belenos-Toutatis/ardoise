import { describe, expect, it } from 'vitest';
import { INKSPACE_COLORS } from '../note';
import {
  CUSTOM_COLORS_KEY,
  MAX_CUSTOM_COLORS,
  fromHex,
  loadCustomColors,
  rememberCustomColor,
  toHex,
} from './colors';
import { translation } from './geometry';
import { canRedo, canUndo, commit, createHistory, redo, replacePresent, undo, type History } from './history';
import {
  addLayer,
  addStroke,
  applyStyle,
  autoCrop,
  canDraw,
  clearSelection,
  crop,
  deleteSelection,
  docFromNote,
  duplicateSelection,
  erase,
  initialState,
  mergeDown,
  moveLayer,
  paste,
  pasteStrokes,
  rotate,
  select,
  selectAll,
  selectedStrokes,
  selectionBounds,
  setActiveLayer,
  setLayerVisible,
  transformSelection,
  type Doc,
} from './state';
import { RED, hline, note } from './test-fixtures';

const pen = (y: number) => {
  const { layer, ...rest } = hline(0, 1000, y);
  void layer;
  return rest;
};

describe('history', () => {
  it('undoes and redoes every step, without limit', () => {
    let h: History<number> = createHistory(0);
    expect(canUndo(h)).toBe(false);
    for (let i = 1; i <= 500; i++) h = commit(h, i);
    expect(h.present).toBe(500);
    for (let i = 0; i < 500; i++) h = undo(h);
    expect(h.present).toBe(0);
    expect(canUndo(h)).toBe(false);
    expect(undo(h)).toBe(h);
    for (let i = 0; i < 500; i++) h = redo(h);
    expect(h.present).toBe(500);
    expect(canRedo(h)).toBe(false);
    expect(redo(h)).toBe(h);
  });

  it('drops the redo list on a new step, and ignores no-op commits', () => {
    let h = commit(commit(createHistory('a'), 'b'), 'c');
    h = undo(h);
    expect(canRedo(h)).toBe(true);
    h = commit(h, 'd');
    expect(canRedo(h)).toBe(false);
    expect(h.past).toEqual(['a', 'b']);
    expect(commit(h, h.present)).toBe(h);
  });

  it('replaces the present without adding a step', () => {
    const h = replacePresent(commit(createHistory(1), 2), 3);
    expect(h.present).toBe(3);
    expect(undo(h).present).toBe(1);
  });

  it('makes a drag one step and restores the selection with the note', () => {
    let h = createHistory<Doc>(docFromNote(note([hline(0, 1000, 100), hline(0, 1000, 500)])));
    h = replacePresent(h, select(h.present, [1]));
    // A drag: many intermediate states shown, one committed at the end.
    let live = h.present;
    for (let i = 0; i < 20; i++) live = transformSelection(h.present, translation(10 * i, 0));
    h = commit(h, live);
    expect(h.past).toHaveLength(1);
    expect(h.present.note.strokes[1].points[0].x).toBe(190);
    h = undo(h);
    expect(h.present.note.strokes[1].points[0].x).toBe(0);
    expect(h.present.selection).toEqual([1]);
  });
});

describe('doc operations', () => {
  it('starts on the top layer, adding one to a note without layers', () => {
    const doc = docFromNote(note([], 3));
    expect(doc.activeLayer).toBe(2);
    const empty = docFromNote({ ...note([]), layers: [] }, 'Calque 1');
    expect(empty.note.layers).toEqual([{ name: 'Calque 1', visible: true }]);
    expect(initialState(note([])).tools.mode).toBe('pen');
  });

  it('draws on the active layer and clears the selection', () => {
    let doc = select(docFromNote(note([hline(0, 1000, 100)], 2)), [0]);
    doc = setActiveLayer(doc, 0);
    doc = addStroke(doc, pen(800));
    expect(doc.note.strokes[1].layer).toBe(0);
    expect(doc.selection).toEqual([]);
    expect(setActiveLayer(doc, 9)).toBe(doc);
  });

  it('applies a style to the selection, or to everything', () => {
    const doc = docFromNote(note([hline(0, 1000, 100), hline(0, 1000, 500)]));
    const all = applyStyle(doc, { color: RED });
    expect(all.note.strokes.every((s) => s.color.r === RED.r)).toBe(true);
    const some = applyStyle(select(doc, [0]), { width: 200 });
    expect(some.note.strokes.map((s) => s.width)).toEqual([200, 60]);
    expect(applyStyle(select(doc, [0]), { width: 200 }, true).note.strokes.map((s) => s.width)).toEqual([200, 200]);
  });

  it('selects, deletes, duplicates and pastes', () => {
    const doc = docFromNote(note([hline(0, 1000, 100), hline(0, 1000, 500), hline(0, 1000, 900)]));
    expect(select(doc, [2, 0, 2, 7]).selection).toEqual([0, 2]);
    expect(select(doc, [])).toBe(doc);
    expect(selectAll(doc).selection).toEqual([0, 1, 2]);
    expect(clearSelection(selectAll(doc)).selection).toEqual([]);
    const sel = select(doc, [0, 2]);
    expect(selectionBounds(sel)).toEqual({ x: -30, y: 70, width: 1060, height: 860 });
    expect(deleteSelection(sel).note.strokes).toEqual([doc.note.strokes[1]]);
    const dup = duplicateSelection(sel);
    expect(dup.note.strokes).toHaveLength(5);
    expect(dup.selection).toEqual([3, 4]);
    expect(dup.note.strokes[3].points[0]).toMatchObject({ x: 400, y: 500 });
    const pasted = paste(doc, selectedStrokes(sel), { x: 0, y: 10 });
    expect(pasted.note.strokes[4].points[0]).toMatchObject({ x: 0, y: 910 });
  });

  it('erases and clears the selection', () => {
    const doc = select(docFromNote(note([hline(0, 1000, 100), hline(0, 1000, 500)])), [1]);
    const whole = erase(doc, [{ x: 500, y: 100 }], 50, 'whole');
    expect(whole.note.strokes).toHaveLength(1);
    expect(whole.selection).toEqual([]);
    const partial = erase(doc, [{ x: 500, y: 100 }], 150, 'partial');
    expect(partial.note.strokes).toHaveLength(3);
    expect(erase(doc, [{ x: 5000, y: 5000 }], 50, 'whole')).toBe(doc);
  });

  it('keeps the active layer and the selection consistent with layer changes', () => {
    let doc = docFromNote(note([hline(0, 1000, 100, 100, 0), hline(0, 1000, 500, 100, 1)], 2));
    doc = addLayer(doc, 'L3');
    expect(doc.activeLayer).toBe(2);
    doc = moveLayer(doc, 2, 0);
    expect(doc.activeLayer).toBe(0);
    expect(doc.note.strokes.map((s) => s.layer)).toEqual([1, 2]);
    doc = select(doc, [0, 1]);
    doc = setLayerVisible(doc, 2, false);
    expect(doc.selection).toEqual([0]);
    doc = setActiveLayer(doc, 2);
    doc = mergeDown(doc, 2);
    expect(doc.activeLayer).toBe(1);
    expect(doc.note.layers).toHaveLength(2);
  });

  it('never puts new ink on a hidden layer, and keeps the hidden layer chosen', () => {
    const hiddenTop = setLayerVisible(docFromNote(note([], 3)), 2, false).note;
    // Opening: the top visible layer is chosen.
    let doc = docFromNote(hiddenTop);
    expect(doc.activeLayer).toBe(1);
    // Hiding the chosen layer does not move the choice: the zone warns, the pen refuses.
    doc = setLayerVisible(doc, 1, false);
    expect(doc.activeLayer).toBe(1);
    expect(canDraw(doc)).toBe(false);
    expect(addStroke(doc, pen(100))).toBe(doc);
    expect(paste(doc, [hline(0, 1000, 100)])).toBe(doc);
    // Showing it again gives the same state back.
    doc = setLayerVisible(doc, 1, true);
    expect(doc.activeLayer).toBe(1);
    expect(addStroke(doc, pen(100)).note.strokes[0].layer).toBe(1);
    // Hiding another layer leaves the chosen one alone.
    doc = setLayerVisible(doc, 0, false);
    expect(doc.activeLayer).toBe(1);
    expect(canDraw(doc)).toBe(true);
    // Nothing visible: the choice stays, and drawing or pasting does nothing.
    doc = setLayerVisible(doc, 1, false);
    expect(doc.activeLayer).toBe(1);
    expect(canDraw(doc)).toBe(false);
    expect(addStroke(doc, pen(100))).toBe(doc);
    expect(docFromNote(doc.note).activeLayer).toBe(2);
  });

  describe('the chosen layer', () => {
    // Layer 0 has two strokes, layer 1 has two crossing them.
    const crossing = () =>
      docFromNote(
        note(
          [hline(0, 1000, 100, 100, 0), hline(0, 1000, 500, 100, 0), hline(0, 1000, 100, 100, 1), hline(0, 1000, 500, 100, 1)],
          2,
        ),
      );

    it('is the top layer when a note opens, and a new layer becomes the chosen one', () => {
      const doc = crossing();
      expect(doc.activeLayer).toBe(1);
      const added = addLayer(setActiveLayer(select(doc, [0]), 0), 'L3');
      expect(added.activeLayer).toBe(2);
      expect(added.selection).toEqual([]);
    });

    it('selects all the chosen layer’s strokes only (Ctrl+A)', () => {
      const doc = crossing();
      expect(selectAll(doc).selection).toEqual([2, 3]);
      expect(selectAll(setActiveLayer(doc, 0)).selection).toEqual([0, 1]);
    });

    it('selects nothing in a hidden chosen layer', () => {
      const doc = setLayerVisible(crossing(), 1, false);
      expect(doc.activeLayer).toBe(1);
      expect(selectAll(doc).selection).toEqual([]);
    });

    it('clears the selection when another layer is chosen', () => {
      const doc = select(crossing(), [2]);
      const other = setActiveLayer(doc, 0);
      expect(other.activeLayer).toBe(0);
      expect(other.selection).toEqual([]);
      // The same layer again changes nothing, the selection included.
      expect(setActiveLayer(doc, 1)).toBe(doc);
    });

    it('lets the eraser touch the chosen layer only, whole or partial', () => {
      const doc = crossing();
      const at = [{ x: 500, y: 100 }];
      const whole = erase(doc, at, 50, 'whole');
      expect(whole.note.strokes.map((s) => [s.layer, s.points[0].y])).toEqual([
        [0, 100],
        [0, 500],
        [1, 500],
      ]);
      const partial = erase(setActiveLayer(doc, 0), at, 150, 'partial');
      expect(partial.note.strokes.filter((s) => s.layer === 1)).toHaveLength(2);
      expect(partial.note.strokes.filter((s) => s.layer === 0).length).toBeGreaterThan(2);
      // Another layer's stroke is under the eraser, but nothing of the chosen layer is: nothing happens.
      expect(erase(doc, [{ x: 500, y: 900 }], 50, 'whole')).toBe(doc);
    });

    it('lets the eraser touch nothing while the chosen layer is hidden', () => {
      const doc = setLayerVisible(crossing(), 1, false);
      expect(erase(doc, [{ x: 500, y: 100 }], 50, 'whole')).toBe(doc);
      expect(erase(doc, [{ x: 500, y: 100 }], 150, 'partial')).toBe(doc);
    });

    it('follows its layer when layers move, and a merge takes it down', () => {
      let doc = crossing();
      doc = moveLayer(doc, 1, 0);
      expect(doc.activeLayer).toBe(0);
      doc = mergeDown(addLayer(doc, 'L3'), 2);
      expect(doc.activeLayer).toBe(1);
    });
  });

  it('rotates and crops the page', () => {
    const doc = docFromNote(note([hline(2000, 4000, 3000)]));
    expect(rotate(doc, 'cw').note.page).toEqual({ width: 21600, height: 29700 });
    expect(crop(doc, { x: 1000, y: 1000, width: 5000, height: 5000 }).note.page).toEqual({ width: 5000, height: 5000 });
    expect(crop(doc, { x: 0, y: 0, width: 10, height: 10 })).toBe(doc);
    expect(autoCrop(doc, 500).note.page).toEqual({ width: 3060, height: 1060 });
    expect(autoCrop(docFromNote(note([])))).toEqual(docFromNote(note([])));
  });
});

describe('custom colours', () => {
  function memoryStorage() {
    const data = new Map<string, string>();
    return {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => void data.set(k, v),
      data,
    };
  }

  it('converts hexadecimal colours', () => {
    expect(toHex({ r: 255, g: 16, b: 0, a: 255 })).toBe('#ff1000');
    expect(fromHex('#FF1000')).toEqual({ r: 255, g: 16, b: 0, a: 255 });
    expect(fromHex('nope')).toBeNull();
  });

  it('remembers custom colours, most recent first, without duplicates or palette colours', () => {
    const storage = memoryStorage();
    let list = rememberCustomColor({ r: 1, g: 2, b: 3, a: 255 }, [], storage);
    list = rememberCustomColor({ r: 4, g: 5, b: 6, a: 255 }, list, storage);
    list = rememberCustomColor({ r: 1, g: 2, b: 3, a: 255 }, list, storage);
    list = rememberCustomColor(INKSPACE_COLORS[2], list, storage);
    expect(list.map(toHex)).toEqual(['#010203', '#040506']);
    expect(loadCustomColors(storage).map(toHex)).toEqual(['#010203', '#040506']);
    for (let i = 0; i < 20; i++) list = rememberCustomColor({ r: 100 + i, g: 0, b: 0, a: 255 }, list, storage);
    expect(loadCustomColors(storage)).toHaveLength(MAX_CUSTOM_COLORS);
  });

  it('copes with broken or missing storage', () => {
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('full');
      },
    };
    expect(loadCustomColors(broken)).toEqual([]);
    expect(rememberCustomColor({ r: 1, g: 2, b: 3, a: 255 }, [], broken)).toHaveLength(1);
    expect(loadCustomColors(null)).toEqual([]);
    const storage = memoryStorage();
    storage.setItem(CUSTOM_COLORS_KEY, '{"not":"a list"}');
    expect(loadCustomColors(storage)).toEqual([]);
  });
});

describe('pasteStrokes at a time (M17)', () => {
  it('puts the copies at the given note time on the active layer, selected, as one step (O15)', () => {
    const doc = docFromNote(note([hline(1000, 3000, 2000)]));
    const copied = [hline(5000, 6000, 8000)];
    const next = pasteStrokes(doc, copied, 9000);
    expect(next.note.strokes).toHaveLength(2);
    expect(next.note.strokes[1].points[0].t).toBe(9000);
    expect(next.note.strokes[1].points.at(-1)!.t - next.note.strokes[1].points[0].t).toBe(
      copied[0].points.at(-1)!.t - copied[0].points[0].t,
    );
    expect(next.selection).toEqual([1]);
    // Without a time: after the note's ink, as before.
    const after = pasteStrokes(doc, copied);
    expect(after.note.strokes[1].points[0].t).toBe(doc.note.strokes[0].points.at(-1)!.t + 500);
  });
});
