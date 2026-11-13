/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from '../ink';
import { templateSnapGrid } from '../mock-notes';
import type { Template } from '../note';
import { BLACK } from './test-fixtures';
import {
  MAX_SHAPE_MS,
  MIN_SHAPE_MS,
  SAMPLE_SPACING,
  SHAPE_PRESSURE,
  buildShapeStroke,
  effectiveToggles,
  isShapeKind,
  resolveShape,
  shapeDuration,
  shapePath,
  shapePoints,
  snapPoint,
  snapToAxis,
  type ShapeKind,
  type ShapeRequest,
  type ShapeToggles,
  type SnapGrid,
} from './shape-tools';

const PLAIN: ShapeToggles = { regular: false, centre: false, snap: false };
const A4 = { width: 21000, height: 29700 };

/** A request with nothing asked, a minimal size of 40 units (0.4 mm). */
function request(kind: ShapeKind, start: [number, number], end: [number, number], toggles: Partial<ShapeToggles> = {}, grid: SnapGrid | null = null): ShapeRequest {
  return { kind, start: { x: start[0], y: start[1] }, end: { x: end[0], y: end[1] }, toggles: { ...PLAIN, ...toggles }, grid, minSize: 40 };
}

const rect = (spec: ReturnType<typeof resolveShape>) => {
  if (spec?.kind !== 'rectangle') throw new Error(`not a rectangle: ${JSON.stringify(spec)}`);
  return spec.corners.map((c) => [c.x, c.y]);
};
const ellipse = (spec: ReturnType<typeof resolveShape>) => {
  if (spec?.kind !== 'ellipse') throw new Error(`not an ellipse: ${JSON.stringify(spec)}`);
  return spec;
};

describe('the options of one stroke', () => {
  it('Shift inverts « Forme régulière » and Alt « Depuis le centre », and nothing inverts the snapping', () => {
    const off = PLAIN;
    expect(effectiveToggles(off, {})).toEqual(off);
    expect(effectiveToggles(off, { shift: true })).toEqual({ ...off, regular: true });
    expect(effectiveToggles(off, { alt: true })).toEqual({ ...off, centre: true });
    const on: ShapeToggles = { regular: true, centre: true, snap: true };
    expect(effectiveToggles(on, { shift: true, alt: true })).toEqual({ regular: false, centre: false, snap: true });
    expect(effectiveToggles(on, { shift: true })).toEqual({ regular: false, centre: true, snap: true });
  });

  it('knows its three tools', () => {
    expect(['line', 'rectangle', 'ellipse'].every(isShapeKind)).toBe(true);
    expect(['pen', 'select', ''].some(isShapeKind)).toBe(false);
  });
});

describe('a line', () => {
  it('goes exactly from the first point to the last: nothing is straightened unless asked', () => {
    const spec = resolveShape(request('line', [1000, 1000], [5003, 1217]));
    expect(spec).toEqual({ kind: 'line', a: { x: 1000, y: 1000 }, b: { x: 5003, y: 1217 } });
  });

  it('a regular line snaps to the horizontal, the vertical or a 45° diagonal, whichever is closest', () => {
    const end = (x: number, y: number) => (resolveShape(request('line', [1000, 1000], [x, y], { regular: true })) as { b: { x: number; y: number } }).b;
    // Under 22.5° from the horizontal: horizontal, the length along it kept.
    expect(end(5000, 1500)).toEqual({ x: 5000, y: 1000 });
    expect(end(-3000, 1100)).toEqual({ x: -3000, y: 1000 });
    // Under 22.5° from the vertical.
    expect(end(1400, 6000)).toEqual({ x: 1000, y: 6000 });
    expect(end(900, -4000)).toEqual({ x: 1000, y: -4000 });
    // Else a diagonal, the larger side taken, in the quadrant the pointer is in.
    expect(end(5000, 4000)).toEqual({ x: 5000, y: 5000 });
    expect(end(-2000, 3000)).toEqual({ x: -2000, y: 4000 });
    expect(end(4000, -4500)).toEqual({ x: 6500, y: -4500 });
    expect(end(-3000, -3000)).toEqual({ x: -3000, y: -3000 });
  });

  it('from the centre it grows both ways, with or without being regular', () => {
    expect(resolveShape(request('line', [3000, 3000], [4000, 3500], { centre: true }))).toEqual({
      kind: 'line',
      a: { x: 2000, y: 2500 },
      b: { x: 4000, y: 3500 },
    });
    expect(resolveShape(request('line', [3000, 3000], [5000, 3300], { centre: true, regular: true }))).toEqual({
      kind: 'line',
      a: { x: 1000, y: 3000 },
      b: { x: 5000, y: 3000 },
    });
  });

  it('a click, or a drag that stays put, draws nothing', () => {
    expect(resolveShape(request('line', [1000, 1000], [1000, 1000]))).toBeNull();
    expect(resolveShape(request('line', [1000, 1000], [1020, 1010]))).toBeNull();
    expect(resolveShape(request('line', [1000, 1000], [1040, 1000]))).not.toBeNull();
    // From the centre the line is twice the drag.
    expect(resolveShape(request('line', [1000, 1000], [1020, 1000], { centre: true }))).not.toBeNull();
  });
});

describe('a rectangle', () => {
  it('is the box of the two corners, whichever way it is dragged, the stroke starting at the first corner', () => {
    expect(rect(resolveShape(request('rectangle', [1000, 1000], [4000, 3000])))).toEqual([[1000, 1000], [4000, 1000], [4000, 3000], [1000, 3000]]);
    expect(rect(resolveShape(request('rectangle', [4000, 3000], [1000, 1000])))).toEqual([[4000, 3000], [1000, 3000], [1000, 1000], [4000, 1000]]);
    expect(rect(resolveShape(request('rectangle', [4000, 1000], [1000, 3000])))).toEqual([[4000, 1000], [1000, 1000], [1000, 3000], [4000, 3000]]);
  });

  it('is a square on the longer side when regular', () => {
    expect(rect(resolveShape(request('rectangle', [1000, 1000], [4000, 2000], { regular: true })))).toEqual([[1000, 1000], [4000, 1000], [4000, 4000], [1000, 4000]]);
    // Dragged up and to the left, the square grows that way.
    expect(rect(resolveShape(request('rectangle', [4000, 4000], [3500, 1000], { regular: true })))).toEqual([[4000, 4000], [1000, 4000], [1000, 1000], [4000, 1000]]);
  });

  it('is centred on the first point when drawn from the centre, the stroke starting where the pointer is', () => {
    expect(rect(resolveShape(request('rectangle', [3000, 3000], [4000, 3500], { centre: true })))).toEqual([[4000, 3500], [2000, 3500], [2000, 2500], [4000, 2500]]);
    // Regular too: a square around the centre.
    expect(rect(resolveShape(request('rectangle', [3000, 3000], [3400, 4000], { centre: true, regular: true })))).toEqual([[4000, 4000], [2000, 4000], [2000, 2000], [4000, 2000]]);
  });

  it('draws nothing when a side is smaller than the minimum', () => {
    expect(resolveShape(request('rectangle', [1000, 1000], [3000, 1000]))).toBeNull();
    expect(resolveShape(request('rectangle', [1000, 1000], [3000, 1030]))).toBeNull();
    expect(resolveShape(request('rectangle', [1000, 1000], [1000, 1000]))).toBeNull();
    expect(resolveShape(request('rectangle', [1000, 1000], [3000, 1040]))).not.toBeNull();
  });
});

describe('an ellipse', () => {
  it('is inscribed in the box of the two points', () => {
    const e = ellipse(resolveShape(request('ellipse', [1000, 1000], [5000, 3000])));
    expect(e).toMatchObject({ centre: { x: 3000, y: 2000 }, rx: 2000, ry: 1000 });
  });

  it('is a circle on the longer side when regular', () => {
    const e = ellipse(resolveShape(request('ellipse', [1000, 1000], [5000, 3000], { regular: true })));
    expect(e).toMatchObject({ centre: { x: 3000, y: 3000 }, rx: 2000, ry: 2000 });
  });

  it('has the first point for centre when drawn from the centre', () => {
    expect(ellipse(resolveShape(request('ellipse', [3000, 3000], [4000, 3500], { centre: true })))).toMatchObject({ centre: { x: 3000, y: 3000 }, rx: 1000, ry: 500 });
    expect(ellipse(resolveShape(request('ellipse', [3000, 3000], [4000, 3500], { centre: true, regular: true })))).toMatchObject({ centre: { x: 3000, y: 3000 }, rx: 1000, ry: 1000 });
  });

  it('draws nothing from a click or a flat drag', () => {
    expect(resolveShape(request('ellipse', [1000, 1000], [1000, 1000]))).toBeNull();
    expect(resolveShape(request('ellipse', [1000, 1000], [4000, 1000]))).toBeNull();
  });
});

describe('snapping to the page', () => {
  const grid5 = templateSnapGrid(A4, 'grid5mm')!;

  it('moves a point to the nearest line of an axis, the ends included, and leaves a free axis alone', () => {
    const axis = { first: 100, pitch: 500, count: 4 };
    expect(snapToAxis(100, axis)).toBe(100);
    expect(snapToAxis(349, axis)).toBe(100);
    expect(snapToAxis(351, axis)).toBe(600);
    expect(snapToAxis(-5000, axis)).toBe(100);
    expect(snapToAxis(99_999, axis)).toBe(1600);
    expect(snapToAxis(777, null)).toBe(777);
    expect(snapToAxis(777, { first: 0, pitch: 500, count: 0 })).toBe(777);
    expect(snapPoint({ x: 1234, y: 560 }, { x: null, y: axis })).toEqual({ x: 1234, y: 600 });
    expect(snapPoint({ x: 1234, y: 560 }, null)).toEqual({ x: 1234, y: 560 });
  });

  it('squared paper: the corners go to the intersections of the 5 mm grid', () => {
    // x: every 500 from 0; y: every 500 from 100 (297 mm holds 59 squares and 2 mm over).
    const spec = resolveShape(request('rectangle', [1234, 1560], [4180, 3290], { snap: true }, grid5));
    expect(rect(spec)).toEqual([[1000, 1600], [4000, 1600], [4000, 3100], [1000, 3100]]);
  });

  it('a line ends on the grid too, and is exactly what the toggle says without it', () => {
    expect(resolveShape(request('line', [1234, 1560], [4180, 3290], { snap: true }, grid5))).toMatchObject({ a: { x: 1000, y: 1600 }, b: { x: 4000, y: 3100 } });
    expect(resolveShape(request('line', [1234, 1560], [4180, 3290], {}, grid5))).toMatchObject({ a: { x: 1234, y: 1560 }, b: { x: 4180, y: 3290 } });
    // No grid on the page: the toggle does nothing.
    expect(resolveShape(request('line', [1234, 1560], [4180, 3290], { snap: true }, null))).toMatchObject({ a: { x: 1234, y: 1560 } });
  });

  it('keeps a square a square: snapped, then regular, on a grid with the same pitch both ways', () => {
    const spec = resolveShape(request('rectangle', [1000, 1600], [3610, 2300], { snap: true, regular: true }, grid5));
    // The end snaps to (3500, 2100), then the larger side, 2500, makes both sides: all on the grid.
    expect(rect(spec)).toEqual([[1000, 1600], [3500, 1600], [3500, 4100], [1000, 4100]]);
  });

  it('from the centre, the centre and the end are on the grid, and so is the opposite corner', () => {
    const spec = resolveShape(request('ellipse', [3010, 3090], [4510, 3590], { snap: true, centre: true }, grid5));
    expect(ellipse(spec)).toMatchObject({ centre: { x: 3000, y: 3100 }, rx: 1500, ry: 500 });
  });

  it('ruled paper: only the horizontal lines, the vertical is free', () => {
    const lined = templateSnapGrid(A4, 'lined')!;
    expect(lined.x).toBeNull();
    // First rule at 16 mm, every 8 mm.
    expect(resolveShape(request('line', [1234, 1700], [5678, 2500], { snap: true }, lined))).toMatchObject({ a: { x: 1234, y: 1600 }, b: { x: 5678, y: 2400 } });
  });

  it('Seyès: the horizontal rules and the vertical lines of the large squares', () => {
    const seyes = templateSnapGrid(A4, 'seyes')!;
    // Vertical lines: the margin at 32 mm, then every 8 mm; rules: from 16 mm every 8 mm.
    expect(resolveShape(request('line', [3300, 1700], [5000, 2500], { snap: true }, seyes))).toMatchObject({ a: { x: 3200, y: 1600 }, b: { x: 4800, y: 2400 } });
  });

  it('millimetre and dotted paper have their own lines', () => {
    const mm = templateSnapGrid(A4, 'millimetric')!;
    expect(resolveShape(request('line', [1234, 1260], [2000, 2000], { snap: true }, mm))).toMatchObject({ a: { x: 1200, y: 1250 }, b: { x: 2000, y: 2050 } });
    expect(templateSnapGrid(A4, 'dotted')).toEqual(grid5);
  });

  it('draws nothing when both points snap to the same dot', () => {
    expect(resolveShape(request('rectangle', [1010, 1610], [1100, 1700], { snap: true }, grid5))).toBeNull();
    expect(resolveShape(request('line', [1010, 1610], [1100, 1700], { snap: true }, grid5))).toBeNull();
  });

  it('a regular shape wins over the snapping where they disagree (ruled paper: the height may leave the rules)', () => {
    const lined = templateSnapGrid(A4, 'lined')!;
    const spec = resolveShape(request('rectangle', [1000, 1600], [3000, 2400], { snap: true, regular: true }, lined));
    // The end snaps to y = 2400; the square then takes the larger side (2000): 3600 is no rule, and stays a square.
    expect(rect(spec)).toEqual([[1000, 1600], [3000, 1600], [3000, 3600], [1000, 3600]]);
  });
});

describe('the points of the stroke', () => {
  const spacing = (points: { x: number; y: number }[]) => points.slice(1).map((p, i) => Math.hypot(p.x - points[i].x, p.y - points[i].y));

  it('a line is densified, its ends being samples', () => {
    const path = shapePath(resolveShape(request('line', [1000, 1000], [3000, 1000]))!);
    expect(path[0]).toEqual({ x: 1000, y: 1000 });
    expect(path.at(-1)).toEqual({ x: 3000, y: 1000 });
    expect(path).toHaveLength(41);
    expect(Math.max(...spacing(path))).toBeLessThanOrEqual(SAMPLE_SPACING + 1e-9);
  });

  it('a rectangle is one closed path through its four corners, which are samples', () => {
    const path = shapePath(resolveShape(request('rectangle', [1000, 1000], [3000, 2000]))!);
    expect(path[0]).toEqual({ x: 1000, y: 1000 });
    expect(path.at(-1)).toEqual({ x: 1000, y: 1000 });
    for (const [x, y] of [[3000, 1000], [3000, 2000], [1000, 2000]]) expect(path.some((p) => p.x === x && p.y === y)).toBe(true);
    expect(Math.max(...spacing(path))).toBeLessThanOrEqual(SAMPLE_SPACING + 1e-9);
  });

  it('an ellipse is one closed path of equal angles on the ellipse, at least 72 points even when small', () => {
    const small = shapePath(resolveShape(request('ellipse', [1000, 1000], [1400, 1300]))!);
    expect(small.length).toBeGreaterThanOrEqual(73);
    const big = shapePath(resolveShape(request('ellipse', [1000, 1000], [9000, 6000]))!);
    expect(big.at(-1)).toEqual(big[0]);
    for (const p of big) expect(((p.x - 5000) / 4000) ** 2 + ((p.y - 3500) / 2500) ** 2).toBeCloseTo(1, 9);
    expect(Math.max(...spacing(big))).toBeLessThanOrEqual(SAMPLE_SPACING + 1e-6);
  });

  it('the number of points is bounded, however big the shape', () => {
    const huge = shapePath({ kind: 'ellipse', centre: { x: 0, y: 0 }, rx: 5e6, ry: 5e6, start: 0 });
    expect(huge.length).toBeLessThanOrEqual(4001);
    const line = shapePath({ kind: 'line', a: { x: 0, y: 0 }, b: { x: 5e6, y: 0 } });
    expect(line.length).toBeLessThanOrEqual(4001);
  });

  it('the times grow with the distance along the path, from the base to the base plus the duration', () => {
    const points = shapePoints(shapePath(resolveShape(request('rectangle', [1000, 1000], [5000, 3000]))!), 12_000, 800);
    expect(points[0].t).toBe(12_000);
    expect(points.at(-1)!.t).toBe(12_800);
    for (let i = 1; i < points.length; i++) expect(points[i].t).toBeGreaterThanOrEqual(points[i - 1].t);
    expect(new Set(points.map((p) => p.t)).size).toBeGreaterThan(50);
    expect(points.every((p) => Number.isInteger(p.t) && p.pressure === SHAPE_PRESSURE)).toBe(true);
  });

  it('a shape lasts what the drag lasted, within limits, except in a recording where the real time is kept', () => {
    expect(shapeDuration(40, false)).toBe(MIN_SHAPE_MS);
    expect(shapeDuration(700, false)).toBe(700);
    expect(shapeDuration(30_000, false)).toBe(MAX_SHAPE_MS);
    expect(shapeDuration(30_000, true)).toBe(30_000);
    expect(shapeDuration(40, true)).toBe(MIN_SHAPE_MS);
    expect(shapeDuration(Number.NaN, false)).toBe(MIN_SHAPE_MS);
  });
});

describe('the stroke', () => {
  it('takes the ink, the colour and the width given, a constant pressure and increasing times', () => {
    const red = { r: 200, g: 0, b: 0, a: 255 };
    const stroke = buildShapeStroke({ ...request('line', [1000, 1000], [4000, 1000]), tool: 'fountainPen', color: red, width: 90, base: 5000, durationMs: 600 })!;
    expect(stroke).toMatchObject({ tool: 'fountainPen', width: 90, color: red });
    expect(stroke.color).not.toBe(red);
    expect(new Set(stroke.points.map((p) => p.pressure))).toEqual(new Set([1]));
    expect(stroke.points[0].t).toBe(5000);
    expect(stroke.points.at(-1)!.t).toBe(5600);
  });

  it('is null for a click', () => {
    expect(buildShapeStroke({ ...request('rectangle', [1000, 1000], [1001, 1001]), tool: 'ballpoint', color: BLACK, width: 60, base: 0, durationMs: 200 })).toBeNull();
  });
});

describe('the outline of a shape, as the screen and the exports fill it', () => {
  let ink: Ink;
  beforeAll(async () => {
    ink = await instantiateInk(readFileSync(new URL('../../assets/ardoise_ink.wasm', import.meta.url)));
  });

  const stroke = (kind: ShapeKind, start: [number, number], end: [number, number], tool: 'ballpoint' | 'brush' | 'highlighter' = 'ballpoint') =>
    buildShapeStroke({ ...request(kind, start, end), tool, color: BLACK, width: 60, base: 0, durationMs: 500 })!;

  /** The farthest the contours reach from `corner` into the direction `(dx, dy)` (a unit vector). */
  const reach = (contours: Float32Array[], corner: [number, number], dx: number, dy: number) => {
    let best = -Infinity;
    for (const c of contours) for (let i = 0; i < c.length; i += 2) best = Math.max(best, (c[i] - corner[0]) * dx + (c[i + 1] - corner[1]) * dy);
    return best;
  };

  it('a rectangle keeps its four corners sharp: each reaches half the width beyond the corner, the start too', () => {
    const s = stroke('rectangle', [1000, 1000], [5000, 3000]);
    const contours = ink.contours(s);
    const r = Math.SQRT1_2;
    // A round join on a sharp centre line: half the width straight out along the diagonal. A rounded centre line
    // (what a smooth curve through the samples would make) stops short of it by far more than a unit.
    expect(reach(contours, [1000, 1000], -r, -r)).toBeGreaterThan(29);
    expect(reach(contours, [5000, 1000], r, -r)).toBeGreaterThan(29);
    expect(reach(contours, [5000, 3000], r, r)).toBeGreaterThan(29);
    expect(reach(contours, [1000, 3000], -r, r)).toBeGreaterThan(29);
    // And not more: nothing spikes out.
    expect(reach(contours, [5000, 3000], r, r)).toBeLessThan(31);
  });

  it('an ellipse is smooth: its outline stays within half the width of the ideal curve', () => {
    const s = stroke('ellipse', [1000, 1000], [9000, 9000]);
    for (const c of ink.contours(s)) {
      for (let i = 0; i < c.length; i += 2) {
        const d = Math.hypot(c[i] - 5000, c[i + 1] - 5000);
        expect(Math.abs(d - 4000)).toBeLessThan(32);
      }
    }
  });

  it('a line is straight: every contour point is within half the width of its axis', () => {
    const s = stroke('line', [1000, 1000], [9000, 4000]);
    const [dx, dy] = [8000, 3000];
    const length = Math.hypot(dx, dy);
    for (const c of ink.contours(s)) {
      for (let i = 0; i < c.length; i += 2) {
        const off = Math.abs((c[i] - 1000) * dy - (c[i + 1] - 1000) * dx) / length;
        expect(off).toBeLessThan(30.5);
      }
    }
  });

  it('at full pressure every ink draws its nominal width, so a shape is as wide as the width chosen', () => {
    for (const tool of ['ballpoint', 'highlighter'] as const) {
      const s = stroke('line', [1000, 1000], [9000, 1000], tool);
      expect(ink.widthAt(tool, 60, s.points[10].pressure)).toBeCloseTo(60, 6);
    }
  });
});

describe('the mock stand-in for the snap grid', () => {
  const span = (axis: { first: number; pitch: number; count: number } | null) => axis && [axis.first, axis.pitch, axis.first + (axis.count - 1) * axis.pitch];

  it('gives the numbers the Rust tests give for A4 portrait, in device units', () => {
    const expected: Record<Template, [number[] | null, number[] | null] | null> = {
      blank: null,
      music: null,
      lined: [null, [1600, 800, 28_800]],
      seyes: [[3200, 800, 20_800], [1600, 800, 29_600]],
      grid5mm: [[0, 500, 21_000], [100, 500, 29_600]],
      dotted: [[0, 500, 21_000], [100, 500, 29_600]],
      millimetric: [[0, 100, 21_000], [50, 100, 29_650]],
    };
    for (const [template, want] of Object.entries(expected) as [Template, (typeof expected)[Template]][]) {
      const grid = templateSnapGrid(A4, template);
      if (want === null) {
        expect(grid, template).toBeNull();
        continue;
      }
      const near = (got: number[] | null, wanted: number[] | null) => {
        if (wanted === null) return expect(got, template).toBeNull();
        expect(got, template).not.toBeNull();
        got!.forEach((v, i) => expect(v, `${template} ${i}`).toBeCloseTo(wanted[i], 3));
      };
      near(span(grid!.x), want[0]);
      near(span(grid!.y), want[1]);
    }
  });
});
