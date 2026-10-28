import { describe, expect, it } from 'vitest';
import { MARK_WIDTH_PX, eraseMarks, markStyle, newMark } from './live-marks';
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
