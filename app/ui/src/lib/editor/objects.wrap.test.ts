/// <reference types="node" />
/**
 * Text frames that wrap (format 1.7, `docs/design/page-objects.md` §4): the wrap width of a new frame, the side handles
 * that change it (`rewrapped`), what a change of size, a resize of the selection and the page operations do to it, the
 * clipboard and the reading of a note; measured by the real WebAssembly module.
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from '../ink';
import { isTextObject, type PageObject, type TextObject } from '../note';
import { decodeClipboard, encodeClipboard } from './clipboard';
import {
  MIN_WRAP_EMS,
  TEXT_WRAP_MARGIN,
  changeTexts,
  measureText,
  newFrameWrap,
  newTextObject,
  objectBounds,
  pagePoint,
  rewrapped,
  textStyle,
  transformObject,
  wrapHandles,
  withObjects,
} from './objects';
import { resizePage, rotateQuarter } from './ops';
import { BLACK, note } from './test-fixtures';

let ink: Ink;

beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../../assets/ardoise_ink.wasm', import.meta.url)));
});

const SENTENCE = 'La lumière se propage en ligne droite dans un milieu transparent et homogène.';

function frame(fields: Partial<TextObject> = {}): TextObject {
  return newTextObject(ink.text, {
    id: 'f',
    layer: 0,
    t: 0,
    x: 1000,
    y: 2000,
    text: SENTENCE,
    font: 'latinModern',
    bold: false,
    size: 600,
    color: BLACK,
    wrapWidth: 8000,
    ...fields,
  });
}

describe('a frame that wraps', () => {
  it('wraps at the page edge less a margin from where it is put, and never too narrow', () => {
    expect(newFrameWrap(29700, 5000, 600)).toBe(29700 - TEXT_WRAP_MARGIN - 5000);
    // Put near the right edge: still a few words wide.
    expect(newFrameWrap(29700, 29000, 600)).toBe(MIN_WRAP_EMS * 3 * 600);
    const wrapping = frame();
    expect(textStyle(wrapping).wrap).toBe(8000);
    const lines = ink.text.layout(wrapping.text, textStyle(wrapping)).lines;
    expect(lines.length).toBeGreaterThan(1);
    // Its box: the lines' ink, no wider than the wrap width; as high as its lines.
    expect(wrapping.width).toBeLessThanOrEqual(8000);
    expect(wrapping.width).toBeGreaterThan(6000);
    expect(wrapping.height).toBe(lines.length * 600 * 1.25);
    // A frame made before 1.7 (no wrap width) keeps its one line.
    const old = frame({ wrapWidth: undefined });
    expect('wrapWidth' in old).toBe(false);
    expect(textStyle(old).wrap).toBeNull();
    expect(ink.text.layout(old.text, textStyle(old)).lines).toHaveLength(1);
    expect(old.width).toBeGreaterThan(wrapping.width);
  });

  it('gets a new width from its side handles, its height following', () => {
    const wrapping = frame();
    const { left, right } = wrapHandles(wrapping);
    expect(right.x).toBeCloseTo(wrapping.x + wrapping.width, 3);
    expect(left.x).toBeCloseTo(wrapping.x, 3);
    expect(left.y).toBeCloseTo(wrapping.y + wrapping.height / 2, 3);
    // The right handle dragged to the left: narrower, taller.
    const narrower = rewrapped(wrapping, 'right', { x: wrapping.x + 4000, y: right.y }, ink.text);
    expect(narrower.wrapWidth).toBe(4000);
    expect(narrower.height).toBeGreaterThan(wrapping.height);
    expect([narrower.x, narrower.y]).toEqual([wrapping.x, wrapping.y]);
    // A drag that starts on the handle changes nothing that shows.
    const same = rewrapped(wrapping, 'right', right, ink.text);
    expect(ink.text.layout(same.text, textStyle(same)).lines).toEqual(ink.text.layout(wrapping.text, textStyle(wrapping)).lines);
    // The left handle moves the left edge; the right boundary stays.
    const moved = rewrapped(wrapping, 'left', { x: wrapping.x + 2000, y: left.y }, ink.text);
    expect(moved.x).toBeCloseTo(wrapping.x + 2000, 3);
    expect(moved.y).toBe(wrapping.y);
    expect(moved.wrapWidth).toBeCloseTo(6000, 3);
    // Never narrower than two ems.
    expect(rewrapped(wrapping, 'right', { x: wrapping.x - 5000, y: right.y }, ink.text).wrapWidth).toBe(MIN_WRAP_EMS * 600);
    expect(rewrapped(wrapping, 'left', { x: wrapping.x + 50_000, y: left.y }, ink.text).wrapWidth).toBeCloseTo(MIN_WRAP_EMS * 600, 3);
    // A frame made before 1.7 gets a wrap width from its handle: it wraps from then on.
    const old = frame({ wrapWidth: undefined });
    const set = rewrapped(old, 'right', { x: old.x + 5000, y: 0 }, ink.text);
    expect(set.wrapWidth).toBe(5000);
    expect(set.height).toBeGreaterThan(old.height);
  });

  it('is measured along its own lines when turned or scaled', () => {
    const turned = { ...frame(), quarterTurns: 1, scale: 2 };
    const handles = wrapHandles(turned);
    // A clockwise quarter turn: the lines go down the page.
    expect(handles.right.y).toBeCloseTo(turned.y + turned.width * 2, 3);
    const p = pagePoint(turned, 3000, turned.height / 2);
    const next = rewrapped(turned, 'right', p, ink.text);
    expect(next.wrapWidth).toBeCloseTo(3000, 3);
    expect(next.quarterTurns).toBe(1);
    expect(next.scale).toBe(2);
  });

  it('keeps its width on the page when its size changes, and scales whole with the selection and the page', () => {
    const scaled = measureText({ ...frame(), scale: 1.5 }, ink.text);
    const doc = withObjects(note([]), [scaled]);
    const resized = changeTexts(doc, [0], { size: 900 }, ink.text).objects![0] as TextObject;
    expect(resized.scale).toBe(1);
    expect(resized.size).toBe(900);
    expect(resized.wrapWidth).toBeCloseTo(8000 * 1.5, 3);
    // The colour, the family, bold leave the wrap width alone.
    const bold = changeTexts(doc, [0], { bold: true }, ink.text).objects![0] as TextObject;
    expect(bold.wrapWidth).toBe(8000);
    // A resize of the selection scales the frame: the same lines, larger.
    const twice = transformObject(scaled, { a: 2, b: 0, c: 0, d: 2, e: 0, f: 0 });
    expect(twice.wrapWidth).toBe(8000);
    expect(twice.scale).toBe(3);
    // A quarter turn of the page and « Adapter l'écriture à la feuille » carry it by its placement.
    const turned = rotateQuarter(doc, 'cw').objects![0] as TextObject;
    expect(turned.wrapWidth).toBe(8000);
    expect(turned.quarterTurns).toBe(1);
    const fitted = resizePage(doc, { width: 14850, height: 10800 }, true).objects![0] as TextObject;
    expect(fitted.wrapWidth).toBe(8000);
    expect(objectBounds(fitted).width).toBeLessThan(objectBounds(scaled).width);
  });

  it('travels on the clipboard with its wrap width, and a damaged one is refused', () => {
    const wrapping = frame();
    const copied = decodeClipboard(encodeClipboard([], [wrapping]))!;
    expect(copied.objects).toHaveLength(1);
    expect((copied.objects[0] as TextObject).wrapWidth).toBe(8000);
    const old = decodeClipboard(encodeClipboard([], [frame({ wrapWidth: undefined })]))!;
    expect('wrapWidth' in old.objects[0]).toBe(false);
    const json = JSON.parse(encodeClipboard([], [wrapping]));
    json.objects[0].wrapWidth = -3;
    // A damaged object makes the whole clipboard not Ardoise's (nothing is pasted).
    expect(decodeClipboard(JSON.stringify(json))).toBeNull();
  });

  it('is a text frame of this version only with a positive wrap width (as Rust reads it)', () => {
    const base = frame() as PageObject;
    expect(isTextObject(base)).toBe(true);
    for (const bad of [0, -1, Number.NaN, 'page', null]) expect(isTextObject({ ...base, wrapWidth: bad } as PageObject), String(bad)).toBe(false);
    expect(isTextObject({ ...base, wrapWidth: undefined })).toBe(true);
  });
});
