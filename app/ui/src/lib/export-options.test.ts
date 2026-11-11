// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_CHOICE,
  FORMATS,
  MAX_IMAGE_SIDE,
  MAX_WEBP_SIDE,
  dpiFits,
  hasLayers,
  imageSide,
  isRaster,
  largestFittingDpi,
  layerInk,
  loadChoice,
  saveChoice,
} from './export-options';
import type { Note, Stroke } from './note';

const A4 = { width: 29700, height: 21600 };
/** 3 m wide: 17 717 px at 150 dpi, 11 339 px at 96 dpi, 8 504 px at 72 dpi. */
const WIDE = { width: 300_000, height: 21600 };
/** 5 m wide: only 72 dpi gives less than 16 384 px (14 173). */
const HUGE = { width: 500_000, height: 21600 };
/** 7 m wide: nothing fits, even at 72 dpi. */
const GIANT = { width: 700_000, height: 21600 };

describe('raster size limit', () => {
  it('converts device units (10 µm) to pixels the way the renderer rounds them', () => {
    expect(imageSide(2540, 150)).toBe(150);
    expect(imageSide(29700, 300)).toBe(3508);
    expect(MAX_IMAGE_SIDE).toBe(16384);
  });

  it('keeps every resolution for an ordinary page', () => {
    for (const dpi of [72, 96, 150, 300, 600]) expect(dpiFits([A4], dpi)).toBe(true);
  });

  it('refuses the resolutions whose image would exceed 16 384 px, for the largest note', () => {
    expect(dpiFits([WIDE], 150)).toBe(false);
    expect(dpiFits([WIDE], 96)).toBe(true);
    expect(dpiFits([A4, WIDE], 150)).toBe(false);
    // The height counts as much as the width.
    expect(dpiFits([{ width: 21600, height: 300_000 }], 150)).toBe(false);
    // Exactly the limit is fine.
    expect(imageSide(MAX_IMAGE_SIDE * 2540, 1)).toBe(MAX_IMAGE_SIDE);
    expect(dpiFits([{ width: MAX_IMAGE_SIDE * 2540, height: 1000 }], 1)).toBe(true);
  });

  it('keeps WebP one pixel under the other formats, and knows which formats are pixels', () => {
    const edge = { width: MAX_IMAGE_SIDE * 2540, height: 1000 };
    expect(MAX_WEBP_SIDE).toBe(MAX_IMAGE_SIDE - 1);
    expect(dpiFits([edge], 1, 'png')).toBe(true);
    expect(dpiFits([edge], 1, 'webp')).toBe(false);
    expect(dpiFits([A4], 300, 'webp')).toBe(true);
    expect(largestFittingDpi([WIDE], [150, 96], 'webp')).toBe(96);
    expect(FORMATS.filter(isRaster)).toEqual(['png', 'jpg', 'webp']);
  });

  it('picks the largest of the candidates that fits, null when none does', () => {
    const candidates = [150, 96, 72];
    expect(largestFittingDpi([A4], candidates)).toBe(150);
    expect(largestFittingDpi([WIDE], candidates)).toBe(96);
    expect(largestFittingDpi([HUGE], candidates)).toBe(72);
    expect(largestFittingDpi([GIANT], candidates)).toBeNull();
  });
});

describe('remembered choice', () => {
  const KEY = 'ardoise.export';
  beforeEach(() => localStorage.clear());
  const stored = (layers?: unknown) => ({
    format: 'pdf',
    options: { dpi: 300, background: false, quality: 70, pdfPage: 'a4' },
    ...(layers === undefined ? {} : { layers }),
  });

  it('starts from the defaults: SVG separate, PDF merged, as the files were before the choice', () => {
    expect(DEFAULT_CHOICE.layers).toEqual({ svg: 'separate', pdf: 'merged' });
    expect(loadChoice()).toEqual(DEFAULT_CHOICE);
    // Each load gives its own object.
    loadChoice().layers.svg = 'merged';
    expect(loadChoice().layers.svg).toBe('separate');
    expect(DEFAULT_CHOICE.layers.svg).toBe('separate');
  });

  it('keeps one choice per format', () => {
    saveChoice({ ...DEFAULT_CHOICE, format: 'svg', layers: { svg: 'merged', pdf: 'separate' } });
    const choice = loadChoice();
    expect(choice.format).toBe('svg');
    expect(choice.layers).toEqual({ svg: 'merged', pdf: 'separate' });
  });

  it('reads the defaults for a choice stored before the layers existed, or with another word', () => {
    localStorage.setItem(KEY, JSON.stringify(stored()));
    const old = loadChoice();
    expect(old.options).toEqual({ dpi: 300, background: false, quality: 70, pdfPage: 'a4' });
    expect(old.layers).toEqual({ svg: 'separate', pdf: 'merged' });

    localStorage.setItem(KEY, JSON.stringify(stored({ svg: 'both', pdf: 'separate' })));
    expect(loadChoice().layers).toEqual({ svg: 'separate', pdf: 'separate' });
    localStorage.setItem(KEY, JSON.stringify(stored({ svg: 3, pdf: null })));
    expect(loadChoice().layers).toEqual({ svg: 'separate', pdf: 'merged' });
    for (const garbage of ['x', 7, [], true]) {
      localStorage.setItem(KEY, JSON.stringify(stored(garbage)));
      expect(loadChoice().layers).toEqual({ svg: 'separate', pdf: 'merged' });
    }
  });

  it('knows which formats can keep layers apart', () => {
    expect(FORMATS.filter(hasLayers)).toEqual(['svg', 'pdf']);
  });
});

describe('layers of a note at export', () => {
  const stroke = (layer: number, points = 3): Stroke => ({
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer,
    points: Array.from({ length: points }, (_, i) => ({ x: i, y: i, pressure: 1, t: i })),
  });
  const note = (layers: [string, boolean][], strokes: Stroke[]) =>
    ({ layers: layers.map(([name, visible]) => ({ name, visible })), strokes }) as unknown as Note;

  it('counts the visible layers that hold ink, and tells when a hidden one does', () => {
    expect(layerInk(note([['A', true]], [stroke(0)]))).toEqual({ visible: 1, hidden: false });
    expect(
      layerInk(
        note(
          [
            ['A', true],
            ['B', true],
          ],
          [stroke(0), stroke(0), stroke(1)],
        ),
      ),
    ).toEqual({ visible: 2, hidden: false });
    expect(
      layerInk(
        note(
          [
            ['A', true],
            ['B', false],
          ],
          [stroke(0), stroke(1)],
        ),
      ),
    ).toEqual({ visible: 1, hidden: true });
  });

  it('ignores a layer without ink, and counts a stroke whose layer has no entry as visible', () => {
    const two: [string, boolean][] = [
      ['A', true],
      ['B', true],
    ];
    expect(layerInk(note(two, [stroke(0)])).visible).toBe(1);
    expect(layerInk(note([['A', true], ['B', false]], [stroke(0), stroke(1, 0)]))).toEqual({ visible: 1, hidden: false });
    expect(layerInk(note([['A', true]], [stroke(0), stroke(4)])).visible).toBe(2);
    expect(layerInk(note([], []))).toEqual({ visible: 0, hidden: false });
  });
});
