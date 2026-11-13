// The position of the Xiaomi blackboard in the live stand-in (Q17, docs/design/blackboard.md §11): the run has the page
// of the position set when it starts, a change during the run applies to the next one, and a resumed sheet the other
// way round is refused (`sheetBoardPosition`), after the other tablet's sheet (`sheetOtherTablet`).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BOARD_POSITIONS, boardRunOrientation, drawingTurns, type BoardPosition } from './board-position';
import type { LiveBatch, LiveSnapshot } from './live';
import { mockLibrary, resetMockLibrary } from './mock-data';
import {
  mockLiveConfig,
  mockLiveInvoke,
  mockLiveSubscribe,
  mockLiveTick,
  resetMockLive,
  setMockLiveBoardPosition,
  setMockLiveOrientation,
  setMockLiveTablet,
} from './mock-live';

const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);
const start = () => invoke('live_start') as LiveSnapshot;
const snapshotNow = () => mockLiveSubscribe(() => {});

/** A subscription that records every batch. */
async function subscribe() {
  const batches: LiveBatch[] = [];
  await mockLiveSubscribe((b) => batches.push(b));
  const events = () => batches.flatMap((b) => b.events);
  return { events, snapshots: () => events().flatMap((e) => (e.type === 'snapshot' ? [e.snapshot] : [])) };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 10, 13, 9, 0));
  resetMockLive();
  resetMockLibrary();
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

const UPRIGHT = { width: 20500, height: 27200 };
const WIDE = { width: 27200, height: 20500 };
const sizeOf = (position: BoardPosition) => (drawingTurns(position) % 2 === 1 ? WIDE : UPRIGHT);

function failure(action: () => unknown): { code: string; message: string } {
  try {
    action();
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error('expected a refusal');
}

/** The demo sheet, made a page of the blackboard of this size. */
function boardSheet(size: { width: number; height: number }): string {
  const [path, stored] = [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')!;
  stored.note.page = { ...size };
  return path;
}

describe('the blackboard run in the live stand-in', () => {
  it.each(BOARD_POSITIONS)('has the page of the position %s, named with the Slate word for its quarter turns', (position) => {
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition(position);
    const snapshot = start();
    expect(snapshot.tablet).toBe('blackboard');
    expect(snapshot.page.page).toEqual(sizeOf(position));
    expect(snapshot.orientation).toBe(boardRunOrientation(position));
    expect(snapshot.view.x).toBe(sizeOf(position).width / 2);
  });

  it('writes inside the page of a landscape position', async () => {
    mockLiveConfig.waitMs = 0;
    mockLiveConfig.tickMs = 5;
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition('buttonsLeft');
    start();
    await vi.advanceTimersByTimeAsync(10);
    mockLiveTick(600);
    const page = (await snapshotNow()).page;
    expect(page.page).toEqual(WIDE);
    const points = page.strokes.flatMap((s) => s.points);
    expect(points.length).toBeGreaterThan(50);
    for (const p of points) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(WIDE.width);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(WIDE.height);
    }
  });

  it('keeps its position when the setting or the Slate orientation change; the next run takes the new one', async () => {
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition('buttonsLeft');
    start();
    setMockLiveBoardPosition('buttonsBottom');
    setMockLiveOrientation('portraitRight');
    const snapshot = await snapshotNow();
    expect(snapshot.page.page).toEqual(WIDE);
    expect(snapshot.orientation).toBe('portraitLeft');
    invoke('live_stop');
    expect(start().page.page).toEqual(UPRIGHT);
  });

  it('takes the position set when the blackboard connects to a run that was waiting for it', async () => {
    setMockLiveBoardPosition('buttonsTop');
    const waiting = start();
    expect(waiting.tablet).toBeNull();
    setMockLiveBoardPosition('buttonsRight');
    setMockLiveTablet('blackboard');
    const snapshot = await snapshotNow();
    expect(snapshot.tablet).toBe('blackboard');
    expect(snapshot.page.page).toEqual(WIDE);
    expect(snapshot.orientation).toBe('portraitRight');
  });

  it('leaves the Slate runs as they were: its orientation is the setting', () => {
    setMockLiveTablet('slate');
    setMockLiveBoardPosition('buttonsLeft');
    setMockLiveOrientation('portraitLeft');
    const snapshot = start();
    expect(snapshot.page.page).toEqual({ width: 21600, height: 29700 });
    expect(snapshot.orientation).toBe('portraitLeft');
  });
});

describe('resuming a sheet with the blackboard connected', () => {
  const pairs = BOARD_POSITIONS.flatMap((position) => [UPRIGHT, WIDE].map((sheet) => [position, sheet] as const));

  it.each(pairs)('%s with a sheet of %j: accepted when it is the same way round, sheetBoardPosition otherwise', (position, sheet) => {
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition(position);
    const path = boardSheet(sheet);
    const before = JSON.stringify(mockLibrary().notes.get(path)!.note);
    if (sizeOf(position).width === sheet.width) {
      const snapshot = invoke('live_resume', { path }) as LiveSnapshot;
      expect(snapshot.page.page).toEqual(sheet);
      expect(snapshot.orientation).toBe(boardRunOrientation(position));
    } else {
      // Nothing started.
      expect(failure(() => invoke('live_resume', { path })).code).toBe('sheetBoardPosition');
    }
    // Nothing written on the note.
    expect(JSON.stringify(mockLibrary().notes.get(path)!.note)).toBe(before);
  });

  it('puts the other tablet sheet first: a sheet of the Slate is sheetOtherTablet, never sheetBoardPosition', () => {
    setMockLiveTablet('blackboard');
    const path = [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];
    expect(mockLibrary().notes.get(path)!.note.page).toEqual({ width: 29700, height: 21600 });
    for (const position of BOARD_POSITIONS) {
      setMockLiveBoardPosition(position);
      expect(failure(() => invoke('live_resume', { path })).code).toBe('sheetOtherTablet');
    }
  });

  it('refuses a page of the blackboard, either way round, with the Slate (sheetOtherTablet)', () => {
    setMockLiveTablet('slate');
    for (const size of [UPRIGHT, WIDE]) {
      expect(failure(() => invoke('live_resume', { path: boardSheet(size) })).code).toBe('sheetOtherTablet');
    }
  });

  it('does not refuse for the position while no tablet is connected: the connection decides', () => {
    setMockLiveBoardPosition('buttonsBottom');
    expect((invoke('live_resume', { path: boardSheet(WIDE) }) as LiveSnapshot).state).toBe('waiting');
  });

  it('refuses when the blackboard connects later, with the position of that moment', async () => {
    setMockLiveBoardPosition('buttonsBottom');
    const path = boardSheet(WIDE);
    const sub = await subscribe();
    invoke('live_resume', { path });
    // The position is changed before the tablet connects: the run takes it, and the wide page now fits.
    setMockLiveBoardPosition('buttonsLeft');
    setMockLiveTablet('blackboard');
    expect(sub.events().some((e) => e.type === 'refused')).toBe(false);
    expect((await snapshotNow()).page.page).toEqual(WIDE);
    invoke('live_stop');

    setMockLiveBoardPosition('buttonsBottom');
    setMockLiveTablet(null);
    invoke('live_resume', { path });
    setMockLiveTablet('blackboard');
    expect(sub.events().find((e) => e.type === 'refused')).toMatchObject({ type: 'refused', code: 'sheetBoardPosition' });
    expect(sub.snapshots().at(-1)?.state).toBe('idle');
  });
});
