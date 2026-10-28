// Milestone M8: portrait/landscape, paper size, image-only turn, clipboard strokes.
import { describe, expect, it } from 'vitest';
import { currentFit, fitRect, turnImageAlone } from '../background';
import type { BackgroundImage, Note, Stroke } from '../note';
import { CLIPBOARD_MARKER, decodeStrokes, encodeStrokes } from './clipboard';
import { commit, createHistory, undo } from './history';
import {
  PASTE_GAP_MS,
  PASTE_STEP,
  pasteOffset,
  resizePage,
  retimedForPaste,
  rotateBackgroundImage,
  setOrientation,
  strokesOutsidePage,
} from './ops';
import {
  PAPER_SIZES,
  inOrientation,
  orientationOf,
  parseMillimetres,
  presetOf,
  presetPage,
  toMillimetres,
} from './page-size';
import { docFromNote, pasteStrokes, rotateBackgroundImage as rotateDocImage, setLayerVisible } from './state';
import { hline, line, note } from './test-fixtures';

const LANDSCAPE = { width: 29700, height: 21600 };
const PORTRAIT = { width: 21600, height: 29700 };

function withImage(n: Note, image: BackgroundImage): Note {
  return { ...n, backgroundImage: image };
}

describe('page sizes', () => {
  it('knows the presets in both orientations, and reads millimetres', () => {
    expect(presetOf({ width: 21000, height: 29700 })).toBe('a4');
    expect(presetOf({ width: 29700, height: 21000 })).toBe('a4');
    expect(presetOf({ width: 21590, height: 27940 })).toBe('letter');
    expect(presetOf(LANDSCAPE)).toBe('tablet');
    expect(presetOf({ width: 20000, height: 30000 })).toBeNull();
    expect(presetPage('a5', LANDSCAPE)).toEqual({ width: 21000, height: 14800 });
    expect(presetPage('a5', PORTRAIT)).toEqual({ width: 14800, height: 21000 });
    expect(inOrientation(PAPER_SIZES.a4, 'landscape')).toEqual({ width: 29700, height: 21000 });
    expect(orientationOf(LANDSCAPE)).toBe('landscape');
    expect(orientationOf({ width: 100, height: 100 })).toBe('portrait');
    expect(toMillimetres(21590)).toBe(215.9);
    expect(parseMillimetres('210')).toBe(21000);
    expect(parseMillimetres(' 215,9 ')).toBe(21590);
    for (const bad of ['', 'abc', '9', '2001', '-5', 'Infinity']) expect(parseMillimetres(bad)).toBeNull();
  });
});

describe('orientation and paper size keep the ink', () => {
  it('swaps width and height without turning the ink, which stays where it is when it fits', () => {
    const stroke = hline(1000, 3000, 2000);
    const swapped = setOrientation(note([stroke]), 'portrait');
    expect(swapped.page).toEqual(PORTRAIT);
    // The same stroke object: nothing moved, nothing turned.
    expect(swapped.strokes[0]).toBe(stroke);
    expect(setOrientation(swapped, 'landscape').page).toEqual(LANDSCAPE);
    // Already so: the same note.
    const n = note([stroke]);
    expect(setOrientation(n, 'landscape')).toBe(n);
  });

  it('moves ink back inside the new page by the smallest shift when it fits', () => {
    const low = hline(5000, 6000, 28000);
    const n = note([low], 1, PORTRAIT);
    // Portrait to landscape (29700 x 21600): the stroke at y 28000 falls below the page; it fits, so it moves up.
    const turned = setOrientation(n, 'landscape');
    const ys = turned.strokes[0].points.map((p) => p.y);
    expect(Math.max(...ys)).toBeCloseTo(21600, 6);
    expect(turned.strokes[0].points.map((p) => p.x)).toEqual(low.points.map((p) => p.x));
    expect(strokesOutsidePage(turned)).toBe(0);
  });

  it('keeps ink that does not fit where it is, and counts it as outside', () => {
    const wide = hline(1000, 28000, 1000);
    const n = note([wide, hline(1000, 2000, 3000)]);
    const portrait = setOrientation(n, 'portrait');
    expect(portrait.strokes[0]).toBe(wide);
    expect(portrait.strokes[1]).toBe(n.strokes[1]);
    expect(strokesOutsidePage(n)).toBe(0);
    expect(strokesOutsidePage(portrait)).toBe(1);
    // The former size brings it back.
    expect(strokesOutsidePage(setOrientation(portrait, 'landscape'))).toBe(0);
  });

  it('counts hidden layers when deciding whether the ink fits, and moves them too', () => {
    const hidden = hline(1000, 3000, 28000, 100, 1);
    const n = {
      ...note([hidden], 2, PORTRAIT),
      layers: [
        { name: 'a', visible: true },
        { name: 'b', visible: false },
      ],
    };
    const turned = setOrientation(n, 'landscape');
    expect(strokesOutsidePage(turned)).toBe(0);
  });

  it('scales the ink, its widths and the image with the page when asked, centred', () => {
    const stroke = line(
      [
        [0, 0],
        [29700, 21600],
      ],
      0,
      'ballpoint',
      100,
    );
    const image = withImage(note([stroke]), {
      name: 'f.png',
      rect: { x: 0, y: 0, width: 29700, height: 21600 },
      quarterTurns: 0,
    });
    // A4 portrait 21000 x 29700: factor min(0.707, 1.375) = 0.707, centred vertically.
    const out = resizePage(image, { width: 21000, height: 29700 }, true);
    const k = 21000 / 29700;
    expect(out.page).toEqual({ width: 21000, height: 29700 });
    const [a, b] = out.strokes[0].points;
    expect(a.x).toBeCloseTo(0, 6);
    expect(a.y).toBeCloseTo((29700 - 21600 * k) / 2, 6);
    expect(b.x).toBeCloseTo(21000, 6);
    expect(out.strokes[0].width).toBeCloseTo(100 * k, 6);
    const rect = out.backgroundImage!.rect;
    expect(rect.width).toBeCloseTo(21000, 6);
    expect(rect.height).toBeCloseTo(21600 * k, 6);
    expect(rect.y).toBeCloseTo(a.y, 6);
    // Times and pressures are untouched.
    expect(out.strokes[0].points.map((p) => p.t)).toEqual(stroke.points.map((p) => p.t));
  });

  it('resizes to a custom size within bounds only', () => {
    const n = note([hline(0, 1000, 500)]);
    expect(resizePage(n, { width: 999, height: 5000 })).toBe(n);
    expect(resizePage(n, { width: 5000, height: 10_000_001 })).toBe(n);
    expect(resizePage(n, LANDSCAPE)).toBe(n);
    const small = resizePage(n, { width: 14800, height: 21000 });
    expect(small.page).toEqual({ width: 14800, height: 21000 });
    expect(small.strokes[0]).toBe(n.strokes[0]);
  });

  it('moves the background image with the ink shift', () => {
    const low = hline(5000, 6000, 28000);
    const n = withImage(note([low], 1, PORTRAIT), {
      name: 'f.png',
      rect: { x: 100, y: 27000, width: 2000, height: 1500 },
      quarterTurns: 1,
      opacity: 0.5,
    });
    const turned = setOrientation(n, 'landscape');
    const dy = turned.strokes[0].points[0].y - low.points[0].y;
    expect(dy).toBeLessThan(0);
    expect(turned.backgroundImage).toMatchObject({
      rect: { x: 100, y: 27000 + dy, width: 2000, height: 1500 },
      quarterTurns: 1,
      opacity: 0.5,
    });
  });

  it('is one undoable step', () => {
    const first = docFromNote(note([hline(0, 1000, 500)]));
    const h = commit(createHistory(first), { ...first, note: setOrientation(first.note, 'portrait') });
    expect(h.present.note.page).toEqual(PORTRAIT);
    expect(undo(h).present.note.page).toEqual(LANDSCAPE);
  });
});

describe('turning the background image alone', () => {
  const pixels = { width: 827, height: 1169 };

  it('turns the pixels and places the image again with its fit, leaving page and ink alone', () => {
    const image: BackgroundImage = {
      name: 'f.pdf',
      rect: fitRect(PORTRAIT, pixels, 'contain'),
      quarterTurns: 0,
      opacity: 1,
    };
    const n = withImage(note([hline(1000, 2000, 1000)], 1, PORTRAIT), image);
    const turned = rotateBackgroundImage(n, pixels, 'cw');
    expect(turned.page).toBe(n.page);
    expect(turned.strokes).toBe(n.strokes);
    const after = turned.backgroundImage!;
    expect(after.quarterTurns).toBe(1);
    expect(currentFit(after, PORTRAIT, pixels)).toBe('contain');
    // Landscape picture on a portrait page: wider than tall.
    expect(after.rect.width).toBeGreaterThan(after.rect.height);
    // Counter-clockwise undoes it, and four turns come back to 0.
    const back = rotateBackgroundImage(turned, pixels, 'ccw').backgroundImage!;
    expect(back.quarterTurns).toBe(0);
    expect(back.rect.x).toBeCloseTo(image.rect.x, 6);
    expect(back.rect.width).toBeCloseTo(image.rect.width, 6);
    expect(turnImageAlone({ ...image, quarterTurns: 3 }, PORTRAIT, pixels, 'cw').quarterTurns).toBe(0);
    expect(turnImageAlone(image, PORTRAIT, pixels, 'ccw').quarterTurns).toBe(3);
  });

  it('does not stretch an image whose proportions made every fit the same', () => {
    const exact = { width: 2160, height: 2970 };
    const image: BackgroundImage = { name: 'f.png', rect: { x: 0, y: 0, width: 21600, height: 29700 }, quarterTurns: 0 };
    const after = turnImageAlone(image, PORTRAIT, exact, 'cw');
    expect(after.rect.width / after.rect.height).toBeCloseTo(2970 / 2160, 6);
    expect(after.rect.width).toBeCloseTo(21600, 6);
  });

  it('turns a moved image in place around its centre, also without pixels', () => {
    const image: BackgroundImage = { name: 'f.png', rect: { x: 1000, y: 2000, width: 4000, height: 2000 } };
    const after = turnImageAlone(image, PORTRAIT, undefined, 'cw');
    expect(after.rect).toEqual({ x: 2000, y: 1000, width: 2000, height: 4000 });
    expect(after.quarterTurns).toBe(1);
  });

  it('does nothing without an image, and works on the doc with its pixels', () => {
    const n = note([]);
    expect(rotateBackgroundImage(n, undefined, 'cw')).toBe(n);
    const image: BackgroundImage = { name: 'f.pdf', rect: fitRect(PORTRAIT, pixels, 'contain'), quarterTurns: 0 };
    const doc = { ...docFromNote(withImage(note([], 1, PORTRAIT), image)), backgroundPixels: { key: 'k', ...pixels } };
    const next = rotateDocImage(doc, 'cw');
    expect(next.note.backgroundImage!.quarterTurns).toBe(1);
    expect(next.backgroundPixels).toBe(doc.backgroundPixels);
  });
});

describe('clipboard text', () => {
  const stroke: Stroke = {
    ...line(
      [
        [10, 20],
        [30, 40],
      ],
      3,
      'pencil',
      80,
    ),
    color: { r: 1, g: 2, b: 3, a: 255 },
  };
  const wrap = (strokes: unknown, extra: object = {}) =>
    JSON.stringify({ ardoise: CLIPBOARD_MARKER, version: 1, strokes, ...extra });

  it('round-trips colour, tool, width and samples, and puts the strokes on layer 0 for the paste to place', () => {
    const text = encodeStrokes([stroke]);
    expect(text).toContain(CLIPBOARD_MARKER);
    expect(decodeStrokes(text)).toEqual([{ ...stroke, layer: 0 }]);
  });

  it('ignores anything foreign or damaged', () => {
    const bad = [
      '',
      'hello',
      '{"strokes":[]}',
      wrap([stroke], { ardoise: 'other' }),
      wrap([stroke], { version: 2 }),
      wrap([]),
      wrap([{ ...stroke, tool: 'laser' }]),
      wrap([{ ...stroke, width: -1 }]),
      wrap([{ ...stroke, color: { r: 999, g: 0, b: 0, a: 255 } }]),
      wrap([{ ...stroke, points: [] }]),
      wrap([{ ...stroke, points: [{ x: 'a', y: 0, pressure: 0, t: 0 }] }]),
      `{"ardoise":"${CLIPBOARD_MARKER}","version":1,"strokes":[`,
    ];
    for (const text of bad) expect(decodeStrokes(text), text).toBeNull();
  });
});

describe('pasting strokes', () => {
  const copied = [
    line(
      [
        [1000, 1000],
        [2000, 1500],
      ],
      2,
      'brush',
      90,
    ),
    line(
      [
        [1500, 2500],
        [2500, 2600],
      ],
      2,
      'brush',
      90,
    ),
  ];

  it('keeps colour, width and points, goes to the active layer, selects the copies', () => {
    const n = note([hline(0, 500, 100)], 2);
    const doc = { ...docFromNote(n), activeLayer: 1 };
    const next = pasteStrokes(doc, copied);
    expect(next.note.strokes).toHaveLength(3);
    expect(next.selection).toEqual([1, 2]);
    for (const [i, source] of copied.entries()) {
      const pasted = next.note.strokes[1 + i];
      expect(pasted.layer).toBe(1);
      expect(pasted.tool).toBe('brush');
      expect(pasted.width).toBe(90);
      expect(pasted.color).toEqual(source.color);
      expect(pasted.points.map((p) => [p.x, p.y, p.pressure])).toEqual(
        source.points.map((p) => [p.x, p.y, p.pressure]),
      );
    }
    expect(next.note.strokes[0]).toBe(n.strokes[0]);
  });

  it('retimes the copies after the end of the note, keeping their rhythm', () => {
    const n = note([
      line([
        [0, 0],
        [10, 10],
        [20, 20],
      ]),
    ]); // ends at t = 20
    const moved = retimedForPaste(n, [
      line([
        [0, 0],
        [5, 5],
      ]),
      line([
        [1, 1],
        [6, 6],
      ]),
    ]);
    expect(moved[0].points.map((p) => p.t)).toEqual([20 + PASTE_GAP_MS, 30 + PASTE_GAP_MS]);
    expect(moved[1].points.map((p) => p.t)).toEqual([20 + PASTE_GAP_MS, 30 + PASTE_GAP_MS]);
    expect(retimedForPaste(note([]), [line([[0, 0]])])[0].points[0].t).toBe(PASTE_GAP_MS);
  });

  it('offsets a paste on top of an identical copy, again each time, and not elsewhere', () => {
    const doc = docFromNote(note([]));
    const first = pasteStrokes(doc, copied);
    // Into a note without the strokes: in place.
    expect(first.note.strokes[0].points[0]).toMatchObject({ x: 1000, y: 1000 });
    const second = pasteStrokes(first, copied);
    expect(second.note.strokes[2].points[0]).toMatchObject({ x: 1000 + PASTE_STEP, y: 1000 + PASTE_STEP });
    const third = pasteStrokes(second, copied);
    expect(third.note.strokes[4].points[0]).toMatchObject({ x: 1000 + 2 * PASTE_STEP, y: 1000 + 2 * PASTE_STEP });
    expect(third.selection).toEqual([4, 5]);
    expect(pasteOffset(note([]), copied)).toEqual({ x: 0, y: 0 });
  });

  it('centres strokes that would lie wholly outside the page', () => {
    const far = [
      line([
        [40000, 30000],
        [41000, 30500],
      ]),
    ];
    const offset = pasteOffset(note([]), far);
    const moved = [offset.x + 40000, offset.y + 30000];
    expect(moved[0]).toBeGreaterThan(0);
    expect(moved[0]).toBeLessThan(29700);
    expect(moved[1]).toBeLessThan(21600);
  });

  it('is one undo step, and does nothing on a hidden layer', () => {
    const doc = docFromNote(note([hline(0, 500, 100)]));
    const h = commit(createHistory(doc), pasteStrokes(doc, copied));
    expect(undo(h).present.note.strokes).toHaveLength(1);
    const hidden = setLayerVisible(docFromNote(note([], 2)), 0, false);
    const hiddenActive = { ...hidden, activeLayer: 0 };
    expect(pasteStrokes(hiddenActive, copied)).toBe(hiddenActive);
    expect(pasteStrokes(doc, [])).toBe(doc);
  });
});
