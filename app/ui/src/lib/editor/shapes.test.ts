/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { instantiateInk, type Straightened } from '../ink';
import type { Stroke } from '../note';
import { straightenSelection } from './shapes';
import { docFromNote, select } from './state';
import { hline, line, note } from './test-fixtures';

const bytes = readFileSync(new URL('../../assets/ardoise_ink.wasm', import.meta.url));

/** A wobbly circle drawn by hand. */
function circle(cx: number, cy: number, r: number): Stroke {
  return line(
    Array.from({ length: 120 }, (_, i): [number, number] => {
      const a = (i / 115) * 2 * Math.PI;
      const k = 1 + 0.02 * Math.sin(3 * a);
      return [cx + r * k * Math.cos(a), cy + r * k * Math.sin(a)];
    }),
  );
}

describe('straighten the selection', () => {
  it('replaces the recognised strokes in place and keeps them selected', async () => {
    const ink = await instantiateInk(bytes);
    const small = hline(1000, 1600, 1000, 50);
    const strokes = [small, circle(10000, 10000, 2500), hline(2000, 2400, 3000, 50), line([[2000, 15000], [9000, 15200], [16000, 15050]])];
    const doc = select(docFromNote(note(strokes)), [1, 2, 3]);
    const { doc: next, shapes } = straightenSelection(doc, ink);
    expect(shapes).toBe(2);
    expect(next.note.strokes).toHaveLength(4);
    // Unselected and unrecognised strokes are the very same objects.
    expect(next.note.strokes[0]).toBe(small);
    expect(next.note.strokes[2]).toBe(strokes[2]);
    expect(next.note.strokes[1]).not.toBe(strokes[1]);
    expect(next.note.strokes[3]).not.toBe(strokes[3]);
    expect(next.selection).toEqual([1, 2, 3]);
    const ring = next.note.strokes[1].points;
    expect(ring[0]).toMatchObject({ x: ring[ring.length - 1].x, y: ring[ring.length - 1].y });
    // The original note is untouched (undo restores it).
    expect(doc.note.strokes).toEqual(strokes);
  });

  it('puts strokes merged into one shape where the first of them was', () => {
    const strokes = [hline(0, 100, 0), hline(0, 100, 100), hline(0, 100, 200), hline(0, 100, 300)];
    const made: Stroke = hline(0, 500, 500);
    const fake = {
      straighten: (given: readonly Stroke[]): Straightened[] => {
        // The selection is [1, 3]: the arrow is made of both.
        expect(given).toEqual([strokes[1], strokes[3]]);
        return [{ sources: [0, 1], shape: 'arrow', strokes: [made, made] }];
      },
    };
    const doc = select(docFromNote(note(strokes)), [1, 3]);
    const { doc: next, shapes } = straightenSelection(doc, fake);
    expect(shapes).toBe(1);
    expect(next.note.strokes).toEqual([strokes[0], made, made, strokes[2]]);
    expect(next.selection).toEqual([1, 2]);
  });

  it('changes nothing without a selection or a shape', async () => {
    const ink = await instantiateInk(bytes);
    const doc = docFromNote(note([circle(10000, 10000, 2500)]));
    expect(straightenSelection(doc, ink)).toEqual({ doc, shapes: 0 });
    const wavy = select(docFromNote(note([line([[0, 0], [3000, 3000], [6000, 0], [9000, 3000]])])), [0]);
    expect(straightenSelection(wavy, ink).doc).toBe(wavy);
  });
});
