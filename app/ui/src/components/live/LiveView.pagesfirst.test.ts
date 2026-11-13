// @vitest-environment jsdom
/// <reference types="node" />
// H8: the « pages first » dialog of live mode (docs/design/blackboard.md §9.5). The Xiaomi blackboard holds stored pages
// and may not sync them first: `live_start` / `live_resume` are refused (`blackboardPagesFirst`), or the run that was
// waiting ends with the `refused` event. The live view asks whether to import and delete them, follows the sync, then
// asks live mode again. The mock tablet refuses under the same conditions as the service. No microphone is touched.
import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveRefuse, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { mockBoardDraw, mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import LiveView from './LiveView.svelte';

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  // Tests run from `app/` (the Vite root).
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  tablet.reset();
  setLocale('fr');
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
  // Slow enough for `vi.waitFor` (it looks every 50 ms) to see the import under way: a page takes 60 ms.
  mockTablet.stepMs = 100;
  mockTablet.inRange = false;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  resetMockTablet();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

/** The blackboard is known and connected, holds `pages` pages, with these options (the sync does not run by itself). */
async function connectBoard(pages: number, options: { autoSync: boolean; deleteAfterImport: boolean }) {
  mockTablet.boardPages = pages;
  await api.tabletSetOptions({ ...options, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' });
  tablet.start();
  await api.tabletFindBlackboards();
  await vi.waitFor(() => expect(tablet.status?.boardSearch?.found.length).toBe(1));
  await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
  await vi.waitFor(() => expect(tablet.status?.connection).toBe('connected'));
  expect(tablet.status?.active).toBe('blackboard');
}

const OFF = { autoSync: false, deleteAfterImport: false };

function render(resume: string | null = null) {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(LiveView, { target, props: { app, resume, onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  const dialog = () => target.querySelector<HTMLElement>('[role="alertdialog"]');
  const part = (slot: string) => target.querySelector<HTMLElement>(`[data-slot="${slot}"]`);
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`)!;
  return { target, onclose, root, dialog, part, action };
}

type View = ReturnType<typeof render>;

const asked = (view: View) => vi.waitFor(() => expect(view.dialog()).not.toBeNull());
const live = (view: View) => vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
const pagesIn = (view: View) => view.part('pages-first')?.dataset.pages;
const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];

describe('live mode refused because the blackboard holds stored pages', () => {
  it('asks, with the count, instead of showing the sentence with its {count} in the start banner', async () => {
    await connectBoard(3, OFF);
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    expect(start).toHaveBeenCalledOnce();
    expect(pagesIn(view)).toBe('3');
    expect(view.dialog()!.textContent).toContain('La tablette Xiaomi contient encore 3 pages enregistrées.');
    expect(view.dialog()!.getAttribute('aria-modal')).toBe('true');
    expect(view.target.textContent).not.toContain('{count}');
    // Not a failure to start: no banner, nothing started.
    expect(view.part('start-failed')).toBeNull();
    expect(view.root().dataset.state).toBe('idle');
    expect(view.action('pages-first-import').textContent?.trim()).toBe('Importer et effacer');
    expect(view.action('pages-first-cancel').textContent?.trim()).toBe('Annuler');
    // The first button has the focus.
    expect(document.activeElement).toBe(view.action('pages-first-import'));
  });

  it('says one page in the singular, and in English', async () => {
    setLocale('en');
    await connectBoard(1, OFF);
    const view = render();
    await asked(view);
    expect(view.dialog()!.textContent).toContain('The Xiaomi tablet still holds 1 stored page.');
    expect(view.action('pages-first-import').textContent?.trim()).toBe('Import and delete');
    expect(view.action('pages-first-cancel').textContent?.trim()).toBe('Cancel');
    expect(view.dialog()!.querySelector('h2')?.textContent).toBe("Import the tablet's pages first");
  });

  it('is the same with the automatic sync on and « delete after import » off, and not otherwise', async () => {
    await connectBoard(2, { autoSync: true, deleteAfterImport: false });
    const view = render();
    await asked(view);
    expect(pagesIn(view)).toBe('2');
  });

  it('imports and deletes the pages with the confirmation, then starts live mode', async () => {
    await connectBoard(3, OFF);
    const sync = vi.spyOn(api, 'tabletSync');
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    // The import runs: its progress is shown and the button cannot be pressed again.
    await vi.waitFor(() => expect(view.part('pages-first')?.dataset.phase).toBe('importing'));
    expect(view.action('pages-first-import').disabled).toBe(true);
    await vi.waitFor(() => expect(view.part('pages-first-progress')?.textContent).toMatch(/Page \d sur 3/));
    expect(view.target.querySelector('[data-slot="pages-first-progress"] progress')).not.toBeNull();
    view.action('pages-first-import').click();
    await live(view);
    expect(sync).toHaveBeenCalledExactlyOnceWith(true);
    expect(start).toHaveBeenCalledTimes(2);
    expect(view.dialog()).toBeNull();
    expect(tablet.status?.device?.storedPages).toBe(0);
    expect(view.part('start-failed')).toBeNull();
  });

  it('shows the dialog again, with the new count, when pages are left', async () => {
    await connectBoard(3, OFF);
    const real = api.tabletSync;
    // Two more pages are stored while the sync runs: it reads the three it started with.
    const sync = vi.spyOn(api, 'tabletSync').mockImplementation(async (confirm) => {
      await real(confirm);
      mockBoardDraw(2);
    });
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    await vi.waitFor(() => expect(pagesIn(view)).toBe('2'));
    await vi.waitFor(() => expect(view.part('pages-first')?.dataset.phase).toBe('asking'));
    expect(view.dialog()!.textContent).toContain('encore 2 pages enregistrées');
    expect(view.root().dataset.state).toBe('idle');
    expect(start).toHaveBeenCalledOnce();
    sync.mockRestore();
    view.action('pages-first-import').click();
    await live(view);
    expect(start).toHaveBeenCalledTimes(2);
  });

  it('shows the error of the sync and does not start live mode', async () => {
    await connectBoard(3, OFF);
    vi.spyOn(api, 'tabletSync').mockImplementation(async () => {
      mockTabletPatch({ lastSync: { at: Date.now() + 5, saved: [], deleted: 1, error: { code: 'pageUnreadable', message: '' } } });
    });
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    await vi.waitFor(() => expect(view.part('pages-first-error')).not.toBeNull());
    const error = view.part('pages-first-error')!;
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent).toContain("L'import des pages n'a pas abouti : La tablette Xiaomi a envoyé une page qu'Ardoise ne sait pas lire");
    expect(start).toHaveBeenCalledOnce();
    expect(view.root().dataset.state).toBe('idle');
    // The user decides: the import can be asked again, or cancelled.
    expect(view.action('pages-first-import').disabled).toBe(false);
  });

  it('shows the failure of a sync the service did not take', async () => {
    await connectBoard(3, OFF);
    vi.spyOn(api, 'tabletSync').mockRejectedValue({ code: 'tabletNotConnected', message: '' });
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    await vi.waitFor(() => expect(view.part('pages-first-error')).not.toBeNull());
    expect(view.part('pages-first-error')!.textContent).toContain("La tablette n'est pas connectée");
    expect(view.part('pages-first')?.dataset.phase).toBe('asking');
    expect(start).toHaveBeenCalledOnce();
  });

  it('ends the import with the error when the link is lost', async () => {
    await connectBoard(3, OFF);
    // The sync is accepted, then the tablet goes away before it reads anything.
    vi.spyOn(api, 'tabletSync').mockImplementation(async () => {
      mockTabletPatch({ connection: 'searching', active: null });
    });
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    await vi.waitFor(() => expect(view.part('pages-first-error')).not.toBeNull());
    expect(view.part('pages-first-error')!.textContent).toContain("La tablette n'est plus connectée");
    expect(view.part('pages-first')?.dataset.phase).toBe('asking');
    expect(start).toHaveBeenCalledOnce();
  });

  it('cancels with the button: nothing is started and the library is given back', async () => {
    await connectBoard(3, OFF);
    const sync = vi.spyOn(api, 'tabletSync');
    const stop = vi.spyOn(api, 'liveStop');
    const view = render();
    await asked(view);
    view.action('pages-first-cancel').click();
    await vi.waitFor(() => expect(view.dialog()).toBeNull());
    expect(view.onclose).toHaveBeenCalledExactlyOnceWith(undefined, true);
    expect(sync).not.toHaveBeenCalled();
    expect(stop).not.toHaveBeenCalled();
    expect(view.root().dataset.state).toBe('idle');
  });

  it('cancels with Escape, and the focus goes back to where it was', async () => {
    await connectBoard(3, OFF);
    const launcher = document.createElement('button');
    launcher.textContent = 'Mode direct';
    document.body.append(launcher);
    launcher.focus();
    const view = render();
    await asked(view);
    expect(document.activeElement).toBe(view.action('pages-first-import'));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(view.dialog()).toBeNull());
    expect(view.onclose).toHaveBeenCalledExactlyOnceWith(undefined, true);
    expect(document.activeElement).toBe(launcher);
    // The live view's own Escape (end live mode?) was not also taken.
    expect(view.target.querySelector('[data-action="leave-confirm"]')).toBeNull();
  });

  it('keeps Tab inside the dialog', async () => {
    await connectBoard(3, OFF);
    const view = render();
    await asked(view);
    view.action('pages-first-cancel').focus();
    view.dialog()!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(view.action('pages-first-import'));
  });

  it('stops the sync between two pages when cancelled during the import, then leaves', async () => {
    await connectBoard(3, OFF);
    const cancel = vi.spyOn(api, 'tabletCancel');
    const start = vi.spyOn(api, 'liveStart');
    const view = render();
    await asked(view);
    view.action('pages-first-import').click();
    await vi.waitFor(() => expect(view.part('pages-first')?.dataset.phase).toBe('importing'));
    view.action('pages-first-cancel').click();
    expect(cancel).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(view.dialog()).toBeNull());
    expect(view.onclose).toHaveBeenCalledExactlyOnceWith(undefined, true);
    expect(start).toHaveBeenCalledOnce();
  });

  it('is the same when resuming live mode on a sheet', async () => {
    await connectBoard(2, OFF);
    // The blackboard writes only on a page of its own size: a sheet of that size.
    const path = sheetPath();
    mockLibrary().notes.get(path)!.note.page = { width: 20500, height: 27200 };
    const resume = vi.spyOn(api, 'liveResume');
    const start = vi.spyOn(api, 'liveStart');
    const view = render(path);
    await asked(view);
    expect(pagesIn(view)).toBe('2');
    expect(view.target.querySelector('[data-slot="resume-failed"]')).toBeNull();
    view.action('pages-first-import').click();
    await live(view);
    expect(resume).toHaveBeenCalledTimes(2);
    expect(resume).toHaveBeenNthCalledWith(2, path);
    expect(start).not.toHaveBeenCalled();
    expect(view.dialog()).toBeNull();
  });

  it('is asked by the refused event too (the run waited for the blackboard)', async () => {
    // No blackboard is connected: live mode waits for it (the mock only goes live after a long wait).
    mockLiveConfig.waitMs = 60_000;
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    expect(view.dialog()).toBeNull();
    // It switches on, holding four pages: the service ends the run (`refused`, then the `idle` snapshot).
    mockLiveRefuse('blackboardPagesFirst', 'the Xiaomi tablet holds 4 stored pages: import them before live mode');
    await asked(view);
    expect(pagesIn(view)).toBe('4');
    expect(view.root().dataset.state).toBe('idle');
    expect(view.part('start-failed')).toBeNull();
    expect(view.target.textContent).not.toContain('{count}');
    view.action('pages-first-cancel').click();
    expect(view.onclose).toHaveBeenCalledExactlyOnceWith(undefined, true);
  });

  it('does not meet the refusal when the sync may run first', async () => {
    await connectBoard(0, { autoSync: true, deleteAfterImport: true });
    // A page is stored after the connection: with both options on it is synced first, then live mode starts.
    mockBoardDraw(2);
    const sync = vi.spyOn(api, 'tabletSync');
    const view = render();
    await live(view);
    expect(view.dialog()).toBeNull();
    await vi.waitFor(() => expect(tablet.status?.lastSync?.deleted).toBe(2));
    // It was the automatic sync (no confirmation, not asked by the interface).
    expect(sync).not.toHaveBeenCalled();
    expect(tablet.status?.device?.storedPages).toBe(0);
  });

  it('leaves another refusal in the start banner', async () => {
    vi.spyOn(api, 'liveStart').mockRejectedValueOnce({ code: 'tabletNotRegistered', message: '' });
    const view = render();
    await vi.waitFor(() => expect(view.part('start-failed')).not.toBeNull());
    expect(view.dialog()).toBeNull();
    // And live mode itself, started through the mock, is not affected.
    expect(mockLiveInvoke('live_start', {})).toBeTruthy();
  });
});
