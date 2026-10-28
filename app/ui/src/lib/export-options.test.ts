import { describe, expect, it } from 'vitest';
import { FORMATS, MAX_IMAGE_SIDE, MAX_WEBP_SIDE, dpiFits, imageSide, isRaster, largestFittingDpi } from './export-options';

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
