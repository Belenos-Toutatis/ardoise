import { describe, expect, it } from 'vitest';
import {
  MARK_WIDTH_PX,
  MAX_SHARED_MARKS,
  MAX_SHARED_MARK_POINTS,
  MAX_SHARED_POINTS,
  eraseMarks,
  markStyle,
  marksFromWire,
  marksToWire,
  newMark,
  reuseMarks,
} from './live-marks';
import type { Stroke } from './note';

const at = (x: number, y: number) => ({ x, y, pressure: 0.6, t: 0 });
function mark(points: [number, number][]): Stroke {
  const first = newMark(markStyle(false), at(...points[0]), 0.1);
  first.points = points.map(([x, y]) => at(x, y));
  return first;
}

describe('presentation marks', () => {
  it('is red on a light page and yellow on a dark one', () => {
    expect(markStyle(false).color).toMatchObject({ r: 0xd8, g: 0, b: 0 });
    expect(markStyle(true).color).toMatchObject({ r: 255, g: 214, b: 0 });
  });

  it('has the same width on the screen whatever the zoom', () => {
    expect(newMark(markStyle(false), at(0, 0), 0.1).width).toBeCloseTo(MARK_WIDTH_PX / 0.1);
    expect(newMark(markStyle(false), at(0, 0), 0.4).width).toBeCloseTo(MARK_WIDTH_PX / 0.4);
    expect(newMark(markStyle(false), at(0, 0), 0.1).layer).toBe(0);
  });

  it('erases the marks the eraser passes over, whole, and keeps the others', () => {
    const a = mark([
      [0, 0],
      [1000, 0],
    ]);
    const b = mark([
      [0, 2000],
      [1000, 2000],
    ]);
    const marks = [a, b];
    expect(eraseMarks(marks, [{ x: 500, y: 100 }], 300)).toEqual([b]);
    expect(eraseMarks(marks, [{ x: 500, y: 1000 }], 300)).toBe(marks);
    expect(
      eraseMarks(
        marks,
        [
          { x: 500, y: 1000 },
          { x: 500, y: 2050 },
        ],
        300,
      ),
    ).toEqual([a]);
    expect(eraseMarks([], [{ x: 0, y: 0 }], 300)).toEqual([]);
  });

  it('counts the half width of a thick mark', () => {
    const thick = {
      ...mark([
        [0, 0],
        [1000, 0],
      ]),
      width: 400,
    };
    // Radius 100 + half width 200 = 300 of reach.
    expect(eraseMarks([thick], [{ x: 500, y: 250 }], 100)).toEqual([]);
    expect(eraseMarks([thick], [{ x: 500, y: 350 }], 100)).toEqual([thick]);
  });
});

describe('marks for the viewers of the share', () => {
  const line = (n: number, x0 = 0): Stroke => ({
    ...mark([[x0, 0]]),
    points: Array.from({ length: n }, (_, i) => ({ x: x0 + i * 10.04, y: 500.06, pressure: 0.5678, t: 7 * i })),
  });

  it('sends the ink and the points only, rounded', () => {
    const [wire] = marksToWire([{ ...line(2), width: 138.46 }]);
    expect(wire).toEqual({
      tool: 'feltTip',
      color: { r: 0xd8, g: 0, b: 0, a: 255 },
      width: 138.5,
      points: [
        { x: 0, y: 500.1, p: 0.57 },
        { x: 10, y: 500.1, p: 0.57 },
      ],
    });
    expect(Object.keys(wire).sort()).toEqual(['color', 'points', 'tool', 'width']);
    expect(Object.keys(wire.points[0]).sort()).toEqual(['p', 'x', 'y']);
  });

  it('keeps within what Rust keeps: the first marks, the first points', () => {
    const many = Array.from({ length: MAX_SHARED_MARKS + 20 }, (_, i) => line(2, i));
    const wire = marksToWire(many);
    expect(wire).toHaveLength(MAX_SHARED_MARKS);
    expect(wire[MAX_SHARED_MARKS - 1].points[0].x).toBe(MAX_SHARED_MARKS - 1);
    expect(marksToWire([line(MAX_SHARED_MARK_POINTS + 5)])[0].points).toHaveLength(MAX_SHARED_MARK_POINTS);
    const wide = marksToWire(Array.from({ length: 10 }, () => line(MAX_SHARED_MARK_POINTS)));
    expect(wide.reduce((n, m) => n + m.points.length, 0)).toBe(MAX_SHARED_POINTS);
    expect(marksToWire([])).toEqual([]);
    expect(marksToWire([line(0)])).toEqual([]);
  });

  it('comes back as strokes the renderer draws', () => {
    const strokes = marksFromWire(marksToWire([line(3)]));
    expect(strokes).toHaveLength(1);
    expect(strokes[0]).toMatchObject({ tool: 'feltTip', layer: 0, color: { r: 0xd8, g: 0, b: 0, a: 255 } });
    expect(strokes[0].points[1]).toEqual({ x: 10, y: 500.1, pressure: 0.57, t: 0 });
  });

  it('keeps the strokes that did not change, so that only the one that grew is outlined again', () => {
    const before = marksFromWire(marksToWire([line(3), line(4, 100), line(2, 300)]));
    const after = reuseMarks(before, marksFromWire(marksToWire([line(3), line(4, 100), line(5, 300)])));
    expect(after[0]).toBe(before[0]);
    expect(after[1]).toBe(before[1]);
    expect(after[2]).not.toBe(before[2]);
    expect(after[2].points).toHaveLength(5);
    // A mark erased in the middle: what is no longer the same is not mistaken for it.
    const erased = reuseMarks(before, marksFromWire(marksToWire([line(3), line(2, 300)])));
    expect(erased[0]).toBe(before[0]);
    expect(erased[1]).not.toBe(before[1]);
    expect(erased[1].points).toHaveLength(2);
  });
});
