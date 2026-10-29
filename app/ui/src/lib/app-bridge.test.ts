import { describe, expect, it, vi } from 'vitest';
import {
  ACTION_BUDGET_MS,
  BridgeScreens,
  NoteLoads,
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

/**
 * The library screen; `into` set, opening a note mounts an editor there that loads it as `load`
 * says (at once by default), as `Home.svelte` and `Editor.svelte` do.
 */
function home(
  busy: string | null = null,
  into?: BridgeScreens,
  load: (path: string) => Promise<void> = async () => {},
): HomeSlot & { opened: string[] } {
  const opened: string[] = [];
  return {
    opened,
    busy: () => busy,
    openNote: async (path) => {
      opened.push(path);
      if (!into) return;
      const loads = new NoteLoads();
      const loading = loads.begin();
      into.register('editor', { ...editor(path), loaded: () => loads.loaded() });
      load(path).then(loading.ok, loading.fail);
    },
  };
}

function editor(path: string, dirty = false, blocker: string | null = null): EditorSlot {
  return {
    path: () => path,
    dirty: () => dirty,
    blocker: () => blocker ?? (dirty ? 'unsaved changes' : null),
    loaded: async () => {},
  };
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
    const s = screens();
    const library = home(null, s);
    s.register('home', library);
    const answer = await answerBridge({ kind: 'openNote', path: NOTE }, s);
    expect(okOf(answer)).toEqual({ path: NOTE });
    expect(library.opened).toEqual([NOTE]);
    expect(s.editor?.path()).toBe(NOTE);
  });

  it('answers once the editor has read the note, or with why it could not', async () => {
    let finish!: () => void;
    const s = screens();
    s.register(
      'home',
      home(null, s, () => new Promise<void>((resolve) => (finish = resolve))),
    );
    let answered: BridgeAnswer | null = null;
    const answer = answerBridge({ kind: 'openNote', path: NOTE }, s).then((a) => (answered = a));
    await vi.waitFor(() => expect(s.editor).not.toBeNull());
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(answered).toBeNull();
    finish();
    expect(okOf(await answer)).toEqual({ path: NOTE });

    // A damaged note: its error, code included, not "opened".
    const damaged = screens();
    damaged.register('home', home(null, damaged, () => Promise.reject({ code: 'noteDamaged', message: 'bad zip' })));
    expect(errorOf(await answerBridge({ kind: 'openNote', path: NOTE }, damaged))).toEqual({
      code: 'noteDamaged',
      message: 'bad zip',
    });
    // Asked again while it is on screen: still the same error.
    const again = damaged.editor!;
    expect(again.path()).toBe(NOTE);
    expect(errorOf(await answerBridge({ kind: 'openNote', path: NOTE }, damaged))?.code).toBe('noteDamaged');
  });

  it('an editor that never shows up is not taken for success', async () => {
    const answer = await answerBridge({ kind: 'openNote', path: NOTE }, screens({ home: home() }));
    expect(errorOf(answer)?.code).toBe('appNotResponding');
  });

  it('a load replaced by another note fails with appBusy', async () => {
    const loads = new NoteLoads();
    await expect(loads.loaded()).rejects.toMatchObject({ code: 'appNotResponding' });
    const first = loads.begin();
    const waiting = loads.loaded();
    const second = loads.begin();
    await expect(waiting).rejects.toMatchObject({ code: 'appBusy' });
    // Ending a replaced load changes nothing; the latest one tells.
    first.ok();
    second.fail({ code: 'noteDamaged', message: 'bad zip' });
    await expect(loads.loaded()).rejects.toMatchObject({ code: 'noteDamaged' });
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
    expect(presentation).toHaveBeenCalledWith('page', 2, expect.any(Function));
  });

  it('a newer request replaces the one under way; an old one is given up', async () => {
    const seen: (() => unknown)[] = [];
    const presentation = vi.fn(async (_action: string, _page: number | null, abandoned: () => unknown) => {
      seen.push(abandoned);
      return { open: true, screen: null, page: 1, pages: 3 };
    });
    const s = screens({ home: home(), live: { presentation } });
    await answerBridge({ kind: 'presentation', action: 'page', page: 2 }, s);
    expect(seen[0]()).toBeNull();
    await answerBridge({ kind: 'presentation', action: 'next' }, s);
    expect(seen[0]()).toMatchObject({ code: 'appBusy' });
    expect(seen[1]()).toBeNull();
    vi.useFakeTimers({ now: Date.now() });
    try {
      vi.setSystemTime(Date.now() + ACTION_BUDGET_MS + 1);
      expect(seen[1]()).toMatchObject({ code: 'appNotResponding' });
    } finally {
      vi.useRealTimers();
    }
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

  it('stops stepping as soon as the request is abandoned', async () => {
    const live = fakeLive(10, 0);
    let gone = false;
    const step = live.command;
    live.command = async (kind) => {
      await step(kind);
      // Rust gives up after the second step (time out, or a newer request).
      if (live.commands.length === 2) gone = true;
    };
    const abandoned = () => (gone ? { code: 'appNotResponding', message: 'given up' } : null);
    await expect(drivePresentation(live, 'page', 9, abandoned)).rejects.toMatchObject({
      code: 'appNotResponding',
      message: 'given up',
    });
    // No page changed after that.
    expect(live.commands).toEqual(['nextPage', 'nextPage']);
    expect(live.pageIndex).toBe(2);
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
