// @vitest-environment jsdom
// The simulated Xiaomi blackboard of the mock (M18, docs/design/blackboard.md §5).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { catalogs } from './i18n.svelte';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockTablet, resetMockTablet } from './mock-tablet';
import type { TabletOptions, TabletStatus } from './tablet';

const OPTIONS: TabletOptions = { autoSync: true, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' };

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  // The Slate is out of range: the blackboard is the tablet that connects.
  mockTablet.inRange = false;
});

afterEach(() => {
  resetMockTablet();
  vi.useRealTimers();
});

const beat = (beats = 1) => vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));

async function failureOf(action: Promise<unknown>): Promise<{ code: string; message: string }> {
  try {
    await action;
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error('expected a failure');
}

/** Searches, waits for the tablet to be heard, adds it. */
async function addBoard(): Promise<TabletStatus> {
  await api.tabletFindBlackboards();
  await beat(1);
  const found = (await api.tabletStatus()).boardSearch!.found;
  return api.tabletAddBlackboard(found[0].address);
}

describe('mock blackboard: adding it', () => {
  it('knows only the Slate at first', async () => {
    const status = await api.tabletStatus();
    expect(status.known.map((k) => k.kind)).toEqual(['slate']);
    expect(status.active).toBeNull();
    expect(status.boardSearch).toBeNull();
  });

  it('hears a blackboard after a beat, with its signal, and keeps the search under way', async () => {
    await api.tabletFindBlackboards();
    let status = await api.tabletStatus();
    expect(status.connection).toBe('adding');
    expect(status.boardSearch?.found).toEqual([]);
    expect(status.boardSearch!.until).toBeGreaterThan(Date.now());
    await beat(1);
    status = await api.tabletStatus();
    expect(status.boardSearch?.found).toEqual([{ address: '00:00:5E:00:53:01', name: 'xiaomi.dev.ble', rssi: -52 }]);
    expect(status.connection).toBe('adding');
  });

  it('hears several when asked to', async () => {
    mockTablet.boardsHeard = 2;
    await api.tabletFindBlackboards();
    await beat(2);
    const found = (await api.tabletStatus()).boardSearch!.found;
    expect(found.map((f) => f.address)).toEqual(['00:00:5E:00:53:01', '00:00:5E:00:53:02']);
    expect(found[0].rssi!).toBeGreaterThan(found[1].rssi!);
  });

  it('adds the blackboard heard, without registration, then connects to it', async () => {
    const status = await addBoard();
    expect(status.connection).toBe('searching');
    expect(status.boardSearch).toBeNull();
    expect(status.known.map((k) => k.kind)).toEqual(['slate', 'blackboard']);
    const board = status.known[1];
    expect(board).toMatchObject({ name: 'Mijia LCD blackboard', width: 20500, height: 27200, model: 'Xiaomi Mijia LCD blackboard' });
    expect(status.registered?.name).toBe('Bamboo Slate');

    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await beat(1);
    expect(await api.tabletStatus()).toMatchObject({ connection: 'connecting', active: 'blackboard' });
    await beat(0.5);
    const connected = await api.tabletStatus();
    expect(connected.connection).toBe('connected');
    expect(connected.active).toBe('blackboard');
    expect(connected.device).toMatchObject({
      kind: 'blackboard',
      firmware: '',
      charging: false,
      batteryPercent: 90,
      storedPages: 3,
      model: 'Xiaomi Mijia LCD blackboard',
      width: 20500,
      height: 27200,
    });
    expect(connected.device?.memory?.total).toBeGreaterThan(connected.device!.memory!.free);
  });

  it('accepts the address in any case, and refuses one that was not heard', async () => {
    await api.tabletFindBlackboards();
    await beat(1);
    expect((await failureOf(api.tabletAddBlackboard('00:00:5E:00:53:99'))).code).toBe('blackboardNotFound');
    await expect(api.tabletAddBlackboard('00:00:5e:00:53:01')).resolves.toMatchObject({ connection: 'searching' });
    // And without a search under way.
    expect((await failureOf(api.tabletAddBlackboard('00:00:5E:00:53:01'))).code).toBe('blackboardNotFound');
  });

  it('ends a search that hears nothing with blackboardNotFound', async () => {
    mockTablet.boardsHeard = 0;
    await api.tabletFindBlackboards();
    await beat(59);
    expect((await api.tabletStatus()).connection).toBe('adding');
    await beat(1.5);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('searching');
    expect(status.boardSearch).toBeNull();
    expect(status.error?.code).toBe('blackboardNotFound');
  });

  it('can be cancelled, without an error', async () => {
    await api.tabletFindBlackboards();
    await beat(1);
    await api.tabletCancel();
    const status = await api.tabletStatus();
    expect(status.connection).toBe('searching');
    expect(status.boardSearch).toBeNull();
    expect(status.error).toBeNull();
    expect(status.known.map((k) => k.kind)).toEqual(['slate']);
  });

  it('is refused during a sync or live mode', async () => {
    mockTablet.inRange = true;
    await api.tabletSetOptions(OPTIONS);
    await beat(2.1);
    expect((await api.tabletStatus()).connection).toBe('syncing');
    expect((await failureOf(api.tabletFindBlackboards())).code).toBe('tabletBusy');
  });
});

describe('mock blackboard: syncing', () => {
  it('turns its three pages into dated portrait notes and deletes them from the tablet', async () => {
    const before = mockLibrary().notes.size;
    await addBoard();
    await beat(1.5 + 0.5 + 3);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.lastSync).toMatchObject({ deleted: 3, error: null });
    expect(status.lastSync?.saved).toHaveLength(3);
    expect(status.device?.storedPages).toBe(0);
    expect(status.device?.memory?.free).toBe(status.device?.memory?.total);

    const notes = [...mockLibrary().notes.values()].map((entry) => entry.note).slice(before);
    expect(notes).toHaveLength(3);
    for (const note of notes) {
      expect(note.page).toEqual({ width: 20500, height: 27200 });
      expect(note.timing).toBe('synthesized');
    }
    // Dated by the page, one day apart, the oldest first.
    const created = notes.map((n) => n.created);
    expect(created[0]).toBeLessThan(created[1]);
    expect(created[1] - created[0]).toBeGreaterThan(86_000_000);
  });

  it('counts the pages down one by one', async () => {
    await addBoard();
    await beat(1.5 + 0.5 + 0.7);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('syncing');
    expect(status.device?.storedPages).toBe(2);
  });

  it('does not read pages by itself with deletion off, and asks for the confirmation on request', async () => {
    await api.tabletSetOptions({ ...OPTIONS, deleteAfterImport: false });
    await addBoard();
    await beat(2.5);
    let status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.lastSync).toBeNull();
    expect(status.device?.storedPages).toBe(3);

    // Without the flag: refused, nothing happens.
    expect((await failureOf(api.tabletSync())).code).toBe('blackboardConfirmDelete');
    expect((await api.tabletStatus()).connection).toBe('connected');

    // With it: every page is imported and deleted.
    await api.tabletSync(true);
    await beat(3);
    status = await api.tabletStatus();
    expect(status.lastSync).toMatchObject({ deleted: 3 });
    expect(status.device?.storedPages).toBe(0);
  });

  it('asks for the confirmation from the moment the blackboard is being connected, with the option off', async () => {
    await api.tabletSetOptions({ ...OPTIONS, deleteAfterImport: false });
    await addBoard();
    await beat(1);
    expect(await api.tabletStatus()).toMatchObject({ connection: 'connecting', active: 'blackboard' });
    expect((await failureOf(api.tabletSync())).code).toBe('blackboardConfirmDelete');
    // Nothing was queued by the refusal.
    expect((await api.tabletStatus()).pending).toBeNull();
  });

  it('brings nothing, without an error, when a sync waited for the blackboard with deletion off', async () => {
    await api.tabletSetOptions({ ...OPTIONS, deleteAfterImport: false });
    await addBoard();
    await api.tabletSync();
    expect((await api.tabletStatus()).pending).toBe('sync');
    await beat(2);
    const status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.pending).toBeNull();
    expect(status.lastSync).toBeNull();
    expect(status.error).toBeNull();
    expect(status.device?.storedPages).toBe(3);
  });

  it('takes the page its button stores', async () => {
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await addBoard();
    await beat(2);
    await api.simBlackboardStorePage();
    expect((await api.tabletStatus()).device?.storedPages).toBe(4);
    await api.tabletSync();
    await beat(4);
    expect((await api.tabletStatus()).lastSync?.saved).toHaveLength(4);
  });
});

describe('mock blackboard: what it cannot do, forgetting it', () => {
  it('refuses to be renamed, and says 0 for its clock', async () => {
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await addBoard();
    await beat(2);
    expect((await failureOf(api.tabletRename('Autre'))).code).toBe('notSupported');
    expect(await api.tabletSetTime()).toBe(0);
  });

  it('is forgotten alone: the Slate stays known and is looked for again', async () => {
    await addBoard();
    await beat(2);
    await api.tabletForgetBlackboard();
    const status = await api.tabletStatus();
    expect(status.known.map((k) => k.kind)).toEqual(['slate']);
    expect(status.connection).toBe('searching');
    expect(status.active).toBeNull();
    expect(status.device).toBeNull();
    expect(status.registered?.name).toBe('Bamboo Slate');
  });

  it('is the only tablet left when the Slate is forgotten', async () => {
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await addBoard();
    await api.forgetTablet();
    let status = await api.tabletStatus();
    expect(status.registered).toBeNull();
    expect(status.known.map((k) => k.kind)).toEqual(['blackboard']);
    expect(status.connection).toBe('searching');
    await beat(2);
    status = await api.tabletStatus();
    expect(status.connection).toBe('connected');
    expect(status.active).toBe('blackboard');

    await api.tabletForgetBlackboard();
    status = await api.tabletStatus();
    expect(status.known).toEqual([]);
    expect(status.connection).toBe('notRegistered');
  });

  it('knows only the blackboard after the Slate was forgotten first', async () => {
    await api.forgetTablet();
    expect((await api.tabletStatus()).connection).toBe('notRegistered');
    const status = await addBoard();
    expect(status.registered).toBeNull();
    expect(status.known.map((k) => k.kind)).toEqual(['blackboard']);
    await beat(2);
    expect((await api.tabletStatus()).active).toBe('blackboard');
  });

  it('lets the Slate connect first when both are in range', async () => {
    mockTablet.inRange = true;
    await api.tabletSetOptions({ ...OPTIONS, autoSync: false });
    await addBoard();
    await beat(2);
    expect((await api.tabletStatus()).active).toBe('slate');
  });
});

describe('every error code of the second tablet has its text in both languages', () => {
  it.each([
    'blackboardNotFound',
    'transferIncomplete',
    'pageNotDeleted',
    'notSupported',
    'blackboardConfirmDelete',
    'pageUnreadable',
    'sheetOtherTablet',
  ])('%s', (code) => {
    for (const locale of ['fr', 'en'] as const) {
      const text = (catalogs[locale].errorCodes as Record<string, string>)[code];
      expect(text, `${locale} ${code}`).toBeTruthy();
    }
  });
});
