import { describe, expect, it } from 'vitest';
import { drawingOrder, hitStroke, selectInLasso, strokesTouched } from './hit';
import {
  addLayer,
  addStroke,
  autoCropRect,
  cropTo,
  eraseWhole,
  erasePartial,
  insertCopies,
  mergeDown,
  moveLayer,
  removeStrokes,
  renameLayer,
  retimedForPaste,
  rotateQuarter,
  setBackground,
  setLayerVisible,
  setStyle,
  strokesBounds,
  transformStrokes,
} from './ops';
import type { Note } from '../note';
import { scalingAround, translation } from './geometry';
import { RED, hline, line, note } from './test-fixtures';

const square = (x: number, y: number, size: number) => [
  { x, y },
  { x: x + size, y },
  { x: x + size, y: y + size },
  { x, y: y + size },
];

describe('strokes', () => {
  it('adds and removes strokes, keeping untouched stroke objects', () => {
    const a = hline(0, 1000, 100);
    const b = hline(0, 1000, 200);
    const n = addStroke(note([a]), b);
    expect(n.strokes).toEqual([a, b]);
    const removed = removeStrokes(n, [0]);
    expect(removed.strokes).toHaveLength(1);
    expect(removed.strokes[0]).toBe(b);
    expect(removeStrokes(n, [])).toBe(n);
  });

  it('recolours, retools and resizes a selection or the whole note', () => {
    const n = note([hline(0, 1000, 100), hline(0, 1000, 200)]);
    const one = setStyle(n, [1], { color: RED });
    expect(one.strokes[0]).toBe(n.strokes[0]);
    expect(one.strokes[1].color).toEqual(RED);
    const all = setStyle(n, null, { tool: 'pencil', width: 35 });
    expect(all.strokes.every((s) => s.tool === 'pencil' && s.width === 35)).toBe(true);
    expect(all.strokes[0].color).toEqual(n.strokes[0].color);
    // Nothing to change: the same note.
    expect(setStyle(n, null, { tool: 'ballpoint' })).toBe(n);
  });

  it('moves and scales strokes, widths following the scale', () => {
    const n = note([hline(0, 1000, 100), hline(0, 1000, 200)]);
    const moved = transformStrokes(n, [0], translation(50, -20));
    expect(moved.strokes[0].points[0]).toMatchObject({ x: 50, y: 80, pressure: 0.5, t: 0 });
    expect(moved.strokes[1]).toBe(n.strokes[1]);
    const scaled = transformStrokes(n, [1], scalingAround({ x: 0, y: 200 }, 2));
    expect(scaled.strokes[1].points.at(-1)).toMatchObject({ x: 2000, y: 200 });
    expect(scaled.strokes[1].width).toBe(120);
  });

  it('inserts shifted copies on a layer', () => {
    const n = note([hline(0, 1000, 100)], 2);
    const { note: out, indices } = insertCopies(n, [n.strokes[0]], { x: 400, y: 400 }, 1);
    expect(indices).toEqual([1]);
    expect(out.strokes[1].layer).toBe(1);
    expect(out.strokes[1].points[0]).toMatchObject({ x: 400, y: 500 });
    expect(out.strokes[1].color).not.toBe(n.strokes[0].color);
  });

  it('bounds strokes, ignoring hidden layers for the whole note', () => {
    const n = note([hline(1000, 2000, 1000), hline(5000, 6000, 3000, 100, 1)], 2);
    expect(strokesBounds(n, null)).toEqual({ x: 970, y: 970, width: 5060, height: 2060 });
    const hidden = setLayerVisible(n, 1, false);
    expect(strokesBounds(hidden, null)).toEqual({ x: 970, y: 970, width: 1060, height: 60 });
    expect(strokesBounds(hidden, [1])).toEqual({ x: 4970, y: 2970, width: 1060, height: 60 });
  });
});

describe('hit tests', () => {
  it('selects strokes with most of their samples inside the lasso', () => {
    const n = note([hline(1000, 2000, 1000), hline(1000, 2000, 5000), hline(1500, 4000, 1500)]);
    // Covers the first stroke, misses the second, holds less than half of the third.
    expect(selectInLasso(n, square(500, 500, 2000))).toEqual([0]);
    expect(selectInLasso(n, square(500, 500, 2000), 0.2)).toEqual([0, 2]);
    expect(selectInLasso(n, [])).toEqual([]);
  });

  it('never selects strokes of hidden layers', () => {
    const n = setLayerVisible(note([hline(1000, 2000, 1000, 100, 0), hline(1000, 2000, 1200, 100, 1)], 2), 1, false);
    expect(selectInLasso(n, square(0, 0, 5000))).toEqual([0]);
    expect(drawingOrder(n)).toEqual([0]);
  });

  it('draws layer by layer, then in file order, unknown layers on top', () => {
    const n = note([line([[0, 0]], 1), line([[0, 0]], 0), line([[0, 0]], 7), line([[0, 0]], 0)], 2);
    expect(drawingOrder(n)).toEqual([1, 3, 0, 2]);
  });

  it('finds the topmost stroke under a click', () => {
    const n = note([hline(0, 2000, 1000, 100, 1), hline(0, 2000, 1010, 100, 0)], 2);
    // Layer 1 is above layer 0 even though it comes first in the file.
    expect(hitStroke(n, { x: 1000, y: 1005 }, 10)).toBe(0);
    expect(hitStroke(n, { x: 1000, y: 1500 }, 10)).toBeNull();
    expect(hitStroke(n, { x: 1000, y: 1045 }, 10)).toBe(1);
  });

  it('finds strokes within reach of the eraser path', () => {
    const n = note([hline(0, 2000, 1000), hline(0, 2000, 2000)]);
    // Radius 100 plus half the stroke width (30) reaches 130 units.
    expect(strokesTouched(n, [{ x: 500, y: 1120 }], 100)).toEqual([0]);
    expect(strokesTouched(n, [{ x: 500, y: 1140 }], 100)).toEqual([]);
    expect(
      strokesTouched(
        n,
        [
          { x: 3000, y: 0 },
          { x: -1000, y: 3000 },
        ],
        10,
      ),
    ).toEqual([0, 1]);
  });
});

describe('eraser', () => {
  it('removes whole strokes touched by the eraser', () => {
    const n = note([hline(0, 2000, 1000), hline(0, 2000, 2000)]);
    const erased = eraseWhole(n, [{ x: 1000, y: 1000 }], 50);
    expect(erased.strokes).toEqual([n.strokes[1]]);
    expect(eraseWhole(n, [{ x: 1000, y: 5000 }], 50)).toBe(n);
  });

  it('cuts a stroke in two where the eraser passes, in place', () => {
    const before = hline(0, 900, 1000, 100, 0);
    const n = note([hline(0, 900, 500), before, hline(0, 900, 1500)]);
    const out = erasePartial(
      n,
      [
        { x: 450, y: 0 },
        { x: 450, y: 3000 },
      ],
      120,
    );
    // The first and last strokes are crossed too: three strokes become six.
    expect(out.strokes).toHaveLength(6);
    const left = out.strokes[2];
    const right = out.strokes[3];
    expect(left.points.every((p) => p.x < 450 - 120)).toBe(true);
    expect(right.points.every((p) => p.x > 450 + 120)).toBe(true);
    expect(left.points[0]).toEqual(before.points[0]);
    expect(right.points.at(-1)).toEqual(before.points.at(-1));
    expect(left.color).toEqual(before.color);
  });

  it('cuts long segments too, and drops fragments of a single sample', () => {
    const n = note([line([[0, 0], [10000, 0]])]);
    const out = erasePartial(n, [{ x: 5000, y: 0 }], 200);
    expect(out.strokes).toHaveLength(2);
    expect(Math.max(...out.strokes[0].points.map((p) => p.x))).toBeLessThan(4800 + 1e-6);
    expect(Math.min(...out.strokes[1].points.map((p) => p.x))).toBeGreaterThan(5200 - 1e-6);
    // Erasing the end of a short stroke leaves nothing worth keeping.
    expect(erasePartial(note([line([[0, 0], [100, 0]])]), [{ x: 100, y: 0 }], 80).strokes).toEqual([]);
  });

  it('gives the samples it adds whole, non-negative times (Rust stores them as u32)', () => {
    // 10 ms between two samples 1000 units apart: cuts every 50 units fall on fractions of a ms.
    const n = note([line([[0, 0], [1000, 0], [1000, 700]])]);
    const out = erasePartial(n, [{ x: 500, y: 0 }], 100);
    expect(out.strokes.length).toBeGreaterThan(1);
    const times = out.strokes.flatMap((s) => s.points.map((p) => p.t));
    expect(times.length).toBeGreaterThan(10);
    expect(times.every((t) => Number.isInteger(t) && t >= 0)).toBe(true);
  });

  it('touches only the strokes of the layer it is given, whole or partial', () => {
    const n = note([hline(0, 900, 1000, 100, 0), hline(0, 900, 1000, 100, 1)], 2);
    const path = [{ x: 450, y: 1000 }];
    expect(eraseWhole(n, path, 50, 1).strokes).toEqual([n.strokes[0]]);
    expect(eraseWhole(n, path, 50, 0).strokes).toEqual([n.strokes[1]]);
    expect(eraseWhole(n, path, 50).strokes).toEqual([]);
    const cut = erasePartial(n, path, 100, 0);
    expect(cut.strokes[cut.strokes.length - 1]).toBe(n.strokes[1]);
    expect(cut.strokes.slice(0, -1).every((s) => s.layer === 0)).toBe(true);
    expect(cut.strokes.length).toBe(3);
    // Nothing of the chosen layer under the eraser: the note is returned as it is.
    expect(eraseWhole(n, [{ x: 450, y: 5000 }], 50, 1)).toBe(n);
    expect(erasePartial(n, [{ x: 450, y: 5000 }], 100, 1)).toBe(n);
  });

  it('touches nothing of a hidden layer, or of a layer the note does not have', () => {
    const n = setLayerVisible(note([hline(0, 900, 1000, 100, 1)], 2), 1, false);
    expect(eraseWhole(n, [{ x: 450, y: 1000 }], 50, 1)).toBe(n);
    expect(erasePartial(n, [{ x: 450, y: 1000 }], 100, 1)).toBe(n);
    const orphan = note([hline(0, 900, 1000, 100, 4)], 2);
    expect(eraseWhole(orphan, [{ x: 450, y: 1000 }], 50, 4)).toBe(orphan);
    expect(erasePartial(orphan, [{ x: 450, y: 1000 }], 100, 4)).toBe(orphan);
  });

  it('leaves untouched notes and hidden layers alone', () => {
    const n = note([hline(0, 900, 1000)]);
    expect(erasePartial(n, [{ x: 5000, y: 5000 }], 100)).toBe(n);
    const hidden = setLayerVisible(n, 0, false);
    expect(erasePartial(hidden, [{ x: 450, y: 1000 }], 100)).toBe(hidden);
  });
});

describe('layers', () => {
  const three = () =>
    note([line([[0, 0]], 0), line([[1, 1]], 1), line([[2, 2]], 2), line([[3, 3]], 1)], 3);

  it('adds, renames, hides and shows layers', () => {
    const n = addLayer(three(), 'Top');
    expect(n.layers.map((l) => l.name)).toEqual(['L1', 'L2', 'L3', 'Top']);
    expect(renameLayer(n, 0, 'Base').layers[0].name).toBe('Base');
    expect(renameLayer(n, 0, 'L1')).toBe(n);
    expect(setLayerVisible(n, 1, false).layers[1].visible).toBe(false);
    expect(setLayerVisible(n, 1, true)).toBe(n);
  });

  it('reorders layers, strokes following theirs', () => {
    const n = three();
    const moved = moveLayer(n, 0, 2);
    expect(moved.layers.map((l) => l.name)).toEqual(['L2', 'L3', 'L1']);
    expect(moved.strokes.map((s) => s.layer)).toEqual([2, 0, 1, 0]);
    const back = moveLayer(moved, 2, 0);
    expect(back.layers).toEqual(n.layers);
    expect(back.strokes.map((s) => s.layer)).toEqual([0, 1, 2, 1]);
    expect(moveLayer(n, 1, 1)).toBe(n);
    expect(moveLayer(n, 1, 5)).toBe(n);
  });

  it('merges a layer down, keeping it drawn above', () => {
    const n = three();
    const merged = mergeDown(n, 1);
    expect(merged.layers.map((l) => l.name)).toEqual(['L1', 'L3']);
    // Strokes of the merged layer come last, so they stay above the lower layer's.
    expect(merged.strokes.map((s) => [s.points[0].x, s.layer])).toEqual([
      [0, 0],
      [2, 1],
      [1, 0],
      [3, 0],
    ]);
    expect(drawingOrder(merged).map((i) => merged.strokes[i].points[0].x)).toEqual([0, 1, 3, 2]);
    expect(mergeDown(n, 0)).toBe(n);
  });
});

describe('page', () => {
  it('changes the background', () => {
    const n = note([]);
    const out = setBackground(n, { paper: 'slate', template: 'seyes', texture: true });
    expect(out.background).toEqual({ paper: 'slate', template: 'seyes', texture: true });
    expect(setBackground(n, { paper: 'white', template: 'blank', texture: false })).toBe(n);
  });

  it('rotates a quarter turn: points and page size', () => {
    const n = note([line([[1000, 2000], [3000, 500]])], 1, { width: 29700, height: 21600 });
    const cw = rotateQuarter(n, 'cw');
    expect(cw.page).toEqual({ width: 21600, height: 29700 });
    // Top-left corner goes to the top-right.
    expect(cw.strokes[0].points.map((p) => [p.x, p.y])).toEqual([
      [21600 - 2000, 1000],
      [21600 - 500, 3000],
    ]);
    const ccw = rotateQuarter(n, 'ccw');
    expect(ccw.page).toEqual({ width: 21600, height: 29700 });
    expect(ccw.strokes[0].points.map((p) => [p.x, p.y])).toEqual([
      [2000, 29700 - 1000],
      [500, 29700 - 3000],
    ]);
    expect(rotateQuarter(cw, 'ccw')).toEqual(n);
  });

  it('gets back to the start after two half turns', () => {
    const n = note([line([[1000, 2000], [3000, 500], [123.25, 77.5]])]);
    const half = rotateQuarter(rotateQuarter(n, 'cw'), 'cw');
    expect(half.page).toEqual(n.page);
    expect(half.strokes[0].points[0]).toMatchObject({ x: 29700 - 1000, y: 21600 - 2000 });
    const full = rotateQuarter(rotateQuarter(half, 'cw'), 'cw');
    expect(full).toEqual(n);
    const fourCcw = [0, 1, 2, 3].reduce((acc) => rotateQuarter(acc, 'ccw'), n);
    expect(fourCcw).toEqual(n);
  });

  it('crops to a rectangle, dropping strokes outside it', () => {
    const n = note([hline(2000, 4000, 3000), hline(20000, 22000, 15000)]);
    const out = cropTo(n, { x: 1000.4, y: 2000, width: 5000, height: 4000 });
    expect(out.page).toEqual({ width: 5000, height: 4000 });
    expect(out.strokes).toHaveLength(1);
    expect(out.strokes[0].points[0]).toMatchObject({ x: 1000, y: 1000 });
    // Too small, or the whole page: nothing changes.
    expect(cropTo(n, { x: 0, y: 0, width: 500, height: 4000 })).toBe(n);
    expect(cropTo(n, { x: 0, y: 0, width: 29700, height: 21600 })).toBe(n);
  });

  it('keeps strokes on hidden layers when cropping, even outside the new page', () => {
    const visibleOut = hline(20000, 22000, 15000, 100, 0);
    const hiddenOut = hline(20000, 22000, 16000, 100, 1);
    const hiddenIn = hline(2000, 3000, 3000, 100, 1);
    const n = setLayerVisible(note([hline(2000, 4000, 3000, 100, 0), visibleOut, hiddenOut, hiddenIn], 2), 1, false);
    const out = cropTo(n, { x: 1000, y: 2000, width: 5000, height: 4000 });
    // Only the visible stroke outside the rectangle goes.
    expect(out.strokes).toHaveLength(3);
    expect(out.strokes.map((s) => s.layer)).toEqual([0, 1, 1]);
    // Hidden strokes move with the page.
    expect(out.strokes[1].points[0]).toMatchObject({ x: 19000, y: 14000 });
    expect(out.strokes[2].points[0]).toMatchObject({ x: 1000, y: 1000 });
  });

  it('frames the automatic crop on visible strokes only, keeping hidden ones', () => {
    const n = setLayerVisible(note([hline(2000, 4000, 3000, 100, 0), hline(20000, 22000, 15000, 100, 1)], 2), 1, false);
    const rect = autoCropRect(n, 500)!;
    expect(rect).toEqual(autoCropRect(note([hline(2000, 4000, 3000)]), 500));
    expect(cropTo(n, rect).strokes).toHaveLength(2);
  });

  it('finds the automatic crop around the content', () => {
    const n = note([hline(2000, 4000, 3000), hline(5000, 9000, 8000)]);
    expect(autoCropRect(n, 500)).toEqual({ x: 1470, y: 2470, width: 8060, height: 6060 });
    // Clamped to the page.
    expect(autoCropRect(note([hline(0, 4000, 100)]), 500)).toEqual({ x: 0, y: 0, width: 4530, height: 1000 });
    expect(autoCropRect(note([]))).toBeNull();
    const cropped = cropTo(n, autoCropRect(n, 500)!);
    expect(cropped.page).toEqual({ width: 8060, height: 6060 });
    expect(cropped.strokes[0].points[0]).toMatchObject({ x: 530, y: 530 });
  });
});

describe('the background image follows page operations', () => {
  const withImage = (): Note => ({
    ...note([hline(1000, 4000, 3000)]),
    backgroundImage: {
      name: 'fiche.pdf',
      pdfPage: 1,
      rect: { x: -500, y: 1000, width: 12_000, height: 9_000 },
      quarterTurns: 0,
      opacity: 0.8,
    },
  });

  it('turns with the strokes, a quarter turn more each time', () => {
    const n = withImage();
    const cw = rotateQuarter(n, 'cw');
    expect(cw.backgroundImage?.quarterTurns).toBe(1);
    // The corner of the rectangle under a sample moves exactly as the sample does.
    const corner = { x: -500, y: 1000 };
    const moved = { x: n.page.height - corner.y, y: corner.x };
    const r = cw.backgroundImage!.rect;
    expect({ x: r.x + r.width, y: r.y }).toEqual(moved);
    expect(rotateQuarter(cw, 'ccw')).toEqual(n);
    const full = [0, 1, 2, 3].reduce((acc) => rotateQuarter(acc, 'cw'), n);
    expect(full).toEqual(n);
    expect(rotateQuarter(n, 'ccw').backgroundImage?.quarterTurns).toBe(3);
    // A note without an image gets none.
    expect('backgroundImage' in rotateQuarter(note([]), 'cw')).toBe(false);
  });

  it('is shifted by a crop, like the strokes', () => {
    const n = withImage();
    const out = cropTo(n, { x: 800, y: 2000, width: 5000, height: 4000 });
    expect(out.backgroundImage?.rect).toEqual({ x: -1300, y: -1000, width: 12_000, height: 9_000 });
    expect(out.strokes[0].points[0]).toMatchObject({ x: 200, y: 1000 });
    expect(out.backgroundImage?.opacity).toBe(0.8);
    expect('backgroundImage' in cropTo(note([hline(1000, 4000, 3000)]), { x: 800, y: 2000, width: 5000, height: 4000 })).toBe(false);
  });
});

describe('retimedForPaste at a given time (M17)', () => {
  const copied = [
    line([
      [0, 0],
      [5, 5],
      [9, 9],
    ]),
    {
      ...line([
        [1, 1],
        [6, 6],
      ]),
      points: [
        { x: 1, y: 1, pressure: 0.5, t: 100 },
        { x: 6, y: 6, pressure: 0.5, t: 140 },
      ],
    },
  ];

  it('puts the first sample at that note time, keeping the rhythm (a paste during a writing recording, O15)', () => {
    const n = note([line([[0, 0], [10, 10]])]);
    const moved = retimedForPaste(n, copied, 12_345.6);
    expect(moved[0].points.map((p) => p.t)).toEqual([12_346, 12_356, 12_366]);
    expect(moved[1].points.map((p) => p.t)).toEqual([12_446, 12_486]);
    // The originals are untouched.
    expect(copied[0].points[0].t).toBe(0);
  });

  it('never goes below 0, and without a time keeps the rule of before', () => {
    const n = note([line([[0, 0], [10, 10]])]);
    expect(retimedForPaste(n, copied, -50)[0].points[0].t).toBe(0);
    expect(retimedForPaste(n, copied)[0].points[0].t).toBe(10 + 500);
    expect(retimedForPaste(n, copied, Number.NaN)[0].points[0].t).toBe(10 + 500);
  });
});
