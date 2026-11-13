// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { setLocale } from './i18n.svelte';
import { resetMockLibrary } from './mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from './mock-tablet';
import type { TabletEvent, TabletOptions, TabletStatus } from './tablet';
import { TabletState, isBusy, pagesOnTablet, sizeInMillimetres, validTabletName } from './tablet-state.svelte';

let state: TabletState;

beforeEach(() => {
  vi.useFakeTimers();
  setLocale('fr');
  resetMockLibrary();
  resetMockTablet();
  mockTablet.inRange = true;
  state = new TabletState();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetMockTablet();
  vi.useRealTimers();
});

const beat = (beats = 1) => vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));

/** Starts the shared subscription and waits for the first status. */
async function started() {
  state.start();
  await vi.advanceTimersByTimeAsync(0);
  expect(state.status).not.toBeNull();
}

describe('tablet name', () => {
  it('accepts 1 to 15 printable ASCII characters once trimmed', () => {
    expect(validTabletName('Bamboo Slate')).toBe(true);
    expect(validTabletName('  A  ')).toBe(true);
    expect(validTabletName('123456789012345')).toBe(true);
    expect(validTabletName('1234567890123456')).toBe(false);
    expect(validTabletName('')).toBe(false);
    expect(validTabletName('    ')).toBe(false);
    expect(validTabletName('Élève')).toBe(false);
    expect(validTabletName('a\tb')).toBe(false);
  });
});

describe('helpers', () => {
  it('writes the drawing area in millimetres', () => {
    expect(sizeInMillimetres(29700, 21600)).toBe('297 × 216 mm');
    expect(sizeInMillimetres(21600, 14800)).toBe('216 × 148 mm');
  });

  it('knows which connections are busy', () => {
    expect(isBusy('syncing')).toBe(true);
    expect(isBusy('live')).toBe(true);
    expect(isBusy('registering')).toBe(true);
    expect(isBusy('connected')).toBe(false);
    expect(isBusy('searching')).toBe(false);
  });

  it('counts the pages that wait on the connected tablet, and nothing in the other cases', () => {
    const status = (change: Partial<TabletStatus>, device: Partial<NonNullable<TabletStatus['device']>> | null = {}) =>
      ({
        connection: 'connected',
        active: 'slate',
        device: device === null ? null : { kind: 'slate', storedPages: 3, ...device },
        ...change,
      }) as unknown as TabletStatus;
    expect(pagesOnTablet(null)).toBe(0);
    expect(pagesOnTablet(status({}))).toBe(3);
    expect(pagesOnTablet(status({ connection: 'syncing' }))).toBe(3);
    // Not read yet, searching, live mode (the number is stale), or the last reading is of the other tablet.
    expect(pagesOnTablet(status({}, null))).toBe(0);
    expect(pagesOnTablet(status({ connection: 'searching', active: null }))).toBe(0);
    expect(pagesOnTablet(status({ connection: 'live' }))).toBe(0);
    expect(pagesOnTablet(status({ active: 'blackboard' }))).toBe(0);
    expect(pagesOnTablet(status({}, { storedPages: 0 }))).toBe(0);
  });
});

describe('tablet state: subscription', () => {
  it('subscribes once for the whole window', async () => {
    const subscribe = vi.spyOn(api, 'tabletSubscribe');
    state.start();
    state.start();
    await started();
    state.start();
    expect(subscribe).toHaveBeenCalledOnce();
  });

  it('shows the registered tablet, then follows every change', async () => {
    await started();
    expect(state.registered).toBe(true);
    expect(state.status?.connection).toBe('searching');
    await beat(1.5);
    expect(state.status?.connection).toBe('connected');
    expect(state.status?.device?.batteryPercent).toBe(87);
    await beat(1);
    expect(state.status?.connection).toBe('syncing');
    expect(state.status?.sync?.total).toBe(3);
    await beat(3);
    expect(state.status?.connection).toBe('connected');
    expect(state.status?.lastSync?.saved).toHaveLength(3);
  });

  it('keeps an event that came before the answer to the subscription', async () => {
    const newer = { connection: 'connected' } as TabletStatus;
    const older = { connection: 'searching' } as TabletStatus;
    vi.spyOn(api, 'tabletSubscribe').mockImplementation((onEvent: (event: TabletEvent) => void) => {
      onEvent({ type: 'status', status: newer });
      return Promise.resolve(older);
    });
    await started();
    expect(state.status?.connection).toBe('connected');
  });

  it('can subscribe again after a failure', async () => {
    const subscribe = vi.spyOn(api, 'tabletSubscribe').mockRejectedValueOnce({ code: 'tablet', message: 'no service' });
    state.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(state.unavailable).toBe(true);
    expect(state.status).toBeNull();
    state.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(subscribe).toHaveBeenCalledTimes(2);
    expect(state.unavailable).toBe(false);
    expect(state.status).not.toBeNull();
  });

  it('translates the service error and prefers the last action\'s', async () => {
    await started();
    mockTabletPatch({ error: { code: 'bluetoothOff', message: '' } });
    expect(state.statusError).toContain('Le Bluetooth est désactivé');
    expect(state.error).toBe(state.statusError);
    expect(await state.setTime()).toBe(false);
    expect(state.actionError).toContain("La tablette n'est pas connectée");
    expect(state.error).toBe(state.actionError);
    state.dismissError();
    expect(state.error).toBe(state.statusError);
    mockTabletPatch({ error: null });
    expect(state.error).toBeNull();
  });

  it('translates in the current language', async () => {
    await started();
    setLocale('en');
    expect(await state.setTime()).toBe(false);
    expect(state.actionError).toContain('The tablet is not connected');
  });
});

describe('tablet state: actions', () => {
  it('registers, syncs and cancels through the commands', async () => {
    await started();
    const register = vi.spyOn(api, 'tabletRegister');
    const sync = vi.spyOn(api, 'tabletSync');
    const cancel = vi.spyOn(api, 'tabletCancel');
    expect(await state.register()).toBe(true);
    expect(await state.cancel()).toBe(true);
    await beat(1.5);
    expect(await state.sync()).toBe(true);
    expect(register).toHaveBeenCalledOnce();
    expect(cancel).toHaveBeenCalledOnce();
    expect(sync).toHaveBeenCalledOnce();
  });

  it('reports a refused request', async () => {
    await started();
    await beat(1.5);
    await state.sync();
    expect(await state.sync()).toBe(false);
    expect(state.actionError).toContain('La tablette est occupée');
    // The next request that works clears it.
    expect(await state.cancel()).toBe(true);
    expect(state.actionError).toBeNull();
  });

  it('renames with a valid name only', async () => {
    await started();
    await beat(1.5);
    const rename = vi.spyOn(api, 'tabletRename');
    expect(await state.rename('É')).toBe(false);
    expect(await state.rename('   ')).toBe(false);
    expect(rename).not.toHaveBeenCalled();
    expect(await state.rename('  Classe 3B  ')).toBe(true);
    expect(rename).toHaveBeenCalledWith('Classe 3B');
    expect(state.status?.registered?.name).toBe('Classe 3B');
    expect(state.renamed).toBe(true);
    expect(state.working).toBe(false);
  });

  it('says why a rename failed', async () => {
    await started();
    expect(await state.rename('Autre')).toBe(false);
    expect(state.actionError).toContain("La tablette n'est pas connectée");
    expect(state.renamed).toBe(false);
  });

  it('sets the clock and keeps the drift', async () => {
    await started();
    await beat(1.5);
    expect(state.drift).toBeNull();
    mockTablet.drift = 1.5;
    expect(await state.setTime()).toBe(true);
    expect(state.drift).toBe(1.5);
  });

  it('changes an option at once, and saves it', async () => {
    await started();
    const save = vi.spyOn(api, 'tabletSetOptions');
    const done = state.setOptions({ deleteAfterImport: false });
    // Shown before the service answered.
    expect(state.status?.options.deleteAfterImport).toBe(false);
    expect(await done).toBe(true);
    expect(save).toHaveBeenCalledWith({ autoSync: true, deleteAfterImport: false, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' });
    expect((await api.tabletStatus()).options.deleteAfterImport).toBe(false);
  });

  it('puts an option back when the service refuses it', async () => {
    await started();
    vi.spyOn(api, 'tabletSetOptions').mockRejectedValue({ code: 'tablet', message: 'disk full' });
    const done = state.setOptions({ orientation: 'landscapeFlipped' });
    expect(state.status?.options.orientation).toBe('landscapeFlipped');
    expect(await done).toBe(false);
    expect(state.status?.options.orientation).toBe('landscape');
    expect(state.actionError).toContain('Problème avec la tablette : disk full');
  });

  it('saves the position of the blackboard with the other options, and keeps that of the Slate', async () => {
    await started();
    const save = vi.spyOn(api, 'tabletSetOptions');
    expect(await state.setOptions({ boardPosition: 'buttonsLeft' })).toBe(true);
    expect(save).toHaveBeenCalledWith({
      autoSync: true,
      deleteAfterImport: true,
      orientation: 'landscape',
      boardPosition: 'buttonsLeft',
      button: 'newPage',
    });
    expect((await api.tabletStatus()).options).toMatchObject({ boardPosition: 'buttonsLeft', orientation: 'landscape' });
  });

  it('keeps every other option when one changes', async () => {
    await started();
    const odd: TabletOptions = { autoSync: false, deleteAfterImport: true, orientation: 'landscapeFlipped', boardPosition: 'buttonsBottom', button: 'newLayer' };
    await state.setOptions(odd);
    await state.setOptions({ autoSync: true });
    expect(state.status?.options).toEqual({ ...odd, autoSync: true });
  });

  it('forgets the tablet and reads the new status', async () => {
    await started();
    expect(await state.forget()).toBe(true);
    expect(state.registered).toBe(false);
    expect(state.status?.connection).toBe('notRegistered');
    expect(await api.tabletInfo()).toBeNull();
  });

  it('presses the simulated tablet\'s button', async () => {
    await started();
    const press = vi.spyOn(api, 'simPressButton');
    expect(await state.pressButton()).toBe(true);
    expect(press).toHaveBeenCalledOnce();
  });

  it('starts over after a reset', async () => {
    await started();
    state.reset();
    expect(state.status).toBeNull();
    await started();
  });
});
