/// <reference types="node" />
/**
 * Page objects in the editor (`lib/editor/objects.ts`, `docs/design/page-objects.md`): their placement, the order the
 * page draws them in, the lasso and the click, the page operations that carry them (`ops.ts`), and the text frames'
 * changes of style, measured by the real WebAssembly module.
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from '../ink';
import type { Note, PageObject, TextObject } from '../note';
import { drawingOrder } from './hit';
import {
  affineTurns,
  changeTexts,
  contentPoint,
  drawItems,
  hitObject,
  insertObjectCopies,
  measureText,
  measureTexts,
  newTextObject,
  objectBounds,
  objectCentre,
  objectsInLasso,
  objectsOf,
  pagePoint,
  transformObject,
  typedText,
  withObjects,
} from './objects';
import { autoCropRect, cropTo, mergeDown, moveLayer, resizePage, rotateQuarter } from './ops';
import { BLACK, RED, hline, note } from './test-fixtures';

let ink: Ink;

beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../../assets/ardoise_ink.wasm', import.meta.url)));
});

function frame(fields: Partial<TextObject> = {}): TextObject {
  return measureText(
    {
      kind: 'text',
      id: 'f',
      layer: 0,
      t: 0,
      x: 1000,
      y: 2000,
      scale: 1,
      quarterTurns: 0,
      width: 0,
      height: 0,
      text: 'Bonjour',
      font: 'latinModern',
      bold: false,
      size: 600,
      color: BLACK,
      ...fields,
    },
    ink.text,
  );
}

function withFrames(base: Note, frames: PageObject[]): Note {
  return withObjects(base, frames);
}

describe('placement', () => {
  it('maps the content onto the page through scale and quarter turns, and back', () => {
    const o = { ...frame(), width: 100, height: 50 };
    expect(pagePoint(o, 10, 5)).toEqual({ x: 1010, y: 2005 });
    const turned = { ...o, scale: 2, quarterTurns: 1 };
    // Clockwise on screen: right becomes down, down becomes left.
    expect(pagePoint(turned, 10, 0)).toEqual({ x: 1000, y: 2020 });
    expect(pagePoint(turned, 0, 5)).toEqual({ x: 990, y: 2000 });
    expect(objectBounds(turned)).toEqual({ x: 900, y: 2000, width: 100, height: 200 });
    for (const turns of [0, 1, 2, 3]) {
      const t = { ...o, scale: 1.5, quarterTurns: turns };
      const back = contentPoint(t, pagePoint(t, 33, 17));
      expect(back.x).toBeCloseTo(33, 6);
      expect(back.y).toBeCloseTo(17, 6);
    }
    expect(objectCentre(o)).toEqual({ x: 1050, y: 2025 });
  });

  it('turns with the page exactly as the strokes do', () => {
    const page = note([hline(1000, 3000, 2000)]);
    const o = frame({ x: 1200, y: 1800 });
    const corner = pagePoint(o, o.width, o.height);
    for (const direction of ['cw', 'ccw'] as const) {
      const turned = rotateQuarter(withFrames(page, [o]), direction);
      const moved = objectsOf(turned)[0];
      expect(moved.quarterTurns).toBe(direction === 'cw' ? 1 : 3);
      // The frame's far corner goes where the same point of the page goes.
      const expected =
        direction === 'cw'
          ? { x: page.page.height - corner.y, y: corner.x }
          : { x: corner.y, y: page.page.width - corner.x };
      const got = pagePoint(moved, moved.width, moved.height);
      expect(got.x).toBeCloseTo(expected.x, 3);
      expect(got.y).toBeCloseTo(expected.y, 3);
    }
    // Four turns come back.
    let note4 = withFrames(page, [o]);
    for (let i = 0; i < 4; i++) note4 = rotateQuarter(note4, 'cw');
    expect(objectsOf(note4)[0]).toMatchObject({ x: 1200, y: 1800, quarterTurns: 0 });
  });

  it('reads the quarter turns of an affine map', () => {
    expect(affineTurns({ a: 1, b: 0, c: 0, d: 1, e: 5, f: 5 })).toBe(0);
    expect(affineTurns({ a: 0, b: 1, c: -1, d: 0, e: 0, f: 0 })).toBe(1);
    expect(affineTurns({ a: -2, b: 0, c: 0, d: -2, e: 0, f: 0 })).toBe(2);
    expect(affineTurns({ a: 0, b: -1, c: 1, d: 0, e: 0, f: 0 })).toBe(3);
    const scaled = transformObject(frame(), { a: 0.5, b: 0, c: 0, d: 0.5, e: 100, f: 0 });
    expect(scaled).toMatchObject({ x: 600, y: 1000, scale: 0.5, quarterTurns: 0 });
  });
});

describe('the page draws objects with their layer, above its strokes', () => {
  it('interleaves strokes and objects layer by layer, leaving hidden layers out', () => {
    const base = note([hline(0, 1000, 100, 100, 1), hline(0, 1000, 200, 100, 0), hline(0, 1000, 300, 100, 2)], 3);
    const page = withFrames(
      { ...base, layers: base.layers.map((l, i) => (i === 2 ? { ...l, visible: false } : l)) },
      [frame({ id: 'a', layer: 1 }), frame({ id: 'b', layer: 0 }), frame({ id: 'c', layer: 2 }), frame({ id: 'd', layer: 7 })],
    );
    const items = drawItems(page, drawingOrder(page)).map((i) => `${i.kind[0]}${i.index}`);
    expect(items).toEqual(['s1', 'o1', 's0', 'o0', 'o3']);
    // Without objects, the strokes alone.
    expect(drawItems(base, drawingOrder(base)).map((i) => i.index)).toEqual(drawingOrder(base));
  });
});

describe('selection', () => {
  const page = () => withFrames(note([], 2), [
    frame({ id: 'a', layer: 0, x: 1000, y: 1000 }),
    frame({ id: 'b', layer: 1, x: 1000, y: 1000 }),
    frame({ id: 'c', layer: 0, x: 20_000, y: 1000 }),
  ]);
  const square = [
    { x: 0, y: 0 },
    { x: 8000, y: 0 },
    { x: 8000, y: 8000 },
    { x: 0, y: 8000 },
  ];

  it('takes the objects whose centre is in the lasso, on the chosen layer', () => {
    const p = page();
    expect(objectsInLasso(p, square)).toEqual([0, 1]);
    expect(objectsInLasso(p, square, 0)).toEqual([0]);
    expect(objectsInLasso(p, square, 1)).toEqual([1]);
    const hidden = { ...p, layers: [p.layers[0], { ...p.layers[1], visible: false }] };
    expect(objectsInLasso(hidden, square)).toEqual([0]);
  });

  it('finds the topmost object under a click', () => {
    const p = page();
    const inside = objectCentre(objectsOf(p)[0]);
    expect(hitObject(p, inside, 10)).toBe(1);
    expect(hitObject(p, inside, 10, 0)).toBe(0);
    expect(hitObject(p, { x: 15_000, y: 15_000 }, 10)).toBeNull();
  });
});

describe('page operations carry the objects', () => {
  it('crops, keeping hidden ones and dropping visible ones wholly outside', () => {
    const page = withFrames(note([hline(500, 9000, 500)], 2), [
      frame({ id: 'in', x: 2000, y: 2000 }),
      frame({ id: 'out', x: 20_000, y: 15_000 }),
      frame({ id: 'hidden', layer: 1, x: 20_000, y: 15_000 }),
    ]);
    const hiddenLayer = { ...page, layers: [page.layers[0], { ...page.layers[1], visible: false }] };
    const cropped = cropTo(hiddenLayer, { x: 1000, y: 1000, width: 9000, height: 9000 });
    expect(objectsOf(cropped).map((o) => [o.id, o.x, o.y])).toEqual([
      ['in', 1000, 1000],
      ['hidden', 19_000, 14_000],
    ]);
  });

  it('counts them in the automatic crop and fits them to another page size', () => {
    const page = withFrames(note([]), [frame({ x: 10_000, y: 10_000 })]);
    const rect = autoCropRect(page, 0)!;
    expect(rect.x).toBeCloseTo(10_000, 3);
    expect(rect.width).toBeCloseTo(objectsOf(page)[0].width, 3);
    const half = resizePage(page, { width: page.page.width / 2, height: page.page.height / 2 }, true);
    expect(objectsOf(half)[0]).toMatchObject({ x: 5000, y: 5000, scale: 0.5 });
  });

  it('follow their layer when layers move or merge', () => {
    const page = withFrames(note([], 3), [frame({ id: 'a', layer: 0 }), frame({ id: 'b', layer: 1 }), frame({ id: 'c', layer: 2 })]);
    expect(objectsOf(moveLayer(page, 0, 2)).map((o) => o.layer)).toEqual([2, 0, 1]);
    const merged = mergeDown(page, 1);
    // The merged layer's objects go after the lower layer's: they stay above them.
    expect(objectsOf(merged).map((o) => [o.id, o.layer])).toEqual([
      ['a', 0],
      ['c', 1],
      ['b', 0],
    ]);
  });
});

describe('text frames', () => {
  it('are measured by the module, and a change of size keeps the top-left corner', () => {
    const o = newTextObject(ink.text, { id: 'n', layer: 0, t: 5, x: 3000, y: 4000, text: 'Élève\nœuvre', font: 'andika', bold: false, size: 400, color: RED });
    const laid = ink.text.layout('Élève\nœuvre', { font: 'andika', bold: false, size: 400 });
    expect([o.width, o.height]).toEqual([laid.width, laid.height]);
    expect(o.height).toBeCloseTo(2 * 400 * 1.25, 3);
    const scaled = { ...o, scale: 2, quarterTurns: 1 };
    const corner = objectBounds(scaled);
    const page = withFrames(note([]), [scaled]);
    const bigger = objectsOf(changeTexts(page, [0], { size: 900, bold: true }, ink.text))[0] as TextObject;
    expect(bigger).toMatchObject({ size: 900, bold: true, scale: 1, quarterTurns: 1 });
    const box = objectBounds(bigger);
    expect(box.x).toBeCloseTo(corner.x, 3);
    expect(box.y).toBeCloseTo(corner.y, 3);
    expect(bigger.width).toBeGreaterThan(o.width);
    // Nothing to change: the same note.
    expect(changeTexts(page, [0], { font: 'andika' }, ink.text)).toBe(page);
  });

  it('are measured when a note opens, the note kept as it is when they already are', () => {
    const measured = withFrames(note([]), [frame()]);
    expect(measureTexts(measured, ink.text)).toBe(measured);
    const unmeasured = withFrames(note([]), [{ ...frame(), width: 0, height: 0 }]);
    expect(objectsOf(measureTexts(unmeasured, ink.text))[0].width).toBe(objectsOf(measured)[0].width);
  });

  it('give their text, top to bottom, to the search', () => {
    const page = withFrames(note([], 2), [
      frame({ text: 'en bas', y: 9000 }),
      frame({ text: 'en haut', y: 1000 }),
      frame({ text: '   ', y: 500 }),
      frame({ text: 'masqué', layer: 1, y: 100 }),
    ]);
    const hidden = { ...page, layers: [page.layers[0], { ...page.layers[1], visible: false }] };
    expect(typedText(hidden)).toBe('en haut\nen bas');
    expect(typedText(note([]))).toBe('');
  });

  it('are copied with new ids on a layer, shifted', () => {
    const page = withFrames(note([]), [frame({ id: 'a' })]);
    const { note: pasted, indices } = insertObjectCopies(page, [frame({ id: 'a' })], { x: 400, y: 400 }, 0, [77]);
    expect(indices).toEqual([1]);
    const copy = objectsOf(pasted)[1];
    expect(copy.id).not.toBe('a');
    expect(copy).toMatchObject({ x: 1400, y: 2400, t: 77, layer: 0 });
  });
});
