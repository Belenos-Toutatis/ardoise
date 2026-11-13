// @vitest-environment jsdom
// The tablet state with the Xiaomi blackboard (M18).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { setLocale } from './i18n.svelte';
import { resetMockLibrary } from './mock-data';
import { mockTablet, resetMockTablet } from './mock-tablet';
import type { TabletStatus } from './tablet';
import { TabletState, blackboardNeedsConfirmation, formatBytes, tabletName } from './tablet-state.svelte';

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
    expect(clean(formatBytes(7_460_000, 'fr'))).toBe('7,5 Mo');
    expect(clean(formatBytes(8_000_000, 'en'))).toBe('8 MB');
    expect(clean(formatBytes(500, 'en'))).toBe('1 kB');
  });
});
