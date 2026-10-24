/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { TOOLS, instantiateInk, type InkStroke } from './ink';

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
});
