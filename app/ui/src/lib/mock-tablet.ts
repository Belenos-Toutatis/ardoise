/**
 * Browser stand-in for the tablet commands (`tablet_*`, `sim_press_button`): a simulated tablet
 * that registers step by step, connects, holds three pages and syncs them into the demo library
 * (real notes, `notifyLibraryChanged`), reports its battery and takes the settings. Everything
 * moves on `setTimeout`s paced by `mockTablet.stepMs`, so tests drive it with fake timers.
 * Work package C (docs/design/tablet-live-classroom.md).
 *
 * The second tablet (M18, docs/design/blackboard.md §5): a simulated Xiaomi blackboard that the
 * search hears after a beat, that is added without registration, connects, syncs its pages (always
 * deleting each one), reports battery and memory, and is forgotten on its own. The Slate goes
 * first when both are in range, as in the service.
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
import type {
  Connection,
  DeviceDetails,
  KnownTablet,
  TabletEvent,
  TabletKind,
  TabletOptions,
  TabletStatus,
} from './tablet';

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
  /** The simulated blackboard is switched on and in range (as with `--simulated-blackboard`). */
  boardInRange: true,
  /** Blackboards the search hears: 0 ends it with `blackboardNotFound`, 2 gives a choice. */
  boardsHeard: 1,
  /** Battery level the blackboard reports. */
  boardBattery: 90,
  /** Beats until a search gives up (60 s in the service). */
  boardSearchBeats: 60,
};

const FIRMWARE = 'B160805A-C170704A';
const A4 = { width: 29700, height: 21600 };
/** The blackboard's page: about 20 × 27 cm, portrait (docs/design/blackboard.md §1). */
const BOARD_PAGE = { width: 20500, height: 27200 };
const BOARD_NAME = 'Mijia LCD blackboard';
const BOARD_MODEL = 'Xiaomi Mijia LCD blackboard';
/** Memory of the simulated blackboard, in bytes, and what one page takes. */
const BOARD_MEMORY = 8_000_000;
const BOARD_PAGE_BYTES = 180_000;
const DAY_MS = 86_400_000;
/** Connections in which the status names the tablet it is about (`active`). */
const LINKED: ReadonlySet<Connection> = new Set<Connection>(['connecting', 'connected', 'syncing', 'live']);

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
  /** The blackboard, once added through the search (the Slate is the library's). */
  board: KnownTablet | null;
  /** Numbers of the pages in the blackboard's memory (it holds some before it is added). */
  boardStored: number[];
  boardNextPage: number;
  /** The search for a blackboard under way. */
  search: { cancelled: boolean } | null;
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
  mockTablet.boardInRange = true;
  mockTablet.boardsHeard = 1;
  mockTablet.boardBattery = 90;
  mockTablet.boardSearchBeats = 60;
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
    kind: 'slate',
    memory: null,
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

function boardDetails(s: Sim): DeviceDetails {
  const stored = s.boardStored.length;
  return {
    kind: 'blackboard',
    name: s.board?.name ?? BOARD_NAME,
    firmware: '',
    batteryPercent: mockTablet.boardBattery,
    charging: false,
    width: BOARD_PAGE.width,
    height: BOARD_PAGE.height,
    storedPages: stored,
    model: BOARD_MODEL,
    readAt: Date.now(),
    memory: { total: BOARD_MEMORY, free: BOARD_MEMORY - stored * BOARD_PAGE_BYTES },
  };
}

/** What a tablet says about itself, for the status. */
function deviceOf(s: Sim, kind: TabletKind): DeviceDetails | null {
  if (kind === 'blackboard') return boardDetails(s);
  const info = s.status.registered;
  return info ? details(info, s.stored.length) : null;
}

/** Every known tablet, the Slate first. */
function knownOf(s: Sim): KnownTablet[] {
  const list: KnownTablet[] = [];
  const slate = s.status.registered;
  if (slate) {
    list.push({
      kind: 'slate',
      name: slate.name,
      address: slate.address,
      added: slate.registered,
      width: slate.width,
      height: slate.height,
      model: modelOf(slate.width, slate.height),
    });
  }
  if (s.board) list.push({ ...s.board });
  return list;
}

function anyKnown(s: Sim): boolean {
  return s.status.registered !== null || s.board !== null;
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
      active: null,
      known: [],
      boardSearch: null,
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
    board: null,
    boardStored: [1, 2, 3],
    boardNextPage: 4,
    search: null,
  };
  created.status.known = knownOf(created);
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
  if (!LINKED.has(s.status.connection)) s.status.active = null;
  s.status.known = knownOf(s);
  for (const listener of [...s.listeners]) listener({ type: 'status', status: structuredClone(s.status) });
}

/** A tablet was forgotten: ends what depended on it and looks for the one that is left, if any. */
function afterForget(s: Sim, kind: TabletKind): void {
  const wasActive = s.status.active === kind;
  const device = s.status.device?.kind === kind ? null : s.status.device;
  if (!anyKnown(s) && s.status.connection !== 'adding') {
    clearTimers(s);
    s.sync = null;
    update(s, { connection: 'notRegistered', device: null, sync: null, pending: null, lastSync: null });
  } else if (wasActive) {
    clearTimers(s);
    s.sync = null;
    update(s, { connection: 'searching', device, sync: null, pending: null });
    comeIntoRange(s);
  } else {
    update(s, { device });
  }
}

/** Makes the status follow what `mock-library.ts` says about the registered tablet. */
function reconcile(s: Sim): void {
  const info = registeredInfo();
  if (!info && s.status.registered && s.status.connection !== 'registering') {
    s.status = { ...s.status, registered: null };
    afterForget(s, 'slate');
  } else if (info && s.status.registered && info.name !== s.status.registered.name) {
    update(s, { registered: { ...info } });
  } else if (info && !s.status.registered && s.status.connection === 'notRegistered') {
    // The demo library registered a tablet again (a new library).
    update(s, { connection: 'searching', registered: { ...info } });
    comeIntoRange(s);
  } else if (info && !s.status.registered && s.status.connection !== 'registering') {
    update(s, { registered: { ...info } });
  }
}

// --- Connection ----------------------------------------------------------------------------

/** The tablet that is advertising: the Slate first, as in the service. */
function nextInRange(s: Sim): TabletKind | null {
  if (s.status.registered && mockTablet.inRange) return 'slate';
  if (s.board && mockTablet.boardInRange) return 'blackboard';
  return null;
}

/** searching → connecting → connected, then the automatic sync. */
function comeIntoRange(s: Sim): void {
  if (!nextInRange(s)) return;
  later(s, 1, () => {
    if (s.status.connection !== 'searching') return;
    const kind = nextInRange(s);
    if (!kind) return;
    update(s, { connection: 'connecting', active: kind });
    later(s, 0.5, () => {
      if (s.status.connection === 'connecting') connected(s);
    });
  });
}

function connected(s: Sim): void {
  const kind: TabletKind = s.status.active ?? 'slate';
  const device = deviceOf(s, kind);
  if (!device) return;
  update(s, { connection: 'connected', active: kind, device, error: null });
  const wanted = s.status.pending === 'sync';
  // The blackboard is read without being asked only with deletion on (it always deletes what it reads): with the
  // option off its pages wait for a sync confirmed by the user, and a sync that waited for it brings nothing.
  const mayRun = kind === 'slate' || s.status.options.deleteAfterImport;
  const automatic = s.status.options.autoSync && mayRun;
  const waiting = (kind === 'blackboard' ? s.boardStored : s.stored).length;
  if (mayRun && (wanted || automatic) && waiting > 0) {
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

/** A blackboard page: the same drawings, fitted to its portrait page (never turned). */
function boardStrokes(page: number): Stroke[] {
  return pageStrokes(page).map((s) => ({
    ...s,
    points: s.points.map((p) => ({ ...p, x: p.x * 0.7, y: p.y * 1.2 })),
  }));
}

/** The strokes of a tablet page turned to the way the tablet is held (as `pages::orient`). */
function orient(strokes: Stroke[], width: number, height: number, orientation: Orientation): Stroke[] {
  return turnStrokes(strokes, width, height, quarterTurns(orientation));
}

/** `age`: how long ago the blackboard stored the page, in days (its notes are dated by the page). */
function writeNote(s: Sim, page: number, kind: TabletKind, age = 0): string {
  const info = s.status.registered ?? { ...A4 };
  const board = kind === 'blackboard';
  const now = Date.now();
  const created = now - age * DAY_MS;
  const note: Note = {
    id: globalThis.crypto?.randomUUID?.() ?? `00000000-0000-4000-8000-${String(now).slice(-12).padStart(12, '0')}`,
    title: '',
    created,
    modified: created,
    page: board ? { ...BOARD_PAGE } : pageSizeFor(s.status.options.orientation, info.width, info.height),
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: board
      ? boardStrokes(page)
      : orient(pageStrokes(page), info.width, info.height, s.status.options.orientation),
    timing: board ? 'synthesized' : 'recorded',
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
  const kind: TabletKind = s.status.active ?? 'slate';
  const board = kind === 'blackboard';
  // Like the real tablet: it hands over its oldest page only, the next one is reachable once
  // that one is deleted. Without deletion a sync of the Slate therefore reads one page; the
  // blackboard's sync always deletes (it is the only way to its next page).
  const deletes = board || s.status.options.deleteAfterImport;
  const waiting = board ? s.boardStored : s.stored;
  const pages = deletes ? [...waiting] : waiting.slice(0, 1);
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
    update(s, {
      connection: 'connected',
      sync: null,
      lastSync: { at: Date.now(), saved, deleted, error: null },
      device: deviceOf(s, kind) ?? s.status.device,
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
      // A page kept on the Slate earlier is not written twice.
      if (board || !s.imported.has(page)) {
        saved.push(writeNote(s, page, kind, board ? pages.length - index : 0));
        if (!board) s.imported.add(page);
      }
      if (deletes) {
        if (board) s.boardStored = s.boardStored.filter((p) => p !== page);
        else {
          s.stored = s.stored.filter((p) => p !== page);
          s.imported.delete(page);
        }
        deleted += 1;
      }
      // The blackboard's page count goes down page by page.
      update(s, {
        sync: { current: index + 1, total: pages.length, saved: saved.length },
        ...(board ? { device: boardDetails(s) } : {}),
      });
      next(index + 1);
    });
  };
  next(0);
}

/** A page drawn on the simulated tablet (tests, `npm run dev`). */
export function mockTabletDraw(count = 1): void {
  const s = getSim();
  for (let i = 0; i < count; i++) s.stored.push(s.nextPage++);
  if (s.status.device?.kind === 'slate') update(s, { device: { ...s.status.device, storedPages: s.stored.length } });
}

/** A page stored on the simulated blackboard with its button (`sim_blackboard_store_page`). */
export function mockBoardDraw(count = 1): void {
  const s = getSim();
  for (let i = 0; i < count; i++) s.boardStored.push(s.boardNextPage++);
  if (s.status.device?.kind === 'blackboard') update(s, { device: boardDetails(s) });
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
    update(s, {
      connection: anyKnown(s) || registeredInfo() ? 'searching' : 'notRegistered',
      registerStep: null,
      error: { code, message: '' },
    });
    if (anyKnown(s)) comeIntoRange(s);
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

// --- Search for a blackboard to add --------------------------------------------------------

/** Ends the search: back to waiting for a known tablet; `code` is why it heard nothing. */
function endSearch(s: Sim, code: string | null): void {
  s.search = null;
  clearTimers(s);
  update(s, {
    connection: anyKnown(s) ? 'searching' : 'notRegistered',
    boardSearch: null,
    error: code ? { code, message: '' } : null,
  });
  if (anyKnown(s)) comeIntoRange(s);
}

function startSearch(s: Sim): void {
  const run = { cancelled: false };
  // The link is closed while one listens: nothing else is under way here (busy is refused before).
  clearTimers(s);
  s.search = run;
  update(s, {
    connection: 'adding',
    error: null,
    boardSearch: { found: [], until: Date.now() + Math.round(mockTablet.stepMs * mockTablet.boardSearchBeats) },
  });
  for (let i = 0; i < mockTablet.boardsHeard; i++) {
    later(s, 1 + i * 0.5, () => {
      const search = s.status.boardSearch;
      if (s.search !== run || !search) return;
      const found = [
        ...search.found,
        { address: `00:00:5E:00:53:0${i + 1}`, name: 'xiaomi.dev.ble', rssi: -52 - 14 * i },
      ];
      update(s, { boardSearch: { ...search, found } });
    });
  }
  later(s, mockTablet.boardSearchBeats, () => {
    if (s.search !== run) return;
    endSearch(s, (s.status.boardSearch?.found.length ?? 0) === 0 ? 'blackboardNotFound' : null);
  });
}

function addBlackboard(s: Sim, address: string): void {
  const heard = s.search ? s.status.boardSearch?.found.find((f) => f.address.toLowerCase() === address.toLowerCase()) : undefined;
  if (!heard) failure('blackboardNotFound', '');
  s.board = {
    kind: 'blackboard',
    name: BOARD_NAME,
    address: heard.address,
    added: new Date().toISOString(),
    width: BOARD_PAGE.width,
    height: BOARD_PAGE.height,
    model: BOARD_MODEL,
  };
  endSearch(s, null);
}

function cancel(s: Sim): void {
  if (s.search) {
    s.search.cancelled = true;
    endSearch(s, null);
  } else if (s.registering) {
    s.registering.cancelled = true;
    s.registering = null;
    s.pressWaiter = null;
    clearTimers(s);
    update(s, { connection: anyKnown(s) || registeredInfo() ? 'searching' : 'notRegistered', registerStep: null, error: null });
    if (anyKnown(s)) comeIntoRange(s);
  } else if (s.sync) {
    // Between two pages: the page being written is finished.
    s.sync.cancelled = true;
  }
}

// --- Commands ------------------------------------------------------------------------------

function isBusyNow(c: Connection): boolean {
  return c === 'syncing' || c === 'live' || c === 'registering' || c === 'adding';
}

function requireConnected(s: Sim): void {
  const c: Connection = s.status.connection;
  if (c === 'notRegistered') failure('tabletNotRegistered', '');
  if (isBusyNow(c)) failure('tabletBusy', '');
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
      if (isBusyNow(s.status.connection)) failure('tabletBusy', '');
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
      if (isBusyNow(c)) failure('tabletBusy', '');
      if (c === 'connected') {
        // The blackboard's pages are deleted once imported: with the option off, only after the user said so.
        if (s.status.active === 'blackboard' && !s.status.options.deleteAfterImport && args.confirmBlackboardDelete !== true) {
          failure('blackboardConfirmDelete', '');
        }
        startSync(s);
      } else {
        update(s, { pending: 'sync' });
      }
      return undefined;
    }

    case 'tablet_rename': {
      const s = getSim();
      reconcile(s);
      requireConnected(s);
      if (s.status.active === 'blackboard') failure('notSupported', '');
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
      // The blackboard cannot be asked its time: nothing to read back.
      return s.status.active === 'blackboard' ? 0 : mockTablet.drift;
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

    case 'tablet_find_blackboards': {
      const s = getSim();
      reconcile(s);
      if (isBusyNow(s.status.connection) && s.status.connection !== 'adding') failure('tabletBusy', '');
      // A search under way starts again from nothing.
      startSearch(s);
      return undefined;
    }

    case 'tablet_add_blackboard': {
      const s = getSim();
      addBlackboard(s, String(args.address ?? ''));
      return structuredClone(s.status);
    }

    case 'tablet_forget_blackboard': {
      const s = getSim();
      if (s.board) {
        s.board = null;
        afterForget(s, 'blackboard');
      }
      return undefined;
    }

    case 'sim_blackboard_store_page': {
      const s = getSim();
      if (!s.status.simulated) failure('simulationOnly', '');
      mockBoardDraw(1);
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
