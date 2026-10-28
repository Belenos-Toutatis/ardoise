import { describe, expect, it } from 'vitest';
import {
  DEFAULT_PRESSURE,
  DOUBLE_TAP_MS,
  PAN_SLOP,
  PenPresence,
  TAP_MS,
  TouchGestures,
  barrelHeld,
  inkPressure,
  pinchAction,
  pinchView,
  pointerKind,
} from './gestures';

const navigate = { mode: 'navigate' } as const;
const draw = { mode: 'draw' } as const;

describe('pointer classification', () => {
  it('tells mouse, pen tip, pen eraser end and finger apart', () => {
    expect(pointerKind({ pointerType: 'mouse', buttons: 1 })).toBe('mouse');
    expect(pointerKind({})).toBe('mouse');
    expect(pointerKind({ pointerType: 'touch', buttons: 1 })).toBe('touch');
    expect(pointerKind({ pointerType: 'pen', buttons: 1, button: 0 })).toBe('pen');
    expect(pointerKind({ pointerType: 'pen', buttons: 0 })).toBe('pen');
    expect(pointerKind({ pointerType: 'pen', buttons: 1 | 32 })).toBe('eraser');
    expect(pointerKind({ pointerType: 'pen', buttons: 32 })).toBe('eraser');
    expect(pointerKind({ pointerType: 'pen', buttons: 0, button: 5 })).toBe('eraser');
    // A mouse never erases, whatever its buttons say.
    expect(pointerKind({ pointerType: 'mouse', buttons: 32 })).toBe('mouse');
  });

  it('sees the barrel button of a pen only', () => {
    expect(barrelHeld({ pointerType: 'pen', buttons: 1 | 2 })).toBe(true);
    expect(barrelHeld({ pointerType: 'pen', buttons: 2 })).toBe(true);
    expect(barrelHeld({ pointerType: 'pen', buttons: 1 })).toBe(false);
    expect(barrelHeld({ pointerType: 'pen', buttons: 2 | 32 })).toBe(false);
    // A mouse's right button is not the barrel.
    expect(barrelHeld({ pointerType: 'mouse', buttons: 2 })).toBe(false);
    expect(barrelHeld({ pointerType: 'touch', buttons: 2 })).toBe(false);
  });
});

describe('pressure', () => {
  it('keeps the pen pressure so that the line follows it', () => {
    expect(inkPressure('pen', 0.25)).toBe(0.25);
    expect(inkPressure('pen', 1)).toBe(1);
    expect(inkPressure('eraser', 0.4)).toBe(0.4);
  });
  it('gives a constant pressure to a finger and a floor to a driver that reports 0', () => {
    expect(inkPressure('touch', 1)).toBe(DEFAULT_PRESSURE);
    expect(inkPressure('touch', 0.2)).toBe(DEFAULT_PRESSURE);
    expect(inkPressure('pen', 0)).toBe(DEFAULT_PRESSURE);
    expect(inkPressure('mouse', undefined)).toBe(DEFAULT_PRESSURE);
    // The mouse keeps the 0.5 it reports (no change from before).
    expect(inkPressure('mouse', 0.5)).toBe(0.5);
  });
});

describe('palm rejection', () => {
  it('blocks fingers while the pen is in contact, then while it hovers, then no more', () => {
    const pen = new PenPresence();
    expect(pen.blocksTouch(0)).toBe(false);
    pen.seen(1, 100, 'down');
    expect(pen.blocksTouch(5000)).toBe(true);
    pen.lifted(1, 5000);
    expect(pen.blocksTouch(5000 + PenPresence.HOVER_MS - 1)).toBe(true);
    expect(pen.blocksTouch(5000 + PenPresence.HOVER_MS)).toBe(false);
  });
  it('blocks a hovering pen until it leaves the screen', () => {
    const pen = new PenPresence();
    pen.seen(1, 100, 'hover');
    expect(pen.blocksTouch(200)).toBe(true);
    pen.gone(1);
    expect(pen.blocksTouch(200)).toBe(false);
  });
  it('keeps blocking while any of two pens is down', () => {
    const pen = new PenPresence();
    pen.seen(1, 0, 'down');
    pen.seen(2, 10, 'down');
    pen.gone(1);
    expect(pen.blocksTouch(1e9)).toBe(true);
    pen.gone(2);
    expect(pen.blocksTouch(1e9)).toBe(false);
  });
});

describe('finger gestures', () => {
  it('pans with one finger after the slop, without a jump', () => {
    const g = new TouchGestures();
    g.down(1, { x: 100, y: 100 }, 0);
    expect(g.move(1, { x: 100 + PAN_SLOP - 1, y: 100 }, navigate)).toBeNull();
    // Crossing the slop starts the pan from there.
    expect(g.move(1, { x: 120, y: 100 }, navigate)).toBeNull();
    expect(g.move(1, { x: 130, y: 105 }, navigate)).toEqual({ kind: 'pan', dx: 10, dy: 5 });
    expect(g.move(1, { x: 130, y: 105 }, navigate)).toBeNull();
  });

  it('does not pan in draw mode (the finger belongs to the tool)', () => {
    const g = new TouchGestures();
    g.down(1, { x: 0, y: 0 }, 0);
    expect(g.move(1, { x: 50, y: 0 }, draw)).toBeNull();
    expect(g.move(1, { x: 90, y: 0 }, draw)).toBeNull();
  });

  it('pinches with two fingers: zoom factor and the centre moving', () => {
    const g = new TouchGestures();
    expect(g.down(1, { x: 100, y: 100 }, 0)).toBe(false);
    expect(g.down(2, { x: 200, y: 100 }, 10)).toBe(true);
    // The second finger moves away: distance 100 -> 150, centre 150 -> 175.
    expect(g.move(2, { x: 250, y: 100 }, navigate)).toEqual({
      kind: 'pinch',
      from: { x: 150, y: 100 },
      to: { x: 175, y: 100 },
      factor: 1.5,
    });
    // Both fingers move in draw mode too: the pinch always works.
    const d = new TouchGestures();
    d.down(1, { x: 0, y: 0 }, 0);
    d.down(2, { x: 100, y: 0 }, 0);
    expect(d.move(1, { x: 25, y: 0 }, draw)).toMatchObject({ kind: 'pinch', factor: 0.75 });
  });

  it('ignores a third finger and carries on with one finger after a pinch', () => {
    const g = new TouchGestures();
    g.down(1, { x: 0, y: 0 }, 0);
    g.down(2, { x: 100, y: 0 }, 0);
    expect(g.down(3, { x: 50, y: 50 }, 0)).toBe(false);
    expect(g.count).toBe(2);
    expect(g.move(3, { x: 60, y: 60 }, navigate)).toBeNull();
    g.up(2, 100, navigate);
    // The remaining finger pans at once (no slop, no jump).
    expect(g.move(1, { x: 3, y: 4 }, navigate)).toEqual({ kind: 'pan', dx: 3, dy: 4 });
  });

  it('recognises a double tap, but not a slow, moving or two-finger tap', () => {
    const tap = (g: TouchGestures, id: number, x: number, t0: number, hold = 80) => {
      g.down(id, { x, y: 50 }, t0);
      return g.up(id, t0 + hold, navigate);
    };
    const g = new TouchGestures();
    expect(tap(g, 1, 50, 0)).toBeNull();
    expect(tap(g, 2, 60, 200)).toEqual({ kind: 'doubleTap', at: { x: 60, y: 50 } });
    // Back to square one: a third tap alone is not a double tap.
    expect(tap(g, 3, 60, 300)).toBeNull();

    const slow = new TouchGestures();
    expect(tap(slow, 1, 50, 0)).toBeNull();
    expect(tap(slow, 2, 50, DOUBLE_TAP_MS + 100)).toBeNull();

    const far = new TouchGestures();
    expect(tap(far, 1, 50, 0)).toBeNull();
    expect(tap(far, 2, 400, 200)).toBeNull();

    const long = new TouchGestures();
    expect(tap(long, 1, 50, 0, TAP_MS + 50)).toBeNull();
    expect(tap(long, 2, 50, 600)).toBeNull();

    const dragged = new TouchGestures();
    dragged.down(1, { x: 0, y: 0 }, 0);
    dragged.move(1, { x: 40, y: 0 }, navigate);
    expect(dragged.up(1, 50, navigate)).toBeNull();
    expect(tap(dragged, 2, 0, 100)).toBeNull();

    const two = new TouchGestures();
    two.down(1, { x: 0, y: 0 }, 0);
    two.down(2, { x: 100, y: 0 }, 0);
    two.up(2, 40, navigate);
    expect(two.up(1, 60, navigate)).toBeNull();
    expect(tap(two, 3, 0, 100)).toBeNull();
  });

  it('draws no double tap in draw mode, and forgets a cancelled finger', () => {
    const g = new TouchGestures();
    g.down(1, { x: 0, y: 0 }, 0);
    expect(g.up(1, 50, draw)).toBeNull();
    g.down(2, { x: 0, y: 0 }, 100);
    expect(g.up(2, 150, draw)).toBeNull();
    g.down(3, { x: 0, y: 0 }, 200);
    g.cancel(3);
    expect(g.count).toBe(0);
    g.down(4, { x: 0, y: 0 }, 300);
    g.reset();
    expect(g.count).toBe(0);
    expect(g.has(4)).toBe(false);
  });
});

describe('pinch maths', () => {
  it('is null for fingers that did not move, and a plain pan for fingers on top of each other', () => {
    const pair: [{ x: number; y: number }, { x: number; y: number }] = [
      { x: 0, y: 0 },
      { x: 100, y: 0 },
    ];
    expect(pinchAction(pair, pair)).toBeNull();
    expect(
      pinchAction(
        [
          { x: 0, y: 0 },
          { x: 2, y: 0 },
        ],
        [
          { x: 10, y: 0 },
          { x: 12, y: 0 },
        ],
      ),
    ).toEqual({ kind: 'pan', dx: 10, dy: 0 });
  });

  it('keeps the page point under the fingers while zooming and moving', () => {
    const view = { scale: 0.02, x: 30, y: 40 };
    const from = { x: 200, y: 150 };
    const to = { x: 260, y: 120 };
    const next = pinchView(view, from, to, 2);
    expect(next.scale).toBeCloseTo(0.04);
    // The page point that was under `from` is now under `to`.
    const page = { x: (from.x - view.x) / view.scale, y: (from.y - view.y) / view.scale };
    expect(page.x * next.scale + next.x).toBeCloseTo(to.x);
    expect(page.y * next.scale + next.y).toBeCloseTo(to.y);
  });

  it('stops at the zoom limits and the point follows the zoom really reached', () => {
    const view = { scale: 0.3, x: 0, y: 0 };
    const next = pinchView(view, { x: 100, y: 100 }, { x: 100, y: 100 }, 10);
    expect(next.scale).toBeCloseTo(0.4);
    expect((100 / view.scale) * next.scale + next.x).toBeCloseTo(100);
  });
});
