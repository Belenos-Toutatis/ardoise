/** Small synthetic notes for the editor's tests. */
import type { Tool } from '../ink';
import type { Note, Rgba, Stroke } from '../note';

export const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
export const RED: Rgba = { r: 0xd8, g: 0, b: 0, a: 255 };

/** A straight stroke through `points` ([x, y] pairs), pressure 0.5, 10 ms apart. */
export function line(points: [number, number][], layer = 0, tool: Tool = 'ballpoint', width = 60): Stroke {
  return {
    tool,
    color: BLACK,
    width,
    layer,
    points: points.map(([x, y], i) => ({ x, y, pressure: 0.5, t: i * 10 })),
  };
}

/** A horizontal stroke from (x0, y) to (x1, y) with a sample every `step` units. */
export function hline(x0: number, x1: number, y: number, step = 100, layer = 0): Stroke {
  const points: [number, number][] = [];
  for (let x = x0; x <= x1; x += step) points.push([x, y]);
  return line(points, layer);
}

/** 300 strokes of 67 samples each: 20 100 samples over an A4 landscape page. */
export function bigNote(): Note {
  const strokes: Stroke[] = [];
  for (let s = 0; s < 300; s++) {
    const row = Math.floor(s / 10);
    const col = s % 10;
    strokes.push({
      tool: 'ballpoint',
      color: BLACK,
      width: 60,
      layer: 0,
      points: Array.from({ length: 67 }, (_, i) => ({
        x: 1000 + col * 2800 + i * 35,
        y: 1000 + row * 650 + 200 * Math.sin(i / 5 + s),
        pressure: 0.5,
        t: s * 1000 + i * 5,
      })),
    });
  }
  return note(strokes);
}

export function note(strokes: Stroke[], layers = 1, page = { width: 29700, height: 21600 }): Note {
  return {
    id: 'test',
    title: 'Test',
    created: 0,
    modified: 0,
    page,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: Array.from({ length: layers }, (_, i) => ({ name: `L${i + 1}`, visible: true })),
    strokes,
    timing: 'recorded',
    origin: { kind: 'ardoise' },
  };
}
