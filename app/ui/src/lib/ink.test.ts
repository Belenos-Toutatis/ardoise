/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { DEFAULT_SHAPE_TUNING, TOOLS, instantiateInk, type InkStroke, type ShapeStroke } from './ink';
import { DEFAULT_SHAPE_SETTINGS, toShapeTuning } from './shape-settings';

const bytes = readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url));

function wave(tool: InkStroke['tool']): InkStroke {
  return {
    tool,
    width: 90,
    points: Array.from({ length: 30 }, (_, i) => ({
      x: 1000 + 120 * i,
      y: 2000 + 300 * Math.sin(i * 0.4),
      pressure: i / 29,
      t: 5 * i,
    })),
  };
}

describe('stroke geometry in WebAssembly', () => {
  it('outlines every tool with closed contours around the samples', async () => {
    const ink = await instantiateInk(bytes);
    for (const tool of TOOLS) {
      const contours = ink.contours(wave(tool));
      expect(contours.length, tool).toBeGreaterThan(0);
      const xs = contours.flatMap((c) => Array.from(c).filter((_, i) => i % 2 === 0));
      // The outline spans the stroke, from x = 1000 to 4480, plus half the width at each end.
      expect(Math.min(...xs), tool).toBeLessThan(1000);
      expect(Math.max(...xs), tool).toBeGreaterThan(4480);
    }
  });

  it('draws a single ballpoint dot with the diameter of the width response', async () => {
    const ink = await instantiateInk(bytes);
    const dot: InkStroke = { tool: 'ballpoint', width: 50, points: [{ x: 500, y: 500, pressure: 1, t: 0 }] };
    const [disc] = ink.contours(dot);
    const xs = Array.from(disc).filter((_, i) => i % 2 === 0);
    expect(Math.max(...xs) - Math.min(...xs)).toBeCloseTo(ink.widthAt('ballpoint', 50, 1), 0);
    expect(ink.widthAt('ballpoint', 50, 1)).toBe(50);
    expect(ink.pressureForWidth('ballpoint', 50, ink.widthAt('ballpoint', 50, 0.3))).toBeCloseTo(0.3, 5);
  });

  it('gives SVG path data and copes with empty strokes', async () => {
    const ink = await instantiateInk(bytes);
    const d = ink.path(wave('fountainPen'));
    expect(d.startsWith('M')).toBe(true);
    expect(d.endsWith('Z')).toBe(true);
    expect(ink.contours({ tool: 'pencil', width: 35, points: [] })).toEqual([]);
    expect(ink.path({ tool: 'pencil', width: 35, points: [] })).toBe('');
  });

  it("straightens shapes exactly as the Rust code does, keeping each stroke's style", async () => {
    const ink = await instantiateInk(bytes);
    const ring: ShapeStroke = {
      tool: 'highlighter',
      color: { r: 250, g: 200, b: 10, a: 128 },
      width: 400,
      layer: 2,
      points: Array.from({ length: 100 }, (_, i) => {
        const a = (i / 96) * 2 * Math.PI;
        return { x: 9000 + 3000 * Math.cos(a), y: 9000 + 2000 * Math.sin(a) + 30 * Math.sin(5 * a), pressure: 0.3 + i / 300, t: 1000 + 5 * i };
      }),
    };
    const small: ShapeStroke = { ...ring, points: ring.points.slice(0, 3) };
    const [group, ...rest] = ink.straighten([small, ring]);
    expect(rest).toEqual([]);
    expect(group.shape).toBe('ellipse');
    expect(group.sources).toEqual([1]);
    const [made] = group.strokes;
    expect(made).toMatchObject({ tool: 'highlighter', color: ring.color, width: 400, layer: 2 });
    expect(made.points[0].t).toBe(1000);
    expect(made.points[made.points.length - 1].t).toBe(1000 + 5 * 99);
    expect(new Set(made.points.map((p) => p.pressure)).size).toBe(1);
    expect(ink.straighten([])).toEqual([]);
    expect(ink.straighten([small])).toEqual([]);
  });

  /** A slightly wobbly line of `length` device units at `degrees` from the horizontal. */
  function line(length: number, degrees: number): ShapeStroke {
    const angle = (degrees * Math.PI) / 180;
    const [cos, sin] = [Math.cos(angle), Math.sin(angle)];
    return {
      tool: 'ballpoint',
      color: { r: 20, g: 20, b: 20, a: 255 },
      width: 50,
      layer: 1,
      points: Array.from({ length: 30 }, (_, i) => {
        const along = (length * i) / 29;
        const across = 2 * Math.sin(i * 0.9);
        return { x: 2000 + along * cos - across * sin, y: 3000 + along * sin + across * cos, pressure: 0.5, t: 5 * i };
      }),
    };
  }

  it('straightens with the tuning it is given, and with the defaults without one', async () => {
    const ink = await instantiateInk(bytes);
    // 7 mm, turned by 3 degrees: too short for the default minimum of 10 mm.
    const short = line(700, 3);
    expect(ink.straighten([short])).toEqual([]);
    expect(ink.straighten([short], DEFAULT_SHAPE_TUNING)).toEqual([]);
    const small = { ...DEFAULT_SHAPE_TUNING, minSizeMm: 5 };
    const [snapped, ...none] = ink.straighten([short], small);
    expect(none).toEqual([]);
    expect(snapped.shape).toBe('line');
    const ys = snapped.strokes[0].points.map((p) => p.y);
    expect(new Set(ys).size).toBe(1);
    // Without the alignment it keeps its slope (700 x sin 3 degrees = 37 units).
    const [tilted] = ink.straighten([short], { ...small, snapAngleDeg: null });
    const points = tilted.strokes[0].points;
    expect(points[points.length - 1].y - points[0].y).toBeGreaterThan(30);
    // A long line: no tuning is the default tuning.
    const long = line(5000, 20);
    expect(ink.straighten([long])).toEqual(ink.straighten([long], DEFAULT_SHAPE_TUNING));
    expect(ink.straighten([long])).toHaveLength(1);
    // Values out of their limits are brought within them by Rust: a minimum of 0 is 5 mm.
    const wild = { tolerance: 99, minSizeMm: 0, snapAngleDeg: 50, snapRoundPercent: 500 };
    expect(ink.straighten([short], wild)).toHaveLength(1);
    expect(ink.straighten([short], { ...wild, tolerance: Number.NaN, minSizeMm: Number.NaN })).toEqual([]);
  });

  it('has the same default tuning as the settings', () => {
    expect(toShapeTuning({ ...DEFAULT_SHAPE_SETTINGS })).toEqual(DEFAULT_SHAPE_TUNING);
  });
});
