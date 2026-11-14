import { describe, expect, it } from 'vitest';
import {
  currentFit,
  displayedSize,
  fitRect,
  formatPageList,
  opacityOf,
  pageMillimetres,
  parsePageList,
  refit,
  rotateImage,
  shiftImage,
} from './background';
import type { BackgroundImage, PageSize } from './note';

/** The Slate's landscape page. */
const PAGE: PageSize = { width: 29700, height: 21600 };

function image(rect: BackgroundImage['rect'], quarterTurns = 0): BackgroundImage {
  return { name: 'fiche.pdf', pdfPage: 2, rect, quarterTurns, opacity: 0.6 };
}

describe('placing a background image', () => {
  it('fits like ImageRect::fit in Rust', () => {
    // A portrait A4 sheet scanned at 100 dpi, on the landscape page (the Rust test's values).
    const contain = fitRect(PAGE, { width: 827, height: 1169 }, 'contain');
    expect(contain.height).toBeCloseTo(21600, 2);
    expect(contain.x).toBeCloseTo((29700 - contain.width) / 2, 2);
    expect(contain.y).toBe(0);
    const cover = fitRect(PAGE, { width: 827, height: 1169 }, 'cover');
    expect(cover.width).toBeCloseTo(29700, 2);
    expect(cover.y).toBeLessThan(0);
    expect(cover.height).toBeGreaterThan(21600);
    const whole = { x: 0, y: 0, width: 29700, height: 21600 };
    expect(fitRect(PAGE, { width: 827, height: 1169 }, 'stretch')).toEqual(whole);
    expect(fitRect(PAGE, { width: 0, height: 10 }, 'contain')).toEqual(whole);
  });

  it('swaps the sides of a turned image and recognises its placement', () => {
    expect(displayedSize({ width: 40, height: 20 }, 1)).toEqual({ width: 20, height: 40 });
    expect(displayedSize({ width: 40, height: 20 }, 2)).toEqual({ width: 40, height: 20 });
    const pixels = { width: 827, height: 1169 };
    for (const fit of ['contain', 'cover', 'stretch'] as const) {
      const placed = image(fitRect(PAGE, pixels, fit));
      expect(currentFit(placed, PAGE, pixels)).toBe(fit);
      expect(refit(placed, PAGE, pixels, 'contain').rect).toEqual(fitRect(PAGE, pixels, 'contain'));
    }
    // A turned image is placed by its displayed size.
    const turned = refit(image(fitRect(PAGE, pixels, 'stretch'), 1), PAGE, pixels, 'contain');
    expect(turned.rect).toEqual(fitRect(PAGE, { width: 1169, height: 827 }, 'contain'));
    expect(currentFit(turned, PAGE, pixels)).toBe('contain');
    // Moved by a crop: no placement any more.
    expect(currentFit(shiftImage(turned, 500, 0), PAGE, pixels)).toBeNull();
    expect(opacityOf({ name: 'x', rect: turned.rect })).toBe(1);
    expect(opacityOf(turned)).toBe(0.6);
  });
});

describe('page operations move the image like the strokes', () => {
  const rect = { x: 1000, y: 2000, width: 6000, height: 3000 };

  it('turns the rectangle like the points and adds a quarter turn', () => {
    // The points of the editor's rotation (`rotateQuarter` in `editor/ops.ts`).
    const cw = (x: number, y: number) => ({ x: PAGE.height - y, y: x });
    const ccw = (x: number, y: number) => ({ x: y, y: PAGE.width - x });
    const turned = rotateImage(image(rect), PAGE, 'cw');
    expect(turned.quarterTurns).toBe(1);
    // The rectangle's top-left corner becomes its top-right one.
    expect(cw(rect.x, rect.y)).toEqual({ x: turned.rect.x + turned.rect.width, y: turned.rect.y });
    expect(turned.rect).toEqual({ x: 21600 - 5000, y: 1000, width: 3000, height: 6000 });
    const back = rotateImage(image(rect), PAGE, 'ccw');
    expect(back.quarterTurns).toBe(3);
    expect(ccw(rect.x, rect.y)).toEqual({ x: back.rect.x, y: back.rect.y + back.rect.height });
    // Four turns, or a turn and back, give the image back; the other fields stay.
    let full = image(rect);
    let page = PAGE;
    for (let i = 0; i < 4; i++) {
      full = rotateImage(full, page, 'cw');
      page = { width: page.height, height: page.width };
    }
    expect(full).toEqual(image(rect));
    expect(rotateImage(turned, { width: PAGE.height, height: PAGE.width }, 'ccw')).toEqual(image(rect));
    expect(turned.name).toBe('fiche.pdf');
    expect(turned.pdfPage).toBe(2);
    expect(turned.opacity).toBe(0.6);
    // An image without quarter turns counts from 0.
    expect(rotateImage({ name: 'x', rect }, PAGE, 'ccw').quarterTurns).toBe(3);
  });

  it('shifts the rectangle by a crop', () => {
    expect(shiftImage(image(rect), 500, 2500).rect).toEqual({ x: 500, y: -500, width: 6000, height: 3000 });
  });
});

describe('page lists of the import', () => {
  it('reads ranges and single pages within the PDF', () => {
    expect(parsePageList('1-3, 5', 8)).toEqual([1, 2, 3, 5]);
    expect(parsePageList(' 4 ; 2–3 ', 8)).toEqual([4, 2, 3]);
    expect(parsePageList('2, 2, 1-2', 3)).toEqual([2, 1]);
    for (const bad of ['', ' , ', '0', '9', '3-1', 'a', '1-', '1.5', '2-9']) {
      expect(parsePageList(bad, 8), bad).toBeNull();
    }
  });

  it('writes pages as a list the field reads back: sorted runs, no repeat', () => {
    expect(formatPageList([])).toBe('');
    expect(formatPageList([5, 1, 2, 3])).toBe('1-3, 5');
    expect(formatPageList([2, 2, 4])).toBe('2, 4');
    expect(formatPageList([7])).toBe('7');
    const pages = [1, 2, 3, 6, 8, 9, 10, 40];
    expect(parsePageList(formatPageList(pages), 50)).toEqual(pages);
  });

  it('shows a page size in millimetres', () => {
    expect(pageMillimetres({ width: 595.28, height: 841.89 })).toEqual({ width: 210, height: 297 });
  });
});
