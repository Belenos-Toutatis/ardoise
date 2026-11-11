import { describe, expect, it } from 'vitest';
import type { Note } from '../note';
import { drawingOrder, hitStroke, layerVisible, onLayer, selectInLasso, strokesTouched } from './hit';
import { setLayerVisible } from './ops';
import { hline, note } from './test-fixtures';

/** Two crossing pairs: strokes 0 and 1 on layer 0, strokes 2 and 3 on layer 1, 2 over 0 and 3 over 1. */
const crossing = (): Note =>
  note([hline(0, 1000, 100, 100, 0), hline(0, 1000, 500, 100, 0), hline(0, 1000, 100, 100, 1), hline(0, 1000, 500, 100, 1)], 2);

const around = [
  { x: -100, y: -100 },
  { x: 1100, y: -100 },
  { x: 1100, y: 700 },
  { x: -100, y: 700 },
];

describe('onLayer', () => {
  it('shares the strokes and layers and names the one layer', () => {
    const n = crossing();
    const only = onLayer(n, 1);
    expect(only.strokes).toBe(n.strokes);
    expect(only.layers).toBe(n.layers);
    expect(only.only).toBe(1);
    expect(n).not.toHaveProperty('only');
  });

  it('lets one layer’s strokes through, the lasso, the click, the eraser and the drawing order alike', () => {
    const n = crossing();
    expect(selectInLasso(n, around)).toEqual([0, 1, 2, 3]);
    expect(selectInLasso(onLayer(n, 1), around)).toEqual([2, 3]);
    expect(selectInLasso(onLayer(n, 0), around)).toEqual([0, 1]);
    // The click lands on both layers' strokes: the topmost of the chosen layer wins, whichever is above.
    expect(hitStroke(n, { x: 500, y: 100 }, 10)).toBe(2);
    expect(hitStroke(onLayer(n, 0), { x: 500, y: 100 }, 10)).toBe(0);
    expect(strokesTouched(n, [{ x: 500, y: 100 }], 50)).toEqual([0, 2]);
    expect(strokesTouched(onLayer(n, 1), [{ x: 500, y: 100 }], 50)).toEqual([2]);
    expect(drawingOrder(onLayer(n, 1))).toEqual([2, 3]);
  });

  it('finds nothing when the chosen layer is hidden', () => {
    const hidden = onLayer(setLayerVisible(crossing(), 1, false), 1);
    expect(layerVisible(hidden, 1)).toBe(false);
    expect(selectInLasso(hidden, around)).toEqual([]);
    expect(hitStroke(hidden, { x: 500, y: 100 }, 10)).toBeNull();
    expect(strokesTouched(hidden, [{ x: 500, y: 100 }], 50)).toEqual([]);
    expect(drawingOrder(hidden)).toEqual([]);
  });

  it('never hits a stroke whose layer has no entry (an orphan), unlike the whole page', () => {
    // Stroke 2 is on layer 5, which the note does not have.
    const n = note([hline(0, 1000, 100, 100, 0), hline(0, 1000, 500, 100, 1), hline(0, 1000, 300, 100, 5)], 2);
    expect(layerVisible(n, 5)).toBe(true);
    expect(selectInLasso(n, around)).toEqual([0, 1, 2]);
    expect(drawingOrder(n)).toEqual([0, 1, 2]);
    for (const layer of [0, 1, 5]) {
      const only = onLayer(n, layer);
      expect(selectInLasso(only, around).includes(2)).toBe(false);
      expect(hitStroke(only, { x: 500, y: 300 }, 10)).toBeNull();
      expect(strokesTouched(only, [{ x: 500, y: 300 }], 50)).toEqual([]);
      expect(drawingOrder(only).includes(2)).toBe(false);
    }
    // And a choice without entry sees no stroke at all.
    expect(layerVisible(onLayer(n, 5), 5)).toBe(false);
    expect(selectInLasso(onLayer(n, 5), around)).toEqual([]);
  });
});

describe('layerVisible', () => {
  it('is true for a layer that shows or has no entry, false for a hidden one', () => {
    const n = setLayerVisible(crossing(), 0, false);
    expect(layerVisible(n, 0)).toBe(false);
    expect(layerVisible(n, 1)).toBe(true);
    expect(layerVisible(n, 9)).toBe(true);
  });

  it('is true for the one layer only, and only while it shows', () => {
    const n = crossing();
    expect(layerVisible(onLayer(n, 1), 1)).toBe(true);
    expect(layerVisible(onLayer(n, 1), 0)).toBe(false);
    expect(layerVisible(onLayer(n, 1), 9)).toBe(false);
  });
});
