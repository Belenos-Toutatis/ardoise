/**
 * The interface's side of the assistant's bridge (MCP, SPEC §4.9; docs/design/mcp.md, "As built:
 * bridge and guide"). Rust (`app_bridge.rs`) sends the main window a `bridge-request` event; the
 * screens on display carry the request out with the same code as a click (open a note, project
 * the live page, change page) and the answer goes back with `bridge_reply`.
 *
 * Each screen registers what it offers while it is mounted (`bridgeScreens`): the library
 * (`home`), the editor and the live window. `answerBridge` is pure apart from them (tested with
 * stand-ins); `startAppBridge` wires it to the events. Error messages are for the assistant
 * (English, like the `message` of Rust errors), never shown in the interface.
 */
import { tick } from 'svelte';
import type { LiveCommandKind, MonitorInfo } from './live';

export type PresentationAction = 'start' | 'stop' | 'next' | 'previous' | 'page';

export type BridgeRequest =
  | { kind: 'view' }
  | { kind: 'openNote'; path: string }
  | { kind: 'presentation'; action: PresentationAction; page?: number | null };

export interface BridgeError {
  code: string;
  message: string;
}

export type BridgeAnswer = { ok: unknown } | { error: BridgeError };

/** `library`, `editor`, `live`, or while there is no library screen: `setup`, `loading`, `error`. */
export type BridgeScreen = 'library' | 'editor' | 'live' | 'setup' | 'loading' | 'error';

export interface BridgeView {
  screen: BridgeScreen;
  /** Path of the note in the editor. */
  note: string | null;
  /** The editor holds unsaved changes. */
  unsaved: boolean;
}

export interface PresentationState {
  open: boolean;
  screen: string | null;
  /** 1-based; null without live mode on screen. */
  page: number | null;
  pages: number | null;
}

/** The library screen (`Home.svelte`). */
export interface HomeSlot {
  /** Why another screen cannot be shown now (a dialog, the tablet test), or null. */
  busy(): string | null;
  /** Shows the note in the editor, as a click on its card. */
  openNote(path: string): Promise<void>;
}

/** The editor (`Editor.svelte`). */
export interface EditorSlot {
  path(): string;
  dirty(): boolean;
  /** Why the editor cannot be left now (unsaved changes, a recording…), or null. */
  blocker(): string | null;
}

/** The live window (`LiveView.svelte`). */
export interface LiveSlot {
  presentation(action: PresentationAction, page: number | null): Promise<PresentationState>;
}

/** What the main window shows, as the screens register themselves. */
export class BridgeScreens {
  app = $state<'loading' | 'setup' | 'error' | 'home'>('loading');
  home = $state.raw<HomeSlot | null>(null);
  editor = $state.raw<EditorSlot | null>(null);
  live = $state.raw<LiveSlot | null>(null);

  /** Registers `slot` while a screen is mounted; returns what unregisters it (if still the same). */
  register<K extends 'home' | 'editor' | 'live'>(kind: K, slot: BridgeScreens[K]): () => void {
    const screens: BridgeScreens = this;
    screens[kind] = slot;
    return () => {
      if (screens[kind] === slot) screens[kind] = null as BridgeScreens[K];
    };
  }
}

/** The main window's screens. */
export const bridgeScreens = new BridgeScreens();

class Refusal extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

const busy = (message: string) => new Refusal('appBusy', message);

/** What the main window shows. */
export function bridgeView(screens: BridgeScreens): BridgeView {
  const screen: BridgeScreen = screens.live
    ? 'live'
    : screens.editor
      ? 'editor'
      : screens.home
        ? 'library'
        : screens.app === 'home'
          ? 'loading'
          : screens.app;
  return {
    screen,
    note: screens.live ? null : (screens.editor?.path() ?? null),
    unsaved: !screens.live && (screens.editor?.dirty() ?? false),
  };
}

/** Paths as Windows compares them. */
const samePath = (a: string, b: string) => a.replaceAll('/', '\\').toLowerCase() === b.replaceAll('/', '\\').toLowerCase();

async function openNote(screens: BridgeScreens, path: string): Promise<{ path: string }> {
  const home = screens.home;
  if (!home) throw busy(`no library is open in Ardoise (${screens.app})`);
  if (screens.live) throw busy('live mode is on screen in Ardoise: the teacher must finish it first');
  const editor = screens.editor;
  if (editor && samePath(editor.path(), path)) return { path: editor.path() };
  const blocked = editor?.blocker() ?? home.busy();
  if (blocked) throw busy(blocked);
  await home.openNote(path);
  return { path };
}

async function presentation(
  screens: BridgeScreens,
  action: PresentationAction,
  page: number | null,
): Promise<PresentationState> {
  if (action === 'page' && !(Number.isInteger(page) && (page ?? 0) >= 1)) {
    throw new Refusal('invalidArguments', '`page` must be a page number from 1');
  }
  const live = screens.live;
  if (!live) {
    // Nothing is projected without the live window: stopping is already done.
    if (action === 'stop') return { open: false, screen: null, page: null, pages: null };
    throw new Refusal(
      'liveNotRunning',
      'live mode is not open in Ardoise: the teacher starts it from the tablet panel (Live mode)',
    );
  }
  return live.presentation(action, page);
}

/** Carries `request` out with the screens on display; never throws. */
export async function answerBridge(request: BridgeRequest, screens: BridgeScreens = bridgeScreens): Promise<BridgeAnswer> {
  try {
    switch (request.kind) {
      case 'view':
        return { ok: bridgeView(screens) };
      case 'openNote':
        return { ok: await openNote(screens, request.path) };
      case 'presentation':
        return { ok: await presentation(screens, request.action, request.page ?? null) };
      default:
        throw new Refusal('invalidRequest', `unknown request ${(request as { kind: string }).kind}`);
    }
  } catch (error) {
    return { error: asBridgeError(error) };
  }
}

/** `{ code, message }` of anything thrown (a Rust command's error keeps its code). */
export function asBridgeError(error: unknown): BridgeError {
  if (error instanceof Refusal) return { code: error.code, message: error.message };
  if (typeof error === 'object' && error !== null && 'code' in error && 'message' in error) {
    const { code, message } = error as { code: unknown; message: unknown };
    return { code: String(code), message: String(message) };
  }
  return { code: 'unexpected', message: error instanceof Error ? error.message : String(error) };
}

// ---------------------------------------------------------------------------------------------
// The live window's part

/** What the live window gives `drivePresentation`. */
export interface LiveDriver {
  /** Its session as it is now (`LiveState`). */
  readonly pageIndex: number;
  readonly pageCount: number;
  readonly presentation: MonitorInfo | null;
  /** The "Project" button: opens the presentation window on the chosen screen. */
  project(): Promise<MonitorInfo>;
  /** "Stop projecting". */
  stopProjecting(): Promise<void>;
  /** A live command, as the keys and buttons send it. */
  command(kind: LiveCommandKind): Promise<void>;
  /** Waits until `done()` holds (the session's batches update the state), at most `timeoutMs`. */
  until(done: () => boolean, timeoutMs?: number): Promise<boolean>;
}

/** Polls `done` every 20 ms until it holds or `timeoutMs` passes. */
export async function until(done: () => boolean, timeoutMs = 3000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (!done()) {
    if (Date.now() > deadline) return false;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  return true;
}

function presentationState(live: LiveDriver, screen?: string | null): PresentationState {
  return {
    open: live.presentation !== null,
    screen: screen !== undefined ? screen : (live.presentation?.name ?? null),
    page: live.pageIndex + 1,
    pages: live.pageCount,
  };
}

/** One action of the `presentation` tool, as the live window's buttons and keys do it. */
export async function drivePresentation(
  live: LiveDriver,
  action: PresentationAction,
  page: number | null,
): Promise<PresentationState> {
  switch (action) {
    case 'start': {
      const shown = await live.project();
      await live.until(() => live.presentation !== null);
      return { ...presentationState(live, shown.name), open: true };
    }
    case 'stop':
      if (live.presentation !== null) {
        await live.stopProjecting();
        await live.until(() => live.presentation === null);
      }
      return { ...presentationState(live), open: false, screen: null };
    case 'next':
    case 'previous': {
      const from = live.pageIndex;
      if (action === 'previous' && from === 0) return presentationState(live);
      await live.command(action === 'next' ? 'nextPage' : 'previousPage');
      await live.until(() => live.pageIndex !== from);
      return presentationState(live);
    }
    case 'page': {
      const target = (page ?? 1) - 1;
      if (target >= live.pageCount) {
        throw new Refusal('invalidArguments', `there are ${live.pageCount} pages: no page ${target + 1}`);
      }
      // One step at a time, as the arrow keys: never past the last page (which would add one).
      for (let steps = 0; live.pageIndex !== target && steps < 1000; steps++) {
        const from = live.pageIndex;
        await live.command(from < target ? 'nextPage' : 'previousPage');
        if (!(await live.until(() => live.pageIndex !== from))) break;
      }
      if (live.pageIndex !== target) {
        throw new Refusal('appNotResponding', `the live window did not reach page ${target + 1}`);
      }
      return presentationState(live);
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Wiring

/** A request from Rust, as the `bridge-request` event carries it. */
export interface BridgeEvent {
  id: number;
  request: BridgeRequest;
}

/** What `startAppBridge` uses (the real `api`, or stand-ins in tests). */
export interface BridgeChannel {
  listen(handler: (event: BridgeEvent) => void): Promise<() => void>;
  reply(id: number, answer: BridgeAnswer): Promise<void>;
}

/** Answers every request from Rust until the returned function is called. */
export function startAppBridge(channel: BridgeChannel, screens: BridgeScreens = bridgeScreens): () => void {
  let stopped = false;
  let unlisten: (() => void) | null = null;
  void channel
    .listen((event) => {
      void (async () => {
        // A screen that has just changed has registered by the next tick.
        await tick();
        const answer = await answerBridge(event.request, screens);
        if (!stopped) await channel.reply(event.id, answer).catch(() => {});
      })();
    })
    .then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    })
    .catch(() => {});
  return () => {
    stopped = true;
    unlisten?.();
  };
}
