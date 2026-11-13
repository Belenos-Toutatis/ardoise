// @vitest-environment jsdom
// H8: the mock blackboard refuses live mode as the service does while it holds stored pages that may not be synced first
// (SPEC §4.2, docs/design/blackboard.md §9.5), so that the dialog can be tried in the browser and in tests.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import type { LiveBatch } from './live';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, resetMockLive } from './mock-live';
import { mockBoardDraw, mockTablet, resetMockTablet } from './mock-tablet';
import type { TabletOptions } from './tablet';

const OPTIONS: TabletOptions = { autoSync: false, deleteAfterImport: false, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' };

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockLive();
  resetMockTablet();
  // The Slate is out of range: the blackboard is the tablet that connects.
  mockTablet.inRange = false;
  mockLiveConfig.waitMs = 60_000;
});

afterEach(() => {
  resetMockLive();
  resetMockTablet();
  vi.useRealTimers();
});

const beat = (beats = 1) => vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));

/** Hears and adds the blackboard, with these options; connected when `connect`. */
async function board(options: Partial<TabletOptions>, { connect = true } = {}) {
  await api.tabletSetOptions({ ...OPTIONS, ...options });
  if (!connect) mockTablet.boardInRange = false;
  await api.tabletFindBlackboards();
  await beat(1);
  await api.tabletAddBlackboard((await api.tabletStatus()).boardSearch!.found[0].address);
  if (connect) await beat(1.5);
}

function refusal(action: () => unknown): { code: string; message: string } {
  try {
    action();
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error('expected a refusal');
}

const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];

describe('the mock blackboard and live mode', () => {
  it('refuses live_start with the pages it holds when the sync may not run first', async () => {
    await board({});
    expect((await api.tabletStatus()).device?.storedPages).toBe(3);
    expect(refusal(() => mockLiveInvoke('live_start', {}))).toEqual({
      code: 'blackboardPagesFirst',
      message: 'the Xiaomi tablet holds 3 stored pages: import them before live mode',
    });
  });

  it.each([
    ['both off', { autoSync: false, deleteAfterImport: false }],
    ['automatic sync on, deletion off', { autoSync: true, deleteAfterImport: false }],
    ['automatic sync off, deletion on', { autoSync: false, deleteAfterImport: true }],
  ])('refuses with %s', async (_name, options) => {
    await board(options);
    expect(refusal(() => mockLiveInvoke('live_start', {})).code).toBe('blackboardPagesFirst');
  });

  it('refuses live_resume as well (a sheet of the blackboard\'s size), and after the sheet check', async () => {
    await board({});
    const path = sheetPath();
    // A sheet of the Slate cannot be written by the blackboard: that refusal comes first, as in Rust.
    expect(refusal(() => mockLiveInvoke('live_resume', { path })).code).toBe('sheetOtherTablet');
    mockLibrary().notes.get(path)!.note.page = { width: 20500, height: 27200 };
    expect(refusal(() => mockLiveInvoke('live_resume', { path })).code).toBe('blackboardPagesFirst');
  });

  it('does not refuse without pages, without the blackboard connected, or with the Slate', async () => {
    mockTablet.boardPages = 0;
    await board({});
    expect(mockLiveInvoke('live_start', {})).toBeTruthy();
    resetMockLive();
    resetMockTablet();
    mockTablet.inRange = false;
    // Known, not connected: nothing to refuse (a run then waits, and the connection decides).
    await board({}, { connect: false });
    expect((await api.tabletStatus()).active).toBeNull();
    expect(mockLiveInvoke('live_start', {})).toBeTruthy();
  });

  it('lets live mode start once the pages are imported with the confirmation', async () => {
    await board({});
    await api.tabletSync(true);
    await beat(3);
    const status = await api.tabletStatus();
    expect(status.device?.storedPages).toBe(0);
    expect(status.lastSync?.deleted).toBe(3);
    expect(mockLiveInvoke('live_start', {})).toBeTruthy();
  });

  it('syncs the pages first, then starts, when the automatic sync and the deletion are on', async () => {
    await board({ autoSync: true, deleteAfterImport: true });
    // Connected: the automatic sync took the three pages.
    await beat(3);
    expect((await api.tabletStatus()).device?.storedPages).toBe(0);
    // A page stored since: asked now, it is synced first.
    mockBoardDraw(1);
    expect((await api.tabletStatus()).device?.storedPages).toBe(1);
    expect(mockLiveInvoke('live_start', {})).toBeTruthy();
    expect((await api.tabletStatus()).connection).toBe('syncing');
    await beat(3);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.device?.storedPages).toBe(0);
  });

  it('ends a run that waited for the blackboard with the refused event when it connects holding pages', async () => {
    await board({}, { connect: false });
    const batches: LiveBatch[] = [];
    await api.liveSubscribe((batch) => batches.push(batch));
    mockLiveInvoke('live_start', {});
    batches.length = 0;
    // It switches on: forget and add it again so that the simulation connects to it (it was heard, not in range before).
    mockTablet.boardInRange = true;
    await api.tabletForgetBlackboard();
    await board({});
    const events = batches.flatMap((batch) => batch.events);
    expect(events).toContainEqual({
      type: 'refused',
      code: 'blackboardPagesFirst',
      message: 'the Xiaomi tablet holds 3 stored pages: import them before live mode',
    });
    const last = events.filter((event) => event.type === 'snapshot').at(-1);
    expect(last?.type === 'snapshot' && last.snapshot.state).toBe('idle');
  });
});
