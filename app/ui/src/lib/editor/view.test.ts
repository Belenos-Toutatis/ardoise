import { describe, expect, it } from 'vitest';
import { fitView, fitWidthView } from './view';

const A4 = { width: 29700, height: 21000 };

describe('fit the page to the width', () => {
  it('fills the width of a viewport that is taller than the page', () => {
    const view = fitWidthView(A4, { width: 1000, height: 1000 });
    expect(view.scale).toBeCloseTo((1000 - 48) / 29700);
    expect(view.x).toBeCloseTo(24);
    // The page is shorter than the viewport: centred vertically.
    expect(view.y).toBeCloseTo((1000 - 21000 * view.scale) / 2);
  });

  it('shows the top of a page taller than the viewport', () => {
    const portrait = { width: 21000, height: 29700 };
    const view = fitWidthView(portrait, { width: 800, height: 500 });
    expect(view.scale).toBeCloseTo((800 - 48) / 21000);
    expect(view.y).toBe(24);
  });

  it('is wider than "fit the page" for a portrait page in a landscape window', () => {
    const portrait = { width: 21000, height: 29700 };
    const viewport = { width: 1200, height: 700 };
    expect(fitWidthView(portrait, viewport).scale).toBeGreaterThan(fitView(portrait, viewport).scale);
  });

  it('equals "fit the page" when the width is the limit', () => {
    const viewport = { width: 600, height: 900 };
    expect(fitWidthView(A4, viewport).scale).toBeCloseTo(fitView(A4, viewport).scale);
  });
});
