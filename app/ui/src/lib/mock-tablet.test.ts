// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { mockLibrary, mockOnLibraryChanged, resetMockLibrary } from './mock-data';
import { mockTablet, mockTabletDraw, mockTabletPatch, resetMockTablet } from './mock-tablet';
import type { Orientation } from './orientation';
import type { TabletOptions, TabletStatus } from './tablet';

const OPTIONS: TabletOptions = { autoSync: true, deleteAfterImport: true, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' };

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  mockTablet.inRange = true;
});

afterEach(() => {
  resetMockTablet();
  vi.useRealTimers();
});

/** Lets `beats` beats of the simulation go by. */
const beat = (beats = 1) => vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));

async function subscribe() {
  const events: TabletStatus[] = [];
  const first = await api.tabletSubscribe((event) => events.push(event.status));
  return { first, events };
}

async function failureOf(action: Promise<unknown>): Promise<{ code: string; message: string }> {
  try {
    await action;
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error('expected a failure');
}

/** Connects the registered tablet without letting it sync by itself. */
async function connectedWithoutSync(change: Partial<TabletOptions> = {}) {
  await api.tabletSetOptions({ ...OPTIONS, autoSync: false, ...change });
  await beat(1.5);
  expect((await api.tabletStatus()).connection).toBe('connected');
}

describe('mock tablet: connection', () => {
  it('knows the registered demo tablet, searching for it at first', async () => {
    const { first } = await subscribe();
    expect(first.connection).toBe('searching');
    expect(first.registered?.name).toBe('Bamboo Slate');
    expect(first.device).toBeNull();
    expect(first.simulated).toBe(true);
    expect(first.options).toEqual(OPTIONS);
  });

  it('finds the tablet, connects and reads its information', async () => {
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    const { events } = await subscribe();
    await beat(1);
    expect((await api.tabletStatus()).connection).toBe('connecting');
    await beat(0.5);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.device).toMatchObject({
      name: 'Bamboo Slate',
      batteryPercent: 87,
      charging: false,
      storedPages: 3,
      model: 'Bamboo Slate A4',
      width: 29700,
      height: 21600,
    });
    expect(status.device?.firmware).toBeTruthy();
    // Every change was announced.
    expect(events.map((e) => e.connection)).toEqual(['connecting', 'connected']);
  });

  it('stays in the dark when out of range', async () => {
    mockTablet.inRange = false;
    await subscribe();
    await beat(10);
    expect((await api.tabletStatus()).connection).toBe('searching');
  });

  it('hands out copies of its status', async () => {
    const { first } = await subscribe();
    first.options.orientation = 'landscapeFlipped';
    expect((await api.tabletStatus()).options.orientation).toBe('landscape');
  });

  it('can be patched by a test (low battery, charging)', async () => {
    await connectedWithoutSync();
    const { events } = await subscribe();
    const device = (await api.tabletStatus()).device!;
    mockTabletPatch({ device: { ...device, batteryPercent: 9, charging: true } });
    expect(events.at(-1)?.device).toMatchObject({ batteryPercent: 9, charging: true });
  });
});

describe('mock tablet: sync', () => {
  it('syncs the three pages when connecting, as real notes in the library', async () => {
    const changed = vi.fn();
    const stop = mockOnLibraryChanged(changed);
    const { events } = await subscribe();
    await beat(3.2);
    const syncing = (await api.tabletStatus()).sync;
    expect(syncing?.total).toBe(3);

    await beat(3);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.sync).toBeNull();
    expect(status.lastSync?.saved).toHaveLength(3);
    expect(status.lastSync?.deleted).toBe(3);
    expect(status.device?.storedPages).toBe(0);

    // Progress was announced page by page.
    const currents = events.filter((e) => e.connection === 'syncing').map((e) => e.sync?.current);
    expect(new Set(currents)).toEqual(new Set([1, 2, 3]));
    expect(changed).toHaveBeenCalledTimes(3);

    // The notes are in the library: 3 demo notes + 3 pages, the new ones from the tablet.
    const library = mockLibrary();
    expect(library.notes.size).toBe(6);
    for (const path of status.lastSync!.saved) {
      const opened = await api.openNote(path);
      expect(opened.note.origin).toEqual({ kind: 'tablet' });
      expect(opened.note.title).toBe('');
      expect(opened.note.strokes.length).toBeGreaterThan(0);
      expect(opened.note.page).toEqual({ width: 29700, height: 21600 });
    }
    stop();
  });

  it('syncs on demand, and tells a second request that it is busy', async () => {
    await connectedWithoutSync();
    await api.tabletSync();
    expect((await api.tabletStatus()).connection).toBe('syncing');
    expect((await failureOf(api.tabletSync())).code).toBe('tabletBusy');
    await beat(2);
    expect((await api.tabletStatus()).lastSync?.saved).toHaveLength(3);
  });

  it('waits for the tablet when asked to sync before it is connected', async () => {
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await subscribe();
    await api.tabletSync();
    expect((await api.tabletStatus()).pending).toBe('sync');
    await beat(1.5);
    // Connected, the sync about to start.
    expect(await api.tabletStatus()).toMatchObject({ connection: 'connected', pending: 'sync' });
    await beat(0.5);
    expect(await api.tabletStatus()).toMatchObject({ connection: 'syncing', pending: null });
    await beat(2);
    expect((await api.tabletStatus()).lastSync?.saved).toHaveLength(3);
  });

  it('stops between two pages when cancelled; the pages already saved stay saved and deleted', async () => {
    await connectedWithoutSync();
    await api.tabletSync();
    await beat(0.7);
    await api.tabletCancel();
    await beat(3);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.lastSync?.saved).toHaveLength(2);
    expect(status.lastSync?.deleted).toBe(2);
    expect(status.device?.storedPages).toBe(1);
    expect(mockLibrary().notes.size).toBe(5);
  });

  it('reads only the oldest page when deleting is off, and does not import it twice', async () => {
    // The tablet hands over its oldest page only; the next one needs that one to be deleted.
    await connectedWithoutSync({ deleteAfterImport: false });
    await api.tabletSync();
    await beat(3);
    let status = await api.tabletStatus();
    expect(status.lastSync).toMatchObject({ deleted: 0 });
    expect(status.lastSync?.saved).toHaveLength(1);
    expect(status.device?.storedPages).toBe(3);
    expect(mockLibrary().notes.size).toBe(4);

    await api.tabletSync();
    await beat(3);
    status = await api.tabletStatus();
    expect(status.lastSync?.saved).toHaveLength(0);
    expect(status.device?.storedPages).toBe(3);
    expect(mockLibrary().notes.size).toBe(4);

    // Turning deletion on empties the tablet, without a second copy of the first page.
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false, deleteAfterImport: true });
    await api.tabletSync();
    await beat(3);
    status = await api.tabletStatus();
    expect(status.lastSync).toMatchObject({ deleted: 3 });
    expect(status.lastSync?.saved).toHaveLength(2);
    expect(status.device?.storedPages).toBe(0);
    expect(mockLibrary().notes.size).toBe(6);
  });

  it('syncs pages drawn later', async () => {
    await connectedWithoutSync();
    mockTabletDraw(2);
    expect((await api.tabletStatus()).device?.storedPages).toBe(5);
    await api.tabletSync();
    await beat(4);
    expect((await api.tabletStatus()).lastSync?.saved).toHaveLength(5);
    expect(mockLibrary().notes.size).toBe(8);
  });

  it('turns the strokes half a turn for a left-handed user', async () => {
    await connectedWithoutSync({ orientation: 'landscapeFlipped' });
    await api.tabletSync();
    await beat(3);
    const paths = (await api.tabletStatus()).lastSync!.saved;
    const first = (await api.openNote(paths[0])).note.strokes[0].points[0];
    // The square of reference page 1 starts at (3000, 3000).
    expect(first.x).toBe(29700 - 3000);
    expect(first.y).toBe(21600 - 3000);
  });
});

describe('mock tablet: orientation', () => {
  const A4_W = 29700;
  const A4_H = 21600;

  async function firstStroke(orientation: Orientation) {
    await connectedWithoutSync({ orientation });
    await api.tabletSync();
    await beat(3);
    const paths = (await api.tabletStatus()).lastSync!.saved;
    const { note } = await api.openNote(paths[0]);
    return { note, first: note.strokes[0].points[0] };
  }

  it('keeps a landscape page for the two landscape positions and swaps width and height in portrait', async () => {
    expect((await firstStroke('landscape')).note.page).toEqual({ width: A4_W, height: A4_H });
    resetMockLibrary();
    resetMockTablet();
    expect((await firstStroke('portraitLeft')).note.page).toEqual({ width: A4_H, height: A4_W });
  });

  it('turns the corner of reference page 1 a quarter turn the right way in portrait', async () => {
    // The square of reference page 1 starts at (3000, 3000) of the tablet.
    const left = await firstStroke('portraitLeft');
    expect(left.first.x).toBe(A4_H - 3000);
    expect(left.first.y).toBe(3000);
    resetMockLibrary();
    resetMockTablet();
    const right = await firstStroke('portraitRight');
    expect(right.first.x).toBe(3000);
    expect(right.first.y).toBe(A4_W - 3000);
  });

  it('refuses a position it does not know', async () => {
    await connectedWithoutSync();
    expect((await failureOf(api.tabletSetOrientation('upsideDown' as Orientation))).code).toBe('unexpected');
  });

  it('switches the position without touching the other options', async () => {
    await connectedWithoutSync({ deleteAfterImport: false });
    await api.tabletSetOrientation('portraitRight');
    const options = (await api.tabletStatus()).options;
    expect(options.orientation).toBe('portraitRight');
    expect(options.deleteAfterImport).toBe(false);
  });
});

describe('mock tablet: registration', () => {
  async function startFromScratch() {
    await api.forgetTablet();
    const { events } = await subscribe();
    expect((await api.tabletStatus()).connection).toBe('notRegistered');
    return events;
  }

  it('goes through the wizard steps and ends connected, then syncs by itself', async () => {
    const events = await startFromScratch();
    await api.tabletRegister();
    const steps = () => events.map((e) => e.registerStep).filter((step, i, all) => step !== all[i - 1]);

    expect((await api.tabletStatus()).registerStep).toBe('holdButton');
    await beat(1);
    expect((await api.tabletStatus()).registerStep).toBe('searching');
    await beat(1);
    expect((await api.tabletStatus()).registerStep).toBe('connecting');
    await beat(1);
    expect((await api.tabletStatus()).registerStep).toBe('pressButton');
    // The simulated tablet presses its button by itself.
    await beat(1);
    let status = await api.tabletStatus();
    expect(status.registerStep).toBe('done');
    expect(status.registered?.name).toBe('Bamboo Slate');
    expect(status.device?.storedPages).toBe(3);

    await beat(1);
    status = await api.tabletStatus();
    expect(status.registerStep).toBeNull();
    expect(status.connection).toBe('connected');
    expect(steps()).toEqual(['holdButton', 'searching', 'connecting', 'pressButton', 'done', null]);

    await beat(0.5 + 3);
    expect((await api.tabletStatus()).lastSync?.saved).toHaveLength(3);
  });

  it('takes the button press from the test command', async () => {
    mockTablet.autoPress = false;
    await startFromScratch();
    await api.tabletRegister();
    await beat(10);
    expect((await api.tabletStatus()).registerStep).toBe('pressButton');
    await api.simPressButton();
    expect((await api.tabletStatus()).registerStep).toBe('done');
  });

  it('can be cancelled at any step, and then does nothing more', async () => {
    await startFromScratch();
    await api.tabletRegister();
    await beat(1.5);
    await api.tabletCancel();
    const status = await api.tabletStatus();
    expect(status.connection).toBe('notRegistered');
    expect(status.registerStep).toBeNull();
    await beat(10);
    expect(await api.tabletStatus()).toEqual(status);
  });

  it('ends with an error when the tablet is not found', async () => {
    mockTablet.failRegistration = 'tabletTimeout';
    await startFromScratch();
    await api.tabletRegister();
    await beat(2.5);
    const status = await api.tabletStatus();
    expect(status.registerStep).toBeNull();
    expect(status.connection).toBe('notRegistered');
    expect(status.error?.code).toBe('tabletTimeout');
    // Trying again works.
    mockTablet.failRegistration = null;
    await api.tabletRegister();
    expect((await api.tabletStatus()).error).toBeNull();
    await beat(5);
    expect((await api.tabletStatus()).registered).not.toBeNull();
  });

  it('refuses to register while it is busy', async () => {
    await startFromScratch();
    await api.tabletRegister();
    expect((await failureOf(api.tabletRegister())).code).toBe('tabletBusy');
  });

  it('forgets the tablet', async () => {
    const { events } = await subscribe();
    await beat(0.5);
    await api.forgetTablet();
    const status = await api.tabletStatus();
    expect(status).toMatchObject({ connection: 'notRegistered', registered: null, device: null });
    expect(events.at(-1)?.connection).toBe('notRegistered');
    // And it no longer comes by itself.
    await beat(10);
    expect((await api.tabletStatus()).connection).toBe('notRegistered');
    expect((await failureOf(api.tabletSync())).code).toBe('tabletNotRegistered');
  });
});

describe('mock tablet: settings', () => {
  it('renames the tablet, in its status and in the registration', async () => {
    await connectedWithoutSync();
    const status = await api.tabletRename('  Classe 3B ');
    expect(status.registered?.name).toBe('Classe 3B');
    expect(status.device?.name).toBe('Classe 3B');
    expect((await api.tabletInfo())?.name).toBe('Classe 3B');
  });

  it('refuses names the tablet would refuse', async () => {
    await connectedWithoutSync();
    for (const bad of ['', '   ', 'Un nom beaucoup trop long', 'Éléphant']) {
      expect((await failureOf(api.tabletRename(bad))).code, bad).toBe('invalidName');
    }
    expect((await api.tabletStatus()).registered?.name).toBe('Bamboo Slate');
    // Fifteen characters are fine.
    expect((await api.tabletRename('123456789012345')).registered?.name).toBe('123456789012345');
  });

  it('needs a connected tablet to rename it or set its clock', async () => {
    await subscribe();
    expect((await failureOf(api.tabletRename('Autre'))).code).toBe('tabletNotConnected');
    expect((await failureOf(api.tabletSetTime())).code).toBe('tabletNotConnected');
  });

  it('sets the clock and reports the drift left', async () => {
    await connectedWithoutSync();
    mockTablet.drift = 0.8;
    expect(await api.tabletSetTime()).toBe(0.8);
  });

  it('keeps the options it is given and refuses odd ones', async () => {
    const options: TabletOptions = { autoSync: false, deleteAfterImport: false, orientation: 'landscapeFlipped', boardPosition: 'buttonsBottom', button: 'newLayer' };
    const status = await api.tabletSetOptions(options);
    expect(status.options).toEqual(options);
    expect((await api.tabletStatus()).options).toEqual(options);
    const odd = { ...options, orientation: 'both' } as unknown as TabletOptions;
    expect((await failureOf(api.tabletSetOptions(odd))).code).toBe('unexpected');
  });

  it('keeps the position of the blackboard apart from that of the Slate, upright by default, and refuses a wrong one', async () => {
    expect((await api.tabletStatus()).options.boardPosition).toBe('buttonsBottom');
    const options: TabletOptions = { ...OPTIONS, orientation: 'portraitLeft', boardPosition: 'buttonsRight' };
    const status = await api.tabletSetOptions(options);
    expect(status.options).toMatchObject({ orientation: 'portraitLeft', boardPosition: 'buttonsRight' });
    // The Slate's names are not the blackboard's, and a missing field is refused like any other odd option.
    for (const odd of ['landscape', 'portraitLeft', undefined]) {
      const refused = { ...options, boardPosition: odd } as unknown as TabletOptions;
      expect((await failureOf(api.tabletSetOptions(refused))).code, String(odd)).toBe('unexpected');
    }
    expect((await api.tabletStatus()).options.boardPosition).toBe('buttonsRight');
  });

  it('presses its button only for a simulation', async () => {
    await subscribe();
    await expect(api.simPressButton()).resolves.toBeUndefined();
    mockTabletPatch({ simulated: false });
    expect((await failureOf(api.simPressButton())).code).toBe('simulationOnly');
  });
});
