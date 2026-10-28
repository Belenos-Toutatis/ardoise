/**
 * Browser stand-in for the tablet commands (`tablet_*`, `sim_press_button`): a simulated tablet
 * that registers step by step, connects, holds three pages and syncs them into the demo library
 * (real notes, `notifyLibraryChanged`), reports its battery and takes the settings. Everything
 * moves on `setTimeout`s paced by `mockTablet.stepMs`, so tests drive it with fake timers.
 * Work package C (docs/design/tablet-live-classroom.md).
 *
 * Whether a tablet is registered follows `tablet_info` / `forget_tablet` of `mock-library.ts`
 * (the demo library knows a registered tablet). A registration made here after forgetting it is
 * kept in this module only: `tablet_info` keeps answering null in that case.
 */
import type { InkPoint } from './ink';
import { UNHANDLED, fileName, freshStamp, mockLibrary, notifyLibraryChanged } from './mock-data';
import { failure, freePath, mockLibraryInvoke } from './mock-library';
import type { Note, Stroke, TabletInfo } from './note';
import { setMockLiveOrientation } from './mock-live';
import { isOrientation, pageSizeFor, quarterTurns, turnStrokes, type Orientation } from './orientation';
import type { Connection, DeviceDetails, TabletEvent, TabletOptions, TabletStatus } from './tablet';

/** Knobs of the simulated tablet; tests change them. */
export const mockTablet = {
  /** One beat of the simulation, in milliseconds (a sync takes 0.6 beat per page). */
  stepMs: 1000,
  /**
   * The registered tablet comes into range by itself. Off under vitest, where the other
   * screens' tests must not see notes appear on their own; the tablet tests turn it on.
   */
  inRange: !import.meta.env.VITEST,
  /** The simulated tablet presses its button by itself when the wizard asks (as `--simulated-tablet`). */
  autoPress: true,
  /** Error code the next registration ends with, instead of succeeding. */
  failRegistration: null as string | null,
  /** Battery level reported. */
  battery: 87,
  /** Drift left after "set the clock", in seconds. */
  drift: 0.4,
};

const FIRMWARE = 'B160805A-C170704A';
const A4 = { width: 29700, height: 21600 };

const DEFAULT_OPTIONS: TabletOptions = { autoSync: true, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' };

interface Sim {
  status: TabletStatus;
  listeners: Set<(event: TabletEvent) => void>;
  timers: Set<ReturnType<typeof setTimeout>>;
  /** Tablet registered through the wizard while the demo library knows none. */
  local: TabletInfo | null;
  /** Numbers of the pages in the tablet's memory. */
  stored: number[];
  nextPage: number;
  /** Pages already written as notes (not written twice when kept on the tablet). */
  imported: Set<number>;
  /** The sync under way. */
  sync: { cancelled: boolean } | null;
  /** The wizard waits for the button. */
  pressWaiter: (() => void) | null;
  /** The registration under way. */
  registering: { cancelled: boolean } | null;
}

let sim: Sim | null = null;

/** Forgets the simulated tablet, its timers and its subscribers (tests). */
export function resetMockTablet(): void {
  if (sim) for (const timer of sim.timers) clearTimeout(timer);
  sim = null;
  setMockLiveOrientation('landscape');
  mockTablet.stepMs = 1000;
  mockTablet.autoPress = true;
  mockTablet.failRegistration = null;
  mockTablet.battery = 87;
  mockTablet.drift = 0.4;
}

function registeredInfo(): TabletInfo | null {
  return (mockLibraryInvoke('tablet_info', {}) as TabletInfo | null) ?? sim?.local ?? null;
}

function modelOf(width: number, height: number): string {
  if (width === 29700 && height === 21600) return 'Bamboo Slate A4';
  if (width === 21600 && height === 14800) return 'Bamboo Slate A5';
  return `Wacom (${width} × ${height})`;
}

function details(info: TabletInfo, storedPages: number): DeviceDetails {
  return {
    name: info.name,
    firmware: FIRMWARE,
    batteryPercent: mockTablet.battery,
    charging: false,
    width: info.width,
    height: info.height,
    storedPages,
    model: modelOf(info.width, info.height),
    readAt: Date.now(),
  };
}

function getSim(): Sim {
  if (sim) return sim;
  const registered = registeredInfo();
  const created: Sim = {
    status: {
      connection: registered ? 'searching' : 'notRegistered',
      registered: registered ? { ...registered } : null,
      device: null,
      registerStep: null,
      sync: null,
      lastSync: null,
      pending: null,
      error: null,
      simulated: true,
      options: { ...DEFAULT_OPTIONS },
    },
    listeners: new Set(),
    timers: new Set(),
    local: null,
    stored: [1, 2, 3],
    nextPage: 4,
    imported: new Set(),
    sync: null,
    pressWaiter: null,
    registering: null,
  };
  sim = created;
  if (registered) comeIntoRange(created);
  return created;
}

function later(s: Sim, beats: number, action: () => void): void {
  const timer = setTimeout(() => {
    s.timers.delete(timer);
    action();
  }, Math.round(mockTablet.stepMs * beats));
  s.timers.add(timer);
}

function clearTimers(s: Sim): void {
  for (const timer of s.timers) clearTimeout(timer);
  s.timers.clear();
}

/** Applies a change to the status and tells every subscriber. */
function update(s: Sim, change: Partial<TabletStatus>): void {
  s.status = { ...s.status, ...change };
  for (const listener of [...s.listeners]) listener({ type: 'status', status: structuredClone(s.status) });
}

/** Makes the status follow what `mock-library.ts` says about the registered tablet. */
function reconcile(s: Sim): void {
  const info = registeredInfo();
  if (!info && s.status.registered && s.status.connection !== 'registering') {
    clearTimers(s);
    s.sync = null;
    update(s, { connection: 'notRegistered', registered: null, device: null, sync: null, pending: null, lastSync: null });
  } else if (info && s.status.registered && info.name !== s.status.registered.name) {
    update(s, { registered: { ...info } });
  } else if (info && !s.status.registered && s.status.connection === 'notRegistered') {
    // The demo library registered a tablet again (a new library).
    update(s, { connection: 'searching', registered: { ...info } });
    comeIntoRange(s);
  }
}

// --- Connection ----------------------------------------------------------------------------

/** searching → connecting → connected, then the automatic sync. */
function comeIntoRange(s: Sim): void {
  if (!mockTablet.inRange) return;
  later(s, 1, () => {
    if (s.status.connection !== 'searching') return;
    update(s, { connection: 'connecting' });
    later(s, 0.5, () => {
      if (s.status.connection === 'connecting') connected(s);
    });
  });
}

function connected(s: Sim): void {
  const info = s.status.registered;
  if (!info) return;
  update(s, { connection: 'connected', device: details(info, s.stored.length), error: null });
  const wanted = s.status.pending === 'sync';
  if ((wanted || s.status.options.autoSync) && s.stored.length > 0) {
    later(s, 0.5, () => {
      if (s.status.connection === 'connected') startSync(s);
    });
  } else if (wanted) {
    update(s, { pending: null });
  }
}

// --- Pages and sync ------------------------------------------------------------------------

function line(points: [number, number][], pressure: number, t0: number): InkPoint[] {
  return points.map(([x, y], i) => ({ x, y, pressure, t: t0 + i * 5 }));
}

function stroke(points: InkPoint[], width = 60): Stroke {
  return { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width, layer: 0, points };
}

/** The three reference pages, repeated for later ones: shapes, handwriting, three pressures. */
function pageStrokes(page: number): Stroke[] {
  switch ((page - 1) % 3) {
    case 0: {
      const square: [number, number][] = [
        [3000, 3000],
        [11000, 3000],
        [11000, 11000],
        [3000, 11000],
        [3000, 3000],
      ];
      const circle: [number, number][] = Array.from({ length: 49 }, (_, i) => [
        20000 + 5000 * Math.cos((i * Math.PI) / 24),
        7000 + 5000 * Math.sin((i * Math.PI) / 24),
      ]);
      return [stroke(line(square, 0.6, 0)), stroke(line(circle, 0.6, 400))];
    }
    case 1:
      return [
        stroke(
          line(
            Array.from({ length: 80 }, (_, i) => [3000 + i * 300, 10000 + 1200 * Math.sin(i / 3)] as [number, number]),
            0.5,
            0,
          ),
        ),
      ];
    default:
      return [
        stroke(line([[3000, 5000], [26000, 5000]], 0.15, 0), 40),
        stroke(line([[3000, 10000], [26000, 10000]], 0.5, 600), 60),
        stroke(line([[3000, 15000], [26000, 15000]], 0.95, 1200), 90),
      ];
  }
}

/** The strokes of a tablet page turned to the way the tablet is held (as `pages::orient`). */
function orient(strokes: Stroke[], width: number, height: number, orientation: Orientation): Stroke[] {
  return turnStrokes(strokes, width, height, quarterTurns(orientation));
}

function writeNote(s: Sim, page: number): string {
  const info = s.status.registered ?? { ...A4 };
  const strokes = pageStrokes(page);
  const now = Date.now();
  const note: Note = {
    id: globalThis.crypto?.randomUUID?.() ?? `00000000-0000-4000-8000-${String(now).slice(-12).padStart(12, '0')}`,
    title: '',
    created: now,
    modified: now,
    page: pageSizeFor(s.status.options.orientation, info.width, info.height),
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: orient(strokes, info.width, info.height, s.status.options.orientation),
    timing: 'recorded',
    origin: { kind: 'tablet' },
  };
  const library = mockLibrary();
  const path = freePath(library, '', fileName(note));
  library.notes.set(path, { note, stamp: freshStamp(), binder: '' });
  notifyLibraryChanged();
  return path;
}

function startSync(s: Sim): void {
  const run = { cancelled: false };
  s.sync = run;
  // Like the real tablet: it hands over its oldest page only, the next one is reachable once
  // that one is deleted. Without deletion a sync therefore reads one page.
  const pages = s.status.options.deleteAfterImport ? [...s.stored] : s.stored.slice(0, 1);
  const saved: string[] = [];
  let deleted = 0;
  update(s, {
    connection: 'syncing',
    pending: null,
    error: null,
    sync: { current: Math.min(1, pages.length), total: pages.length, saved: 0 },
  });

  const finish = () => {
    s.sync = null;
    const info = s.status.registered;
    update(s, {
      connection: 'connected',
      sync: null,
      lastSync: { at: Date.now(), saved, deleted },
      device: info ? details(info, s.stored.length) : s.status.device,
    });
  };

  const next = (index: number) => {
    if (run.cancelled || index >= pages.length) {
      finish();
      return;
    }
    update(s, { sync: { current: index + 1, total: pages.length, saved: saved.length } });
    later(s, 0.6, () => {
      const page = pages[index];
      // A page kept on the tablet earlier is not written twice.
      if (!s.imported.has(page)) {
        saved.push(writeNote(s, page));
        s.imported.add(page);
      }
      if (s.status.options.deleteAfterImport) {
        s.stored = s.stored.filter((p) => p !== page);
        s.imported.delete(page);
        deleted += 1;
      }
      update(s, { sync: { current: index + 1, total: pages.length, saved: saved.length } });
      next(index + 1);
    });
  };
  next(0);
}

/** A page drawn on the simulated tablet (tests, `npm run dev`). */
export function mockTabletDraw(count = 1): void {
  const s = getSim();
  for (let i = 0; i < count; i++) s.stored.push(s.nextPage++);
  if (s.status.device) update(s, { device: { ...s.status.device, storedPages: s.stored.length } });
}

/** Changes the status as if the service had (tests: low battery, charging, an error…). */
export function mockTabletPatch(change: Partial<TabletStatus>): void {
  update(getSim(), change);
}

// --- Registration --------------------------------------------------------------------------

function startRegistration(s: Sim): void {
  const run = { cancelled: false };
  s.registering = run;
  s.sync = null;
  clearTimers(s);
  update(s, { connection: 'registering', registerStep: 'holdButton', error: null, sync: null, pending: null });
  const alive = () => !run.cancelled && s.registering === run;

  const fail = (code: string) => {
    s.registering = null;
    const registered = registeredInfo();
    update(s, {
      connection: registered ? 'searching' : 'notRegistered',
      registerStep: null,
      error: { code, message: '' },
    });
    if (registered) comeIntoRange(s);
  };

  later(s, 1, () => {
    if (!alive()) return;
    update(s, { registerStep: 'searching' });
    later(s, 1, () => {
      if (!alive()) return;
      if (mockTablet.failRegistration) return fail(mockTablet.failRegistration);
      update(s, { registerStep: 'connecting' });
      later(s, 1, () => {
        if (!alive()) return;
        update(s, { registerStep: 'pressButton' });
        const press = () => {
          s.pressWaiter = null;
          if (!alive()) return;
          const info: TabletInfo = {
            name: 'Bamboo Slate',
            address: 'AA:BB:CC:DD:EE:FF',
            registered: new Date().toISOString(),
            ...A4,
          };
          s.local = registeredInfo() ? null : info;
          const registered = registeredInfo() ?? info;
          s.registering = null;
          update(s, {
            registerStep: 'done',
            registered: { ...registered },
            device: details(registered, s.stored.length),
          });
          later(s, 1, () => {
            if (s.status.connection === 'registering') {
              update(s, { registerStep: null });
              connected(s);
            }
          });
        };
        s.pressWaiter = press;
        if (mockTablet.autoPress) later(s, 1, () => s.pressWaiter?.());
      });
    });
  });
}

function cancel(s: Sim): void {
  if (s.registering) {
    s.registering.cancelled = true;
    s.registering = null;
    s.pressWaiter = null;
    clearTimers(s);
    const registered = registeredInfo();
    update(s, { connection: registered ? 'searching' : 'notRegistered', registerStep: null, error: null });
    if (registered) comeIntoRange(s);
  } else if (s.sync) {
    // Between two pages: the page being written is finished.
    s.sync.cancelled = true;
  }
}

// --- Commands ------------------------------------------------------------------------------

function requireConnected(s: Sim): void {
  const c: Connection = s.status.connection;
  if (c === 'notRegistered') failure('tabletNotRegistered', '');
  if (c === 'syncing' || c === 'live' || c === 'registering') failure('tabletBusy', '');
  if (c !== 'connected') failure('tabletNotConnected', '');
}

function validName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed.length >= 1 && trimmed.length <= 15 && /^[\x20-\x7e]+$/.test(trimmed);
}

export function mockTabletInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'tablet_status': {
      const s = getSim();
      reconcile(s);
      return structuredClone(s.status);
    }

    case 'tablet_register': {
      const s = getSim();
      if (s.status.connection === 'registering' || s.status.connection === 'syncing' || s.status.connection === 'live') {
        failure('tabletBusy', '');
      }
      startRegistration(s);
      return undefined;
    }

    case 'tablet_cancel':
      cancel(getSim());
      return undefined;

    case 'tablet_sync': {
      const s = getSim();
      reconcile(s);
      const c = s.status.connection;
      if (c === 'notRegistered') failure('tabletNotRegistered', '');
      if (c === 'syncing' || c === 'live' || c === 'registering') failure('tabletBusy', '');
      if (c === 'connected') startSync(s);
      else update(s, { pending: 'sync' });
      return undefined;
    }

    case 'tablet_rename': {
      const s = getSim();
      reconcile(s);
      requireConnected(s);
      const name = String(args.name ?? '');
      if (!validName(name)) failure('invalidName', name);
      const trimmed = name.trim();
      const info = registeredInfo();
      if (info) info.name = trimmed;
      const registered = s.status.registered ? { ...s.status.registered, name: trimmed } : null;
      update(s, { registered, device: s.status.device ? { ...s.status.device, name: trimmed } : null });
      return structuredClone(s.status);
    }

    case 'tablet_set_time': {
      const s = getSim();
      reconcile(s);
      requireConnected(s);
      return mockTablet.drift;
    }

    case 'tablet_set_options': {
      const s = getSim();
      const options = args.options as TabletOptions;
      if (!isOrientation(options.orientation) || !['newPage', 'newLayer'].includes(options.button)) {
        failure('unexpected', 'invalid options');
      }
      update(s, { options: { ...options } });
      setMockLiveOrientation(options.orientation);
      return structuredClone(s.status);
    }

    case 'tablet_set_orientation': {
      const s = getSim();
      const orientation = args.orientation as Orientation;
      if (!isOrientation(orientation)) failure('unexpected', 'invalid orientation');
      update(s, { options: { ...s.status.options, orientation } });
      setMockLiveOrientation(orientation);
      return undefined;
    }

    case 'sim_press_button': {
      const s = getSim();
      if (!s.status.simulated) failure('simulationOnly', '');
      s.pressWaiter?.();
      return undefined;
    }

    default:
      return UNHANDLED;
  }
}

/** `tablet_subscribe` in the browser: the current status, then every change. */
export async function mockTabletSubscribe(onEvent: (event: TabletEvent) => void): Promise<TabletStatus> {
  const s = getSim();
  reconcile(s);
  s.listeners.add(onEvent);
  return structuredClone(s.status);
}
