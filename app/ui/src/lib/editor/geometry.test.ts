import { describe, expect, it } from 'vitest';
import {
  affineScale,
  applyAffine,
  boundsOfPoints,
  densify,
  intersection,
  pointInPolygon,
  polylineDistanceSq,
  polylinesDistanceSq,
  rectFromCorners,
  scalingAround,
  segmentDistanceSq,
  segmentsDistanceSq,
  strokeBounds,
  union,
} from './geometry';
import { hline } from './test-fixtures';
import { backgroundWidth, fitView, pageToScreen, screenToPage, visibleRect, zoomAt, MAX_SCALE } from './view';

describe('geometry', () => {
  it('tests points against a lasso polygon, concave shapes included', () => {
    // A "U": the notch between the arms is outside.
    const u = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 30 },
      { x: 20, y: 30 },
      { x: 20, y: 0 },
      { x: 30, y: 0 },
      { x: 30, y: 40 },
      { x: 0, y: 40 },
    ];
    expect(pointInPolygon({ x: 5, y: 5 }, u)).toBe(true);
    expect(pointInPolygon({ x: 25, y: 20 }, u)).toBe(true);
    expect(pointInPolygon({ x: 15, y: 35 }, u)).toBe(true);
    expect(pointInPolygon({ x: 15, y: 10 }, u)).toBe(false);
    expect(pointInPolygon({ x: 40, y: 10 }, u)).toBe(false);
    expect(pointInPolygon({ x: 5, y: 5 }, [])).toBe(false);
  });

  it('measures distances to segments and between polylines', () => {
    expect(segmentDistanceSq({ x: 5, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(9);
    expect(segmentDistanceSq({ x: -3, y: 4 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(25);
    expect(segmentDistanceSq({ x: 1, y: 1 }, { x: 0, y: 0 }, { x: 0, y: 0 })).toBe(2);
    // Crossing segments touch; parallel ones are their gap apart.
    expect(segmentsDistanceSq({ x: 0, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }, { x: 10, y: 0 })).toBe(0);
    expect(segmentsDistanceSq({ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 0, y: 4 }, { x: 10, y: 4 })).toBe(16);
    expect(polylineDistanceSq({ x: 5, y: 5 }, [{ x: 0, y: 0 }])).toBe(50);
    expect(polylineDistanceSq({ x: 5, y: 5 }, [])).toBe(Infinity);
    expect(
      polylinesDistanceSq(
        [
          { x: 0, y: 0 },
          { x: 10, y: 0 },
        ],
        [{ x: 5, y: 2 }],
      ),
    ).toBe(4);
  });

  it('combines rectangles and bounds strokes with half their width', () => {
    expect(rectFromCorners({ x: 10, y: 5 }, { x: 0, y: 20 })).toEqual({ x: 0, y: 5, width: 10, height: 15 });
    expect(union(null, { x: 0, y: 0, width: 1, height: 1 })).toEqual({ x: 0, y: 0, width: 1, height: 1 });
    expect(union({ x: 0, y: 0, width: 1, height: 1 }, { x: 5, y: 5, width: 1, height: 1 })).toEqual({
      x: 0,
      y: 0,
      width: 6,
      height: 6,
    });
    expect(intersection({ x: 0, y: 0, width: 10, height: 10 }, { x: 20, y: 0, width: 5, height: 5 })).toBeNull();
    expect(intersection({ x: 0, y: 0, width: 10, height: 10 }, { x: 5, y: 5, width: 10, height: 10 })).toEqual({
      x: 5,
      y: 5,
      width: 5,
      height: 5,
    });
    expect(boundsOfPoints([])).toBeNull();
    expect(strokeBounds(hline(1000, 2000, 500))).toEqual({ x: 970, y: 470, width: 1060, height: 60 });
  });

  it('applies affine maps and measures their change of scale', () => {
    const m = scalingAround({ x: 100, y: 100 }, 2);
    expect(applyAffine(m, { x: 100, y: 100 })).toEqual({ x: 100, y: 100 });
    expect(applyAffine(m, { x: 150, y: 100 })).toEqual({ x: 200, y: 100 });
    expect(affineScale(m)).toBe(2);
  });

  it('subdivides long segments, interpolating pressure and time', () => {
    const points = [
      { x: 0, y: 0, pressure: 0, t: 0 },
      { x: 100, y: 0, pressure: 1, t: 100 },
    ];
    const dense = densify(points, 25);
    expect(dense.map((p) => p.x)).toEqual([0, 25, 50, 75, 100]);
    expect(dense[2].pressure).toBe(0.5);
    expect(dense[1].t).toBe(25);
    expect(densify(points, 200)).toBe(points);
  });
});

describe('zoom and pan', () => {
  it('maps screen and page coordinates both ways', () => {
    const view = { scale: 0.05, x: 30, y: 40 };
    const p = { x: 1234, y: 5678 };
    const back = screenToPage(view, pageToScreen(view, p));
    expect(back.x).toBeCloseTo(p.x, 9);
    expect(back.y).toBeCloseTo(p.y, 9);
  });

  it('zooms at the cursor, keeping the point under it in place', () => {
    const view = { scale: 0.04, x: 10, y: 20 };
    const at = { x: 400, y: 300 };
    const before = screenToPage(view, at);
    const zoomed = zoomAt(view, at, 2);
    expect(zoomed.scale).toBeCloseTo(0.08, 9);
    const after = screenToPage(zoomed, at);
    expect(after.x).toBeCloseTo(before.x, 6);
    expect(after.y).toBeCloseTo(before.y, 6);
    expect(zoomAt(view, at, 1e6).scale).toBe(MAX_SCALE);
  });

  it('fits the page in the viewport, centred', () => {
    const view = fitView({ width: 29700, height: 21600 }, { width: 1048, height: 800 }, 24);
    expect(view.scale).toBeCloseTo(1000 / 29700, 9);
    expect(view.x).toBeCloseTo(24, 6);
    const r = visibleRect(view, { width: 1048, height: 800 });
    expect(r.x + r.width / 2).toBeCloseTo(29700 / 2, 6);
    expect(r.y + r.height / 2).toBeCloseTo(21600 / 2, 6);
  });

  it('renders the background at the next power of two of its on-screen width', () => {
    expect(backgroundWidth({ width: 29700, height: 21600 }, 1000 / 29700)).toBe(1024);
    expect(backgroundWidth({ width: 29700, height: 21600 }, 1000 / 29700, 2)).toBe(2048);
    expect(backgroundWidth({ width: 29700, height: 21600 }, 0.001)).toBe(512);
    expect(backgroundWidth({ width: 29700, height: 21600 }, 1)).toBe(4096);
  });
});
