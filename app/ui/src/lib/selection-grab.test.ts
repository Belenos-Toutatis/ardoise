// What a press takes hold of on a selection's frame, with the reach of each pointer, and the maps a drag makes
// (shared by the editor and by the live control window).
import { describe, expect, it } from 'vitest';
import { applyAffine, type Rect } from './editor/geometry';
import type { Layer, Stroke } from './note';
import {
  GRAB_TOLERANCE,
  MAX_SCALE,
  MIN_SCALE,
  corners,
  grabAt,
  isNoTransform,
  moveTransform,
  resizeCursor,
  resizeFactor,
  scaleTransform,
  selectionFrame,
  toAffine,
  transformRect,
} from './selection-grab';

/** 200 × 100 page units, at 1 screen pixel per unit: 200 × 100 pixels. */
const FRAME: Rect = { x: 1000, y: 2000, width: 200, height: 100 };

describe('the reach of each pointer', () => {
  it('is wider for a pen than for a mouse, and for a finger than for a pen', () => {
    expect(GRAB_TOLERANCE.mouse).toEqual({ handle: 8, margin: 4 });
    expect(GRAB_TOLERANCE.pen).toEqual({ handle: 16, margin: 12 });
    expect(GRAB_TOLERANCE.touch).toEqual({ handle: 22, margin: 16 });
    expect(GRAB_TOLERANCE.eraser).toEqual(GRAB_TOLERANCE.pen);
  });

  it('takes a handle within the handle reach of its corner, square, the mouse at 8 pixels', () => {
    const near = (dx: number, dy: number) => grabAt(FRAME, { x: 1000 + dx, y: 2000 + dy }, 1, 'mouse');
    expect(near(0, 0)).toMatchObject({ kind: 'scale', corner: 0 });
    // Outside the frame, in the corner's quadrant, within the full reach.
    expect(near(-8, -8)).toMatchObject({ kind: 'scale', corner: 0 });
    // On the handle's square, outside on one axis and inside on the other: the corner still (a press on the corner
    // itself, rounded to the pixel, lands on either side of each edge).
    expect(near(-8, 8)).toMatchObject({ kind: 'scale', corner: 0 });
    expect(near(8, -8)).toMatchObject({ kind: 'scale', corner: 0 });
    expect(near(-0.4, 0.3)).toMatchObject({ kind: 'scale', corner: 0 });
    // Beside the frame, past the handle's square: the margin, 4 pixels, then nothing.
    expect(near(-3, 9)).toEqual({ kind: 'move' });
    expect(near(-9, 8)).toBeNull();
    expect(near(8, 8)).toMatchObject({ kind: 'scale', corner: 0 });
    // Past it: inside the frame it moves.
    expect(near(9, 9)).toEqual({ kind: 'move' });
    // Past it, outside: nothing.
    expect(near(-9, -9)).toBeNull();
  });

  it('the pen reaches a handle at 16 pixels and the finger at 22', () => {
    const at = (kind: 'pen' | 'touch', dx: number) => grabAt(FRAME, { x: 1200 + dx, y: 2100 }, 1, kind);
    expect(at('pen', 16)).toMatchObject({ kind: 'scale', corner: 2 });
    expect(at('pen', 17)).toBeNull();
    expect(at('touch', 22)).toMatchObject({ kind: 'scale', corner: 2 });
    expect(at('touch', 23)).toBeNull();
    // The mouse does not reach so far.
    expect(grabAt(FRAME, { x: 1200 + 16, y: 2100 }, 1, 'mouse')).toBeNull();
  });

  it('takes the frame within a margin around it: 4 pixels for the mouse, 12 for the pen, 16 for a finger', () => {
    const at = (kind: 'mouse' | 'pen' | 'touch', dx: number) => grabAt(FRAME, { x: 1000 + dx, y: 2050 }, 1, kind);
    expect(at('mouse', -4)).toEqual({ kind: 'move' });
    expect(at('mouse', -5)).toBeNull();
    expect(at('pen', -12)).toEqual({ kind: 'move' });
    expect(at('pen', -13)).toBeNull();
    expect(at('touch', -16)).toEqual({ kind: 'move' });
    expect(at('touch', -17)).toBeNull();
    // The far side too, and above.
    expect(grabAt(FRAME, { x: 1200 + 12, y: 2050 }, 1, 'pen')).toEqual({ kind: 'move' });
    expect(grabAt(FRAME, { x: 1100, y: 2000 - 12 }, 1, 'pen')).toEqual({ kind: 'move' });
    expect(grabAt(FRAME, { x: 1100, y: 2100 + 13 }, 1, 'pen')).toBeNull();
  });

  it('measures the reach in screen pixels: the same press is farther in page units when zoomed out', () => {
    // At 0.1 pixel per unit the frame is 20 × 10 pixels; a pen 12 pixels away is 120 units away.
    const small: Rect = { x: 0, y: 0, width: 2000, height: 1000 };
    expect(grabAt(small, { x: -120, y: 500 }, 0.1, 'pen')).toEqual({ kind: 'move' });
    expect(grabAt(small, { x: -130, y: 500 }, 0.1, 'pen')).toBeNull();
    expect(grabAt(small, { x: 1000, y: 500 }, 0, 'pen')).toBeNull();
  });
});

describe('what a press takes hold of', () => {
  it('a handle resizes around the opposite corner', () => {
    const cs = corners(FRAME);
    cs.forEach((corner, k) => {
      expect(grabAt(FRAME, corner, 1, 'pen'), `corner ${k}`).toEqual({ kind: 'scale', corner: k, anchor: cs[(k + 2) % 4] });
    });
  });

  it('inside the frame it moves, and the nearer handle wins when two are in reach', () => {
    expect(grabAt(FRAME, { x: 1100, y: 2050 }, 1, 'mouse')).toEqual({ kind: 'move' });
    // A narrow frame: 30 pixels wide, a finger's reach would take both ends; the nearer one wins.
    const narrow: Rect = { x: 0, y: 0, width: 30, height: 300 };
    expect(grabAt(narrow, { x: 20, y: 5 }, 1, 'touch')).toMatchObject({ kind: 'scale', corner: 1 });
    expect(grabAt(narrow, { x: 10, y: 5 }, 1, 'touch')).toMatchObject({ kind: 'scale', corner: 0 });
  });

  it('a small frame is not all handle: from inside, a corner reaches a third of the shorter side, so that it can still be moved', () => {
    const tiny: Rect = { x: 0, y: 0, width: 30, height: 30 };
    // 10 pixels from each corner of a 30-pixel frame: the finger's 22 would cover everything.
    expect(grabAt(tiny, { x: 15, y: 15 }, 1, 'touch')).toEqual({ kind: 'move' });
    expect(grabAt(tiny, { x: 9, y: 9 }, 1, 'touch')).toMatchObject({ kind: 'scale', corner: 0 });
  });

  it('the centre of a dot (a 4 × 4 frame) moves it, and just outside a corner resizes, for every pointer', () => {
    const dot: Rect = { x: 100, y: 100, width: 4, height: 4 };
    for (const kind of ['mouse', 'pen', 'touch', 'eraser'] as const) {
      expect(grabAt(dot, { x: 102, y: 102 }, 1, kind), kind).toEqual({ kind: 'move' });
      // Just outside each corner, in its quadrant.
      expect(grabAt(dot, { x: 99, y: 99 }, 1, kind), kind).toMatchObject({ kind: 'scale', corner: 0 });
      expect(grabAt(dot, { x: 105, y: 99 }, 1, kind), kind).toMatchObject({ kind: 'scale', corner: 1 });
      expect(grabAt(dot, { x: 105, y: 105 }, 1, kind), kind).toMatchObject({ kind: 'scale', corner: 2 });
      expect(grabAt(dot, { x: 99, y: 105 }, 1, kind), kind).toMatchObject({ kind: 'scale', corner: 3 });
      // On a corner, half a pixel outside on one axis and inside on the other.
      expect(grabAt(dot, { x: 104.4, y: 103.7 }, 1, kind), kind).toMatchObject({ kind: 'scale', corner: 2 });
    }
    // The outer quadrant reaches as far as the pointer's handle reach, and no farther.
    expect(grabAt(dot, { x: 100 - 16, y: 100 - 16 }, 1, 'pen')).toMatchObject({ kind: 'scale', corner: 0 });
    expect(grabAt(dot, { x: 100 - 17, y: 100 - 17 }, 1, 'pen')).toBeNull();
  });

  it('the resize cursor is the diagonal the handle lies on', () => {
    expect([0, 1, 2, 3].map(resizeCursor)).toEqual(['nwse-resize', 'nesw-resize', 'nwse-resize', 'nesw-resize']);
  });
});

describe('the maps a drag makes', () => {
  it('a resize factor is the pointer projected onto the diagonal: 1 where it was pressed, 0 at the anchor', () => {
    const anchor = { x: 0, y: 0 };
    const start = { x: 100, y: 100 };
    expect(resizeFactor(anchor, start, start)).toBe(1);
    expect(resizeFactor(anchor, start, { x: 200, y: 200 })).toBe(2);
    expect(resizeFactor(anchor, start, { x: 150, y: 50 })).toBe(1);
    expect(resizeFactor(anchor, start, { x: 50, y: 50 })).toBe(0.5);
    // Dragged past the anchor, or onto it: not smaller than the least.
    expect(resizeFactor(anchor, start, { x: -300, y: -300 })).toBe(MIN_SCALE);
    expect(resizeFactor(anchor, start, anchor)).toBe(MIN_SCALE);
    // Not larger than the most when asked.
    expect(resizeFactor(anchor, start, { x: 5000, y: 5000 }, MAX_SCALE)).toBe(MAX_SCALE);
    // A press on the anchor itself (an empty frame): nothing to project onto.
    expect(resizeFactor(anchor, anchor, { x: 9, y: 9 })).toBe(1);
  });

  it('a move is a translation, a resize a scaling that keeps the anchor where it is', () => {
    expect(moveTransform(30, -12)).toEqual({ scale: 1, x: 30, y: -12 });
    const t = scaleTransform({ x: 1200, y: 2100 }, 0.5);
    expect(t).toEqual({ scale: 0.5, x: 600, y: 1050 });
    expect(applyAffine(toAffine(t), { x: 1200, y: 2100 })).toEqual({ x: 1200, y: 2100 });
    expect(applyAffine(toAffine(t), { x: 1000, y: 2000 })).toEqual({ x: 1100, y: 2050 });
  });

  it('the frame under a map: the corners go where the strokes go', () => {
    expect(transformRect(FRAME, moveTransform(10, 20))).toEqual({ x: 1010, y: 2020, width: 200, height: 100 });
    const around = transformRect(FRAME, scaleTransform({ x: 1200, y: 2100 }, 2));
    expect(around).toEqual({ x: 800, y: 1900, width: 400, height: 200 });
  });

  it('a map that moves less than a hundredth of a pixel and keeps the size is no change', () => {
    expect(isNoTransform({ scale: 1, x: 0, y: 0 }, 0.03)).toBe(true);
    expect(isNoTransform({ scale: 1, x: 1, y: 0 }, 0.03)).toBe(true);
    expect(isNoTransform({ scale: 1, x: 5, y: 0 }, 0.03)).toBe(false);
    expect(isNoTransform({ scale: 1.01, x: 0, y: 0 }, 0.03)).toBe(false);
  });
});

describe('the frame of the chosen strokes', () => {
  const LAYERS: Layer[] = [
    { name: 'Calque 1', visible: true },
    { name: 'Calque 2', visible: false },
  ];
  const line = (x: number, y: number, layer = 0): Stroke => ({
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer,
    points: [
      { x, y, pressure: 0.6, t: 0 },
      { x: x + 1000, y, pressure: 0.6, t: 1 },
    ],
  });

  it('is the box of the ink: the samples plus half the width', () => {
    expect(selectionFrame([line(1000, 2000)], LAYERS, [0])).toEqual({ x: 970, y: 1970, width: 1060, height: 60 });
    expect(selectionFrame([line(1000, 2000), line(5000, 6000)], LAYERS, [0, 1])).toEqual({ x: 970, y: 1970, width: 5060, height: 4060 });
  });

  it('leaves out strokes of a hidden layer and indices that are not there; null when nothing shows', () => {
    const strokes = [line(1000, 2000), line(5000, 6000, 1)];
    expect(selectionFrame(strokes, LAYERS, [0, 1, 9])).toEqual({ x: 970, y: 1970, width: 1060, height: 60 });
    expect(selectionFrame(strokes, LAYERS, [1])).toBeNull();
    expect(selectionFrame(strokes, LAYERS, [])).toBeNull();
  });
});
