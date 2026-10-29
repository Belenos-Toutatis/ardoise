import { describe, expect, it, vi } from 'vitest';
import {
  BridgeScreens,
  answerBridge,
  asBridgeError,
  drivePresentation,
  startAppBridge,
  type BridgeAnswer,
  type BridgeEvent,
  type EditorSlot,
  type HomeSlot,
  type LiveDriver,
  type LiveSlot,
} from './app-bridge.svelte';
import type { LiveCommandKind, MonitorInfo } from './live';

const NOTE = 'C:\\Bibliothèque\\Physique\\Cours.ardoise';

function home(busy: string | null = null): HomeSlot & { opened: string[] } {
  const opened: string[] = [];
  return {
    opened,
    busy: () => busy,
    openNote: async (path) => {
      opened.push(path);
    },
  };
}

function editor(path: string, dirty = false, blocker: string | null = null): EditorSlot {
  return { path: () => path, dirty: () => dirty, blocker: () => blocker ?? (dirty ? 'unsaved changes' : null) };
}

interface Parts {
  app?: BridgeScreens['app'];
  home?: HomeSlot;
  editor?: EditorSlot;
  live?: LiveSlot;
}

function screens(parts: Parts = {}): BridgeScreens {
  const s = new BridgeScreens();
  s.app = parts.app ?? 'home';
  if (parts.home) s.register('home', parts.home);
  if (parts.editor) s.register('editor', parts.editor);
  if (parts.live) s.register('live', parts.live);
  return s;
}

const errorOf = (answer: BridgeAnswer) => ('error' in answer ? answer.error : null);
const okOf = (answer: BridgeAnswer) => ('ok' in answer ? answer.ok : undefined);
const someLive = (): LiveSlot => ({ presentation: vi.fn() });

describe('what the window shows', () => {
  it('names the screen on display and the note in the editor', async () => {
    expect(okOf(await answerBridge({ kind: 'view' }, screens({ app: 'loading' })))).toEqual({
      screen: 'loading',
      note: null,
      unsaved: false,
    });
    expect(okOf(await answerBridge({ kind: 'view' }, screens({ app: 'setup' })))).toMatchObject({ screen: 'setup' });
    expect(okOf(await answerBridge({ kind: 'view' }, screens({ home: home() })))).toMatchObject({ screen: 'library' });
    expect(okOf(await answerBridge({ kind: 'view' }, screens({ home: home(), editor: editor(NOTE, true) })))).toEqual({
      screen: 'editor',
      note: NOTE,
      unsaved: true,
    });
    expect(okOf(await answerBridge({ kind: 'view' }, screens({ home: home(), live: someLive() })))).toMatchObject({
      screen: 'live',
      note: null,
    });
  });

  it('a screen unregisters only itself', () => {
    const s = new BridgeScreens();
    const first = editor('a.ardoise');
    const second = editor('b.ardoise');
    const dropFirst = s.register('editor', first);
    s.register('editor', second);
    dropFirst();
    expect(s.editor).toBe(second);
  });
});

describe('opening a note', () => {
  it('opens it as a click on its card does', async () => {
    const library = home();
    const answer = await answerBridge({ kind: 'openNote', path: NOTE }, screens({ home: library }));
    expect(okOf(answer)).toEqual({ path: NOTE });
    expect(library.opened).toEqual([NOTE]);
  });

  it('the note already in the editor stays as it is', async () => {
    const library = home();
    const answer = await answerBridge(
      { kind: 'openNote', path: NOTE.toLowerCase().replaceAll('\\', '/') },
      screens({ home: library, editor: editor(NOTE, true) }),
    );
    expect(okOf(answer)).toEqual({ path: NOTE });
    expect(library.opened).toEqual([]);
  });

  it('never throws away unsaved changes, a dialog or live mode', async () => {
    const other = 'C:\\B\\autre.ardoise';
    const cases: [BridgeScreens, string][] = [
      [screens({ home: home(), editor: editor(other, true) }), 'unsaved'],
      [screens({ home: home(), editor: editor(other, false, 'a narration is being recorded') }), 'narration'],
      [screens({ home: home('a dialog is open in Ardoise') }), 'dialog'],
      [screens({ home: home(), live: someLive() }), 'live mode'],
      [screens({ app: 'setup' }), 'no library'],
    ];
    for (const [s, words] of cases) {
      const error = errorOf(await answerBridge({ kind: 'openNote', path: NOTE }, s));
      expect(error?.code).toBe('appBusy');
      expect(error?.message).toContain(words);
    }
  });

  it('a failure of the screen keeps its code', async () => {
    const failing: HomeSlot = {
      busy: () => null,
      openNote: () => Promise.reject({ code: 'noteNotFound', message: 'gone' }),
    };
    expect(errorOf(await answerBridge({ kind: 'openNote', path: NOTE }, screens({ home: failing })))).toEqual({
      code: 'noteNotFound',
      message: 'gone',
    });
  });
});

describe('the presentation', () => {
  it('needs live mode on screen, except to stop', async () => {
    const s = screens({ home: home() });
    expect(errorOf(await answerBridge({ kind: 'presentation', action: 'start' }, s))?.code).toBe('liveNotRunning');
    expect(errorOf(await answerBridge({ kind: 'presentation', action: 'next' }, s))?.code).toBe('liveNotRunning');
    expect(okOf(await answerBridge({ kind: 'presentation', action: 'stop' }, s))).toEqual({
      open: false,
      screen: null,
      page: null,
      pages: null,
    });
  });

  it('checks the page and hands the action to the live window', async () => {
    const presentation = vi.fn(async () => ({ open: true, screen: 'Projecteur', page: 2, pages: 3 }));
    const s = screens({ home: home(), live: { presentation } });
    for (const page of [0, 1.5, null]) {
      const error = errorOf(await answerBridge({ kind: 'presentation', action: 'page', page }, s));
      expect(error?.code).toBe('invalidArguments');
    }
    expect(okOf(await answerBridge({ kind: 'presentation', action: 'page', page: 2 }, s))).toMatchObject({ page: 2 });
    expect(presentation).toHaveBeenCalledWith('page', 2);
  });
});

type FakeLive = LiveDriver & {
  pageIndex: number;
  pageCount: number;
  presentation: MonitorInfo | null;
  commands: LiveCommandKind[];
};

/** A live window: commands change the page at once, the projection opens and closes. */
function fakeLive(pageCount = 3, pageIndex = 0): FakeLive {
  const monitor: MonitorInfo = {
    key: 'P@1920,0',
    name: 'Projecteur',
    x: 1920,
    y: 0,
    width: 1920,
    height: 1080,
    scale: 1,
    primary: false,
    chosen: true,
  };
  const live: FakeLive = {
    pageIndex,
    pageCount,
    presentation: null,
    commands: [],
    project: async () => {
      live.presentation = monitor;
      return monitor;
    },
    stopProjecting: async () => {
      live.presentation = null;
    },
    command: async (kind) => {
      live.commands.push(kind);
      if (kind === 'nextPage') {
        live.pageIndex += 1;
        live.pageCount = Math.max(live.pageCount, live.pageIndex + 1);
      } else if (kind === 'previousPage') {
        live.pageIndex = Math.max(0, live.pageIndex - 1);
      }
    },
    until: async (done) => done(),
  };
  return live;
}

describe('driving the live window', () => {
  it('projects and stops projecting', async () => {
    const live = fakeLive();
    expect(await drivePresentation(live, 'start', null)).toEqual({ open: true, screen: 'Projecteur', page: 1, pages: 3 });
    expect(await drivePresentation(live, 'stop', null)).toEqual({ open: false, screen: null, page: 1, pages: 3 });
    // Stopping again changes nothing.
    expect((await drivePresentation(live, 'stop', null)).open).toBe(false);
  });

  it('pages as the arrow keys do', async () => {
    const live = fakeLive(3, 0);
    expect(await drivePresentation(live, 'previous', null)).toMatchObject({ page: 1 });
    expect(live.commands).toEqual([]);
    expect(await drivePresentation(live, 'next', null)).toMatchObject({ page: 2, pages: 3 });
    expect(await drivePresentation(live, 'page', 3)).toMatchObject({ page: 3, pages: 3 });
    expect(await drivePresentation(live, 'page', 1)).toMatchObject({ page: 1, pages: 3 });
    expect(live.commands).toEqual(['nextPage', 'nextPage', 'previousPage', 'previousPage']);
    // `next` from the last page adds a blank one; `page` never goes past the last.
    await drivePresentation(live, 'page', 3);
    expect(await drivePresentation(live, 'next', null)).toMatchObject({ page: 4, pages: 4 });
    await expect(drivePresentation(live, 'page', 9)).rejects.toMatchObject({ code: 'invalidArguments' });
  });

  it('says so when the window does not follow', async () => {
    const live = fakeLive(3, 0);
    live.command = async () => {};
    live.until = async () => false;
    await expect(drivePresentation(live, 'page', 3)).rejects.toMatchObject({ code: 'appNotResponding' });
  });
});

describe('wiring', () => {
  it('answers each event with the id it came with', async () => {
    let handler: ((event: BridgeEvent) => void) | null = null;
    const replies: [number, BridgeAnswer][] = [];
    const unlisten = vi.fn();
    const stop = startAppBridge(
      {
        listen: async (h) => {
          handler = h;
          return unlisten;
        },
        reply: async (id, answer) => {
          replies.push([id, answer]);
        },
      },
      screens({ home: home() }),
    );
    await vi.waitFor(() => expect(handler).not.toBeNull());
    handler!({ id: 7, request: { kind: 'view' } });
    handler!({ id: 8, request: { kind: 'presentation', action: 'next' } });
    await vi.waitFor(() => expect(replies.length).toBe(2));
    expect(replies.find(([id]) => id === 7)?.[1]).toEqual({ ok: { screen: 'library', note: null, unsaved: false } });
    expect(errorOf(replies.find(([id]) => id === 8)![1])?.code).toBe('liveNotRunning');
    stop();
    expect(unlisten).toHaveBeenCalled();
  });

  it('errors become { code, message }', () => {
    expect(asBridgeError({ code: 'noMonitor', message: 'no screen' })).toEqual({ code: 'noMonitor', message: 'no screen' });
    expect(asBridgeError(new Error('boom'))).toEqual({ code: 'unexpected', message: 'boom' });
    expect(asBridgeError('odd')).toEqual({ code: 'unexpected', message: 'odd' });
  });
});
