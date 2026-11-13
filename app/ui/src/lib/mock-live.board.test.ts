// The position of the Xiaomi blackboard in the live stand-in (Q17, docs/design/blackboard.md §11): the run has the page
// of the position set when it starts, a change during the run turns the page with its ink at once (Q19, as the Slate's
// switch), a resumed sheet the other way round is refused (`sheetBoardPosition`), after the other tablet's sheet
// (`sheetOtherTablet`), and a run on a page that keeps its shape offers only the two positions of its way round.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BOARD_POSITIONS, boardRunOrientation, drawingTurns, type BoardPosition } from './board-position';
import type { Stroke } from './note';
import { turnsFrom, turnStrokes } from './orientation';
import type { LiveBatch, LiveSnapshot } from './live';
import { mockLibrary, resetMockLibrary } from './mock-data';
import {
  mockLiveBoardPositionRefusal,
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

  it('takes a new position during the run (Q19), never the Slate orientation; the next run starts with it', async () => {
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition('buttonsLeft');
    expect(start().boardPosition).toBe('buttonsLeft');
    setMockLiveBoardPosition('buttonsBottom');
    setMockLiveOrientation('portraitRight');
    const snapshot = await snapshotNow();
    expect(snapshot.page.page).toEqual(UPRIGHT);
    expect(snapshot.orientation).toBe('landscape');
    expect(snapshot.boardPosition).toBe('buttonsBottom');
    expect(snapshot.boardPositions).toEqual([...BOARD_POSITIONS]);
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
    expect(snapshot.boardPosition).toBeNull();
    // The blackboard's position does not turn a Slate run.
    setMockLiveBoardPosition('buttonsTop');
    expect(start().page.page).toEqual({ width: 21600, height: 29700 });
  });
});

describe('the blackboard switch during a run (Q19)', () => {
  /** A run of the blackboard held as `position`, with some ink written: its finished strokes and their page. */
  async function written(position: BoardPosition): Promise<{ strokes: Stroke[]; size: { width: number; height: number } }> {
    mockLiveConfig.waitMs = 0;
    mockLiveConfig.pen = false;
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition(position);
    start();
    await vi.advanceTimersByTimeAsync(5);
    mockLiveTick(400);
    const page = (await snapshotNow()).page;
    expect(page.strokes.length).toBeGreaterThan(1);
    return { strokes: page.strokes, size: page.page };
  }

  it.each([
    ['buttonsBottom', 'buttonsLeft', 'a quarter turn: the page goes wide'],
    ['buttonsLeft', 'buttonsBottom', 'a quarter turn back: the page goes upright'],
    ['buttonsBottom', 'buttonsTop', 'a half turn: the page keeps its shape'],
    ['buttonsRight', 'buttonsLeft', 'a half turn of a wide page'],
  ] as const)('from %s to %s (%s), the page turns with what is written on it', async (from, to, _turn) => {
    const before = await written(from);
    const sub = await subscribe();
    setMockLiveBoardPosition(to);
    const snapshot = sub.snapshots().at(-1)!;
    expect(snapshot.boardPosition).toBe(to);
    expect(snapshot.orientation).toBe(boardRunOrientation(to));
    expect(snapshot.page.page).toEqual(sizeOf(to));
    const turns = turnsFrom(boardRunOrientation(to), boardRunOrientation(from));
    const turned = turnStrokes(before.strokes, before.size.width, before.size.height, turns);
    // The strokes written before, turned (the one in progress ended before the turn, and turned with them).
    expect(snapshot.page.strokes.slice(0, before.strokes.length)).toEqual(turned);
    // Back: the same ink as before, exactly.
    setMockLiveBoardPosition(from);
    const back = sub.snapshots().at(-1)!;
    expect(back.page.page).toEqual(before.size);
    expect(back.page.strokes.slice(0, before.strokes.length)).toEqual(before.strokes);
  });

  it('writes the strokes that follow the new way, inside the new page', async () => {
    await written('buttonsBottom');
    setMockLiveBoardPosition('buttonsRight');
    mockLiveTick(600);
    const page = (await snapshotNow()).page;
    expect(page.page).toEqual(WIDE);
    for (const p of page.strokes.flatMap((stroke) => stroke.points)) {
      expect(p.x).toBeLessThanOrEqual(WIDE.width);
      expect(p.y).toBeLessThanOrEqual(WIDE.height);
    }
  });

  it('on a resumed sheet offers only the two positions of its way round, and the sheet never turns', async () => {
    mockLiveConfig.waitMs = 0;
    setMockLiveTablet('blackboard');
    setMockLiveBoardPosition('buttonsRight');
    const path = boardSheet(WIDE);
    const resumed = invoke('live_resume', { path }) as LiveSnapshot;
    expect(resumed.boardPositions).toEqual(['buttonsRight', 'buttonsLeft']);
    expect(mockLiveBoardPositionRefusal('buttonsLeft')).toBeNull();
    for (const position of ['buttonsBottom', 'buttonsTop'] as const) {
      expect(mockLiveBoardPositionRefusal(position)).toMatchObject({ code: 'boardPositionFixed' });
    }
    const strokes = JSON.stringify(resumed.page.strokes);
    setMockLiveBoardPosition('buttonsLeft');
    const snapshot = await snapshotNow();
    expect(snapshot.boardPosition).toBe('buttonsLeft');
    expect(snapshot.page.page).toEqual(WIDE);
    expect(JSON.stringify(snapshot.page.strokes)).toBe(strokes);
    // A new page is the session's, but the session still holds the sheet: the same two positions.
    invoke('live_command', { command: { kind: 'newPage' } });
    expect((await snapshotNow()).boardPositions).toEqual(['buttonsRight', 'buttonsLeft']);
    invoke('live_stop');
    expect(mockLiveBoardPositionRefusal('buttonsBottom')).toBeNull();
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
