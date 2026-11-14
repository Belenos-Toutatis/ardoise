/**
 * Equations in the editor (`lib/editor/objects.ts`, `clipboard.ts`, `docs/design/equations.md`): placed from their drawing
 * at the tool's size, redrawn and resized with their top-left corner kept, selectable and found by a tap, read by the
 * search as their source, carried by the page operations through their placement, and checked like a note's on the
 * clipboard.
 */
import { describe, expect, it } from 'vitest';
import { isDrawnObject, isEquationObject, type EquationObject, type PageObject } from '../note';
import { decodeClipboard, encodeClipboard } from './clipboard';
import {
  equationAt,
  equationScale,
  newEquationObject,
  objectBounds,
  objectsOf,
  redrawnEquation,
  resizeEquations,
  selectable,
  typedText,
  withObjects,
} from './objects';
import { rotateQuarter } from './ops';
import { note } from './test-fixtures';

const DRAWING = { path: 'M0 0L1000 0L1000 500L0 500Z', width: 1000, height: 500 };

function equation(fields: Partial<EquationObject> = {}): EquationObject {
  return newEquationObject({
    id: 'e1',
    layer: 0,
    t: 10,
    at: { x: 2000, y: 3000 },
    size: 600,
    latex: '\\ce{H2O}',
    drawing: DRAWING,
    color: { r: 0, g: 0, b: 200, a: 255 },
    ...fields,
  }) as EquationObject;
}

describe('equations in the editor', () => {
  it('are placed at the tool’s size, their left end and middle at the tap', () => {
    const e = equation();
    expect(e.scale).toBe(equationScale(600));
    expect(e.scale).toBeCloseTo(0.6, 9);
    expect(objectBounds(e)).toEqual({ x: 2000, y: 3000 - 150, width: 600, height: 300 });
    expect(isEquationObject(e) && isDrawnObject(e) && selectable(e)).toBe(true);
  });

  it('are refused when their drawing is not in the stored grammar, and then neither drawn nor selectable', () => {
    for (const path of ['m0 0l1 1z', 'M0 0A1 1 0 0 1 2 2', '<script>', 'M0 0L1e9 0Z', 'M0 2000000']) {
      const bad = { ...equation(), path } as PageObject;
      expect(isEquationObject(bad), path).toBe(false);
      expect(selectable(bad), path).toBe(false);
    }
    expect(isEquationObject({ ...equation(), latex: 'x'.repeat(4001) })).toBe(false);
    expect(isEquationObject({ ...equation(), latex: 'é'.repeat(4000) })).toBe(true);
  });

  it('keep their top-left corner when redrawn or resized, even turned', () => {
    const turned = { ...equation(), quarterTurns: 1 };
    const corner = objectBounds(turned);
    const redrawn = redrawnEquation(turned, 'x', { path: 'M0 0L2000 0L2000 800Z', width: 2000, height: 800 });
    expect(redrawn.latex).toBe('x');
    expect(redrawn.id).toBe('e1');
    expect([objectBounds(redrawn).x, objectBounds(redrawn).y]).toEqual([corner.x, corner.y]);
    const n = withObjects(note([]), [turned]);
    const resized = objectsOf(resizeEquations(n, [0], 900))[0];
    expect(resized.scale).toBeCloseTo(0.9, 9);
    expect(objectBounds(resized).x).toBeCloseTo(corner.x, 6);
    expect(objectBounds(resized).y).toBeCloseTo(corner.y, 6);
    // Already at that size: the same note.
    expect(resizeEquations(n, [0], 600)).toBe(n);
  });

  it('are found by a tap, read by the search as their source, and turn with the page', () => {
    const n = withObjects(note([]), [equation(), { ...equation({ id: 'hidden' }), layer: 1 }]);
    const layered = { ...n, layers: [...n.layers, { name: 'Caché', visible: false }] };
    expect(equationAt(layered, { x: 2300, y: 3000 }, 10, 0)).toBe(0);
    expect(equationAt(layered, { x: 9000, y: 9000 }, 10, 0)).toBeNull();
    expect(typedText(layered)).toBe('\\ce{H2O}');
    const turned = objectsOf(rotateQuarter(n, 'cw'))[0] as EquationObject;
    expect(turned.quarterTurns).toBe(1);
    expect(turned.path).toBe(DRAWING.path);
    expect(turned.latex).toBe('\\ce{H2O}');
  });

  it('travel on the clipboard with their source and drawing, checked like a note’s', () => {
    const e = equation();
    const decoded = decodeClipboard(encodeClipboard([], [e]));
    expect(decoded?.objects).toHaveLength(1);
    const back = decoded!.objects[0] as EquationObject;
    expect([back.kind, back.latex, back.path, back.scale, back.width, back.height]).toEqual(['equation', '\\ce{H2O}', DRAWING.path, e.scale, 1000, 500]);
    // A hostile drawing makes the whole clipboard foreign: nothing is pasted.
    const hostile = encodeClipboard([], [e]).replace(DRAWING.path, 'M0 0<script>');
    expect(decodeClipboard(hostile)).toBeNull();
    // An equation this version cannot read is left behind when copying.
    expect(encodeClipboard([], [{ ...e, path: 'm0 0z' } as PageObject])).not.toContain('equation');
  });
});
