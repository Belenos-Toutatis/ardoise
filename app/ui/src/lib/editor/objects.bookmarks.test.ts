/// <reference types="node" />
/**
 * Bookmarks (SPEC §4.4 « Signets », format 1.6, `docs/design/page-objects.md` §7) as page objects: their label, their
 * box (the same numbers as Rust's `BookmarkBody::content_box`), the order of their listing, that no layer hides them,
 * that the page operations carry them, and what the clipboard and "back to the original" do with them.
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from '../ink';
import type { BookmarkObject, Note, PageObject } from '../note';
import { decodeClipboard, encodeClipboard } from './clipboard';
import {
  BOOKMARK,
  bookmarkAt,
  bookmarkLook,
  bookmarkObjects,
  bookmarkRefs,
  cleanBookmarkLabel,
  drawItems,
  hitObject,
  measureBookmark,
  measureTexts,
  newBookmarkObject,
  objectBounds,
  objectsInLasso,
  objectsOf,
  selectable,
  withObjects,
} from './objects';
import { revertToOriginal } from './original';
import { cropTo, resizePage, rotateQuarter } from './ops';
import { deleteSelection, docFromNote, select, selectAll, setLayerVisible, transformSelection } from './state';
import { hline, note } from './test-fixtures';
import { timeRange as splitTimeRange } from './split';
import { lastSampleTime, writingOrigin } from './writing-clock';

let ink: Ink;

beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../../assets/ardoise_ink.wasm', import.meta.url)));
});

function mark(fields: Partial<BookmarkObject> = {}): BookmarkObject {
  return newBookmarkObject(ink.text, { id: 'b', layer: 0, t: 0, x: 1000, y: 2000, label: 'Théorème', ...fields });
}

function pageWith(objects: PageObject[], layers = 1): Note {
  return withObjects(note([hline(500, 3000, 800)], layers), objects);
}

describe('the label', () => {
  it('is kept as one short line, as Rust keeps it', () => {
    expect(cleanBookmarkLabel('  Chap. 3 \n\t Les  forces ')).toBe('Chap. 3 Les forces');
    expect(cleanBookmarkLabel(' \n ')).toBe('');
    expect(Array.from(cleanBookmarkLabel('é'.repeat(BOOKMARK.maxLabel + 20)))).toHaveLength(BOOKMARK.maxLabel);
    // Cut where a space would end the label: no trailing space.
    expect(cleanBookmarkLabel(`${'a'.repeat(BOOKMARK.maxLabel - 1)} suite`)).toBe('a'.repeat(BOOKMARK.maxLabel - 1));
  });

  it('is cleaned when a bookmark is made', () => {
    expect(mark({ label: '  Un   mot ' }).label).toBe('Un mot');
  });
});

describe('the box', () => {
  it('is the flag, the label and room around them, laid out by the shipped font', () => {
    const { pad, flagWidth, fontSize } = BOOKMARK;
    const look = bookmarkLook('Thalès', ink.text);
    const laid = ink.text.layout('Thalès', { font: 'andika', bold: false, size: fontSize });
    expect(look.width).toBeCloseTo(pad + flagWidth + pad + laid.width + pad, 3);
    expect(look.height).toBeCloseTo(1.25 * fontSize + 2 * pad, 3);
    expect(look.textX).toBe(pad + flagWidth + pad);
    expect(look.flag).toMatch(/^M\d/);
    // The box grows with the label and is measured again for the bookmark.
    const narrow = bookmarkLook('Un', ink.text).width;
    expect(look.width).toBeGreaterThan(narrow);
    const stale = { ...mark(), width: 1, height: 1 };
    const fresh = measureBookmark(stale, ink.text);
    expect([fresh.width, fresh.height]).toEqual([bookmarkLook('Théorème', ink.text).width, look.height]);
    // An empty label has a box too (the flag).
    expect(bookmarkLook('', ink.text).width).toBeGreaterThan(pad + flagWidth + 2 * pad - 1);
  });

  it('is measured when a note is read', () => {
    const stale = { ...mark(), width: 10, height: 10 };
    const measured = measureTexts(pageWith([stale]), ink.text);
    const object = objectsOf(measured)[0];
    expect(object.width).toBeGreaterThan(1000);
    // The same note when nothing changes.
    expect(measureTexts(measured, ink.text)).toBe(measured);
  });
});

describe('the listing', () => {
  it('is in reading order, every layer, blank labels left out', () => {
    const n = pageWith(
      [
        mark({ id: 'low', x: 100, y: 5000, label: 'Bas' }),
        mark({ id: 'right', x: 9000, y: 1000, label: '  Droite ' }),
        mark({ id: 'left', x: 100, y: 1000, label: 'Gauche' }),
        mark({ id: 'hidden', layer: 1, x: 100, y: 3000, label: 'Caché' }),
        { ...mark({ id: 'blank' }), label: '   ' },
        { kind: 'text', id: 't', layer: 0, t: 0, x: 0, y: 0, scale: 1, quarterTurns: 0, width: 1, height: 1, text: 'x', font: 'andika', bold: false, size: 600, color: { r: 0, g: 0, b: 0, a: 255 } },
      ],
      2,
    );
    const hiddenLayer = { ...n, layers: [n.layers[0], { ...n.layers[1], visible: false }] };
    expect(bookmarkObjects(hiddenLayer).map((b) => b.id)).toEqual(['left', 'right', 'hidden', 'low']);
    const refs = bookmarkRefs(hiddenLayer);
    expect(refs.map((r) => r.label)).toEqual(['Gauche', 'Droite', 'Caché', 'Bas']);
    expect(refs[0].id).toBe('left');
    // The spot is the centre of the box, rounded.
    const box = objectBounds(objectsOf(hiddenLayer)[2]);
    expect(refs[0]).toMatchObject({ x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) });
  });
});

describe('layers', () => {
  const layered = (): Note => {
    const n = pageWith([mark({ id: 'on-hidden', layer: 1, x: 1000, y: 2000 })], 2);
    return { ...n, layers: [n.layers[0], { ...n.layers[1], visible: false }] };
  };

  it('do not hide a bookmark: it is drawn with its layer\'s objects, selectable and found', () => {
    const n = layered();
    expect(drawItems(n, [0]).some((item) => item.kind === 'object')).toBe(true);
    const o = objectsOf(n)[0];
    expect(selectable(o)).toBe(true);
    const inside = { x: o.x + 100, y: o.y + 100 };
    // The click takes it whatever layer is chosen; the lasso too.
    expect(hitObject(n, inside, 10, 0)).toBe(0);
    expect(hitObject(n, inside, 10, 1)).toBe(0);
    const box = objectBounds(o);
    const around = [
      { x: box.x - 50, y: box.y - 50 },
      { x: box.x + box.width + 50, y: box.y - 50 },
      { x: box.x + box.width + 50, y: box.y + box.height + 50 },
      { x: box.x - 50, y: box.y + box.height + 50 },
    ];
    expect(objectsInLasso(n, around, 0)).toEqual([0]);
    expect(bookmarkAt(n, inside, 10)).toBe(0);
    expect(bookmarkAt(n, { x: 0, y: 0 }, 10)).toBeNull();
  });

  it('hiding the layer of a selected bookmark leaves it selected', () => {
    const n = pageWith([mark({ id: 'b', layer: 1 })], 2);
    const doc = select(docFromNote(n), [], [0]);
    const hidden = setLayerVisible(doc, 1, false);
    expect(hidden.objectSelection).toEqual([0]);
  });
});

describe('page operations carry a bookmark like any object', () => {
  it('moves and resizes with the selection, and goes with a delete', () => {
    const n = pageWith([mark({ x: 1000, y: 2000 })]);
    const doc = select(docFromNote(n), [], [0]);
    const moved = transformSelection(doc, { a: 1, b: 0, c: 0, d: 1, e: 500, f: -300 });
    expect([objectsOf(moved.note)[0].x, objectsOf(moved.note)[0].y]).toEqual([1500, 1700]);
    expect(objectsOf(deleteSelection(doc).note)).toHaveLength(0);
    expect(selectAll(doc).objectSelection).toEqual([0]);
  });

  it('turns, resizes and crops with the page; a crop keeps a bookmark on a hidden layer that lies outside', () => {
    const n = pageWith([mark({ x: 1000, y: 2000 })]);
    const turned = rotateQuarter(n, 'cw');
    expect(objectsOf(turned)[0].quarterTurns).toBe(1);
    expect(objectsOf(resizePage(n, { width: 20000, height: 15000 }))[0].x).toBeGreaterThanOrEqual(0);
    const cropped = cropTo(n, { x: 500, y: 500, width: 9000, height: 9000 });
    expect([objectsOf(cropped)[0].x, objectsOf(cropped)[0].y]).toEqual([500, 1500]);
    // Wholly outside the crop: a visible one goes; so does one on a hidden layer (a bookmark counts as visible).
    const hidden = pageWith([mark({ layer: 1, x: 20000, y: 18000 })], 2);
    const hiddenLayer = { ...hidden, layers: [hidden.layers[0], { ...hidden.layers[1], visible: false }] };
    expect(objectsOf(cropTo(hiddenLayer, { x: 0, y: 0, width: 9000, height: 9000 }))).toHaveLength(0);
  });

  it('is no content for the clocks of the replay, the writing and the split', () => {
    const n = pageWith([mark({ t: 90_000 })]);
    expect(lastSampleTime(n)).toBe(lastSampleTime(note([hline(500, 3000, 800)])));
    expect(writingOrigin(n)).toBe(writingOrigin(note([hline(500, 3000, 800)])));
    expect(splitTimeRange(n)).toEqual(splitTimeRange(note([hline(500, 3000, 800)])));
  });
});

describe('the clipboard', () => {
  it('carries a bookmark: its label and place, not its id nor its layer', () => {
    const text = encodeClipboard([], [mark({ id: 'x', layer: 3, x: 1200, y: 3400, label: 'Copié' })]);
    const parsed = JSON.parse(text);
    expect(parsed.objects).toEqual([{ kind: 'bookmark', t: 0, x: 1200, y: 3400, scale: 1, quarterTurns: 0, label: 'Copié' }]);
    const back = decodeClipboard(text)!;
    expect(back.strokes).toEqual([]);
    expect(back.objects).toHaveLength(1);
    expect(back.objects[0]).toMatchObject({ kind: 'bookmark', id: '', layer: 0, x: 1200, y: 3400, label: 'Copié' });
  });

  it('refuses a damaged bookmark, a blank one and a label that is not text', () => {
    const good = JSON.parse(encodeClipboard([], [mark()]));
    for (const change of [{ label: 7 }, { label: '   ' }, { x: 'left' }, { scale: 0 }, { quarterTurns: 9 }, { t: -1 }]) {
      const bad = { ...good, objects: [{ ...good.objects[0], ...change }] };
      expect(decodeClipboard(JSON.stringify(bad)), JSON.stringify(change)).toBeNull();
    }
  });
});

describe('back to the original', () => {
  it('keeps the bookmarks the note has and takes the ink of the original', () => {
    const current = pageWith([mark({ id: 'keep', layer: 1 })], 2);
    const original = {
      source: 'firstVersion' as const,
      page: { width: 20000, height: 15000 },
      layers: [{ name: 'Calque 1', visible: true }],
      strokes: [],
      objects: [mark({ id: 'old' })],
    };
    const doc = docFromNote(current);
    const reverted = revertToOriginal(doc, original as never);
    // Its own bookmark stays (moved onto the last layer the original has); the original's, which holds none, brings none.
    expect(objectsOf(reverted.note).map((o) => [o.id, o.layer])).toEqual([['keep', 0]]);
    expect(reverted.note.page).toEqual({ width: 20000, height: 15000 });
  });
});
