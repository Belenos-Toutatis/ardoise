// @vitest-environment jsdom
// The tablet state with the Xiaomi blackboard (M18).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { setLocale } from './i18n.svelte';
import { resetMockLibrary } from './mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from './mock-tablet';
import type { TabletStatus } from './tablet';
import {
  TabletState,
  blackboardNeedsConfirmation,
  blackboardWaitsForRequest,
  formatBytes,
  tabletName,
} from './tablet-state.svelte';

let state: TabletState;

beforeEach(() => {
  vi.useFakeTimers();
  setLocale('fr');
  resetMockLibrary();
  resetMockTablet();
  mockTablet.inRange = false;
  state = new TabletState();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetMockTablet();
  vi.useRealTimers();
});

const beat = (beats = 1) => vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));

describe('tablet state: the two tablets', () => {
  it('names the tablets for the user', async () => {
    state.start();
    await beat(0);
    expect(state.known.map(tabletName)).toEqual(['Bamboo Slate']);
    expect(state.slate?.kind).toBe('slate');
    expect(state.board).toBeNull();
    expect(await state.findBlackboards()).toBe(true);
    await beat(1);
    expect(await state.addBlackboard(state.status!.boardSearch!.found[0].address)).toBe(true);
    expect(state.known.map(tabletName)).toEqual(['Bamboo Slate', 'Tablette Xiaomi']);
    setLocale('en');
    expect(tabletName(state.board!)).toBe('Xiaomi tablet');
  });

  it('forgets each kind with its own command', async () => {
    const forgetSlate = vi.spyOn(api, 'forgetTablet');
    const forgetBoard = vi.spyOn(api, 'tabletForgetBlackboard');
    state.start();
    await beat(0);
    await state.findBlackboards();
    await beat(1);
    await state.addBlackboard(state.status!.boardSearch!.found[0].address);
    expect(await state.forgetKind('blackboard')).toBe(true);
    expect(forgetBoard).toHaveBeenCalledOnce();
    expect(forgetSlate).not.toHaveBeenCalled();
    expect(state.known.map((k) => k.kind)).toEqual(['slate']);
    expect(await state.forgetKind('slate')).toBe(true);
    expect(forgetSlate).toHaveBeenCalledOnce();
    expect(state.known).toEqual([]);
  });

  it('keeps the translated message and the code of a refused addition', async () => {
    state.start();
    await beat(0);
    expect(await state.addBlackboard('00:00:5E:00:53:01')).toBe(false);
    expect(state.actionCode).toBe('blackboardNotFound');
    expect(state.error).toContain('Aucune tablette Xiaomi trouvée');
    state.dismissError();
    expect(state.actionCode).toBeNull();
    expect(state.error).toBeNull();
  });

  it('passes the confirmation to the sync only when it is given', async () => {
    const sync = vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await state.sync();
    await state.sync(true);
    expect(sync.mock.calls).toEqual([[false], [true]]);
  });
});

describe('the confirmation of a sync of the blackboard', () => {
  const status = (change: Partial<TabletStatus>, deleteAfterImport: boolean) =>
    ({ active: null, options: { deleteAfterImport }, ...change }) as TabletStatus;

  it('is needed only when the blackboard is connected and the option is off', () => {
    expect(blackboardNeedsConfirmation(null)).toBe(false);
    expect(blackboardNeedsConfirmation(status({ active: 'blackboard' }, false))).toBe(true);
    expect(blackboardNeedsConfirmation(status({ active: 'blackboard' }, true))).toBe(false);
    expect(blackboardNeedsConfirmation(status({ active: 'slate' }, false))).toBe(false);
    expect(blackboardNeedsConfirmation(status({ active: null }, false))).toBe(false);
  });
});

describe('sizes in bytes', () => {
  it('uses kilobytes below a megabyte and megabytes with a decimal above, in the language', () => {
    const clean = (text: string) => text.replace(/\s/g, ' ');
    expect(clean(formatBytes(900_000, 'fr'))).toBe('900 ko');
    // What the real tablet reports in total, and a plausible free value.
    expect(clean(formatBytes(126_909_952, 'fr'))).toBe('126,9 Mo');
    expect(clean(formatBytes(126_369_952, 'fr'))).toBe('126,4 Mo');
    expect(clean(formatBytes(126_909_952, 'en'))).toBe('126.9 MB');
    expect(clean(formatBytes(500, 'en'))).toBe('1 kB');
  });
});

describe('a sync that would wait for the Xiaomi', () => {
  const known = (...kinds: string[]) => kinds.map((kind) => ({ kind }));
  const status = (change: Partial<TabletStatus>, deleteAfterImport = false) =>
    ({ active: null, known: known('blackboard'), options: { deleteAfterImport }, ...change }) as unknown as TabletStatus;

  it('brings nothing only when the Xiaomi is the only tablet known, is not connected and the option is off', () => {
    expect(blackboardWaitsForRequest(null)).toBe(false);
    expect(blackboardWaitsForRequest(status({}))).toBe(true);
    expect(blackboardWaitsForRequest(status({ active: 'blackboard' }))).toBe(false);
    expect(blackboardWaitsForRequest(status({}, true))).toBe(false);
    expect(blackboardWaitsForRequest(status({ known: known('slate', 'blackboard') } as Partial<TabletStatus>))).toBe(false);
    expect(blackboardWaitsForRequest(status({ known: [] }))).toBe(false);
  });

  it('is told apart from a failure: the request for the confirmation is no error to show', async () => {
    vi.spyOn(api, 'tabletSync').mockRejectedValue({ code: 'blackboardConfirmDelete', message: '' });
    state.start();
    await beat(0);
    mockTabletPatch({ error: { code: 'blackboardConfirmDelete', message: '' } });
    expect(state.confirmDeleteAsked).toBe(true);
    expect(state.statusError).toBeNull();
    mockTabletPatch({ error: { code: 'tabletBusy', message: '' } });
    expect(state.confirmDeleteAsked).toBe(false);
    expect(state.statusError).toContain('occupée');
  });
});
