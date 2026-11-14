// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M16, package C (docs/design/side-panel.md, §6): the Aide tab of the control window shows `LIVE_SHORTCUTS`,
// and every shortcut it shows acts in the mounted window. A probe per entry (a first test fails when an entry has
// none) dispatches the entry's first shown chord on `window` and checks its effect. The `presentation` group, which
// holds the keys of the projected window, has no probe here: `Presentation.marks.test.ts` covers Suppr and Retour
// arrière there, and the last test checks that they do nothing in the control window, which is why they are in a group.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../../App.svelte';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { LIVE_SHORTCUTS } from '../../lib/live-shortcuts';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveTick, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { resetMockTablet } from '../../lib/mock-tablet';
import { share } from '../../lib/share-state.svelte';
import { chordEvents, type Shortcut } from '../../lib/shortcuts';
import { tablet } from '../../lib/tablet-state.svelte';
import type { LiveViewState } from '../../lib/live';
import { openSideTab } from '../side/side-test-helpers';
import LiveView from './LiveView.svelte';

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
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
  resetMockShare();
  share.reset();
  tablet.reset();
  setLocale('fr');
  localStorage.clear();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
  resetMockShare();
  resetMockLive();
  vi.restoreAllMocks();
});

const PAGE = { width: 29700, height: 21600 };
const TRANSFORM = fitTransform(PAGE, { width: 800, height: 560 }, 12);
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });
const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];

function render(resume: string | null = null) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, resume, onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`button[data-action="${name}"]`)!;
  const canvas = () => q<HTMLCanvasElement>('canvas.live-canvas')!;
  const zoom = () => Number(q<HTMLElement>('[data-section="zone"] [data-zoom]')!.dataset.zoom);
  const fire = (type: string, p: { x: number; y: number }) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    const props = { pointerType: 'mouse', pointerId: 1, pressure: 0.5, timeStamp: 0 };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas().dispatchEvent(event);
  };
  /** A lasso round the first stroke of the resumed sheet (the wave at y = 3000). */
  const lasso = () => {
    const corners = [at(1500, 1800), at(22500, 1800), at(22500, 4200), at(1500, 4200)];
    fire('pointerdown', corners[0]);
    for (const corner of corners.slice(1)) fire('pointermove', corner);
    fire('pointermove', at(1500, 1810));
    fire('pointerup', at(1500, 1810));
  };
  return { target, q, action, canvas, zoom, lasso };
}

type View = ReturnType<typeof render>;

async function live(view: View) {
  await vi.waitFor(() => expect(view.q('.live-view')!.dataset.state).toBe('live'));
}

const entry = (id: string): Shortcut<object> => LIVE_SHORTCUTS.find((e) => e.id === id)!;

/** Presses the `index`th chord the help shows for entry `id`, on `window`, as a keyboard does. */
function send(id: string, index = 0): KeyboardEvent {
  const shown = entry(id).keys.filter((key) => key.shown !== false);
  const [press] = chordEvents(shown[index].chord);
  const event = new KeyboardEvent('keydown', {
    key: press.key,
    code: press.code,
    ctrlKey: press.ctrlKey,
    shiftKey: press.shiftKey,
    altKey: press.altKey,
    bubbles: true,
    cancelable: true,
  });
  window.dispatchEvent(event);
  return event;
}

const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');

/** The sheet resumed, its first stroke chosen with the Select tool: two strokes on the page, one of them selected. */
async function withSelection(): Promise<View> {
  const view = render(sheetPath());
  await live(view);
  send('select');
  await tick();
  view.lasso();
  await vi.waitFor(() => expect(view.q('[data-slot="select-bar"]')).not.toBeNull());
  expect(view.canvas().dataset.strokes).toBe('2');
  return view;
}

/** What each entry of the table does in the window, once its first shown chord is pressed. */
const PROBES: Record<string, () => Promise<void>> = {
  async next() {
    const view = render();
    await live(view);
    send('next');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
  },
  async previous() {
    const view = render();
    await live(view);
    send('next');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    send('previous');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
  },
  async clear() {
    const view = render();
    await live(view);
    mockLiveTick(150);
    await tick();
    expect(Number(view.canvas().dataset.strokes)).toBeGreaterThan(0);
    send('clear');
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('0'));
  },
  async board() {
    const view = render();
    await live(view);
    send('board');
    await vi.waitFor(() => expect(view.action('board').getAttribute('aria-pressed')).toBe('true'));
  },
  async laser() {
    const view = render();
    await live(view);
    send('laser');
    await vi.waitFor(() => expect(view.action('laser').getAttribute('aria-pressed')).toBe('true'));
  },
  async select() {
    const view = render();
    await live(view);
    send('select');
    await tick();
    expect(view.action('select').getAttribute('aria-pressed')).toBe('true');
  },
  async pen() {
    const view = render();
    await live(view);
    send('pen');
    await tick();
    expect(view.action('pen').getAttribute('aria-pressed')).toBe('true');
  },
  async delete() {
    const view = await withSelection();
    send('delete');
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('1'));
  },
  async undo() {
    const view = await withSelection();
    send('delete');
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('1'));
    send('undo');
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('2'));
  },
  async zoom() {
    const view = render();
    await live(view);
    expect(view.zoom()).toBe(1);
    send('zoom');
    await vi.waitFor(() => expect(view.zoom()).toBeCloseTo(1.25, 6));
  },
  async whole() {
    const view = render();
    await live(view);
    send('zoom');
    await vi.waitFor(() => expect(view.zoom()).toBeCloseTo(1.25, 6));
    send('whole');
    await vi.waitFor(() => expect(view.zoom()).toBe(1));
  },
  async width() {
    const view = render();
    await live(view);
    view.action('page-width').click();
    await tick();
    const wide = view.zoom();
    send('zoom');
    await vi.waitFor(() => expect(view.zoom()).not.toBeCloseTo(wide, 6));
    send('width');
    await vi.waitFor(() => expect(view.zoom()).toBeCloseTo(wide, 6));
  },
  async pan() {
    const view = render();
    await live(view);
    send('zoom');
    send('zoom');
    await vi.waitFor(() => expect(view.zoom()).toBeCloseTo(1.5625, 2));
    const sent = vi.spyOn(api, 'liveSetView');
    send('pan', 1);
    await vi.waitFor(() => expect(sent).toHaveBeenCalled());
    const before = sent.mock.calls.at(-1)![0] as LiveViewState;
    // Shift + → moved the area right; Shift + ← brings it back.
    send('pan', 0);
    await vi.waitFor(() => expect((sent.mock.calls.at(-1)![0] as LiveViewState).x).toBeLessThan(before.x));
  },
  async leave() {
    // Esc with a selection deselects first; then, with nothing projected, it asks before ending live mode.
    const view = await withSelection();
    send('leave');
    await vi.waitFor(() => expect(view.q('[data-slot="select-bar"]')).toBeNull());
    expect(dialog()).toBeNull();
    send('leave');
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
  },
  async close() {
    const view = render();
    await live(view);
    send('close');
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
  },
  async guide() {
    // F1 is the application's too (`App.svelte`, whatever is on display): it opens the user guide, here in the language of
    // the window, the notice of a web fallback staying out of live mode's way.
    const opened = vi.spyOn(api, 'openGuide').mockResolvedValue('local');
    vi.spyOn(api, 'appState').mockResolvedValue(app);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(App, { target });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await vi.waitFor(() => expect(target.querySelector('.tablet-status')).not.toBeNull());
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await vi.waitFor(() => expect(target.querySelector('.tablet-panel button[data-action="live"]')).not.toBeNull());
    target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('section.live-view[data-state="live"]')).not.toBeNull());
    const event = send('guide');
    expect(event.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(opened).toHaveBeenCalledWith('guide', 'fr'));
    expect(dialog()).toBeNull();
  },
  async quit() {
    // Ctrl + Q is the application's (`App.svelte`, whatever is on display); live mode answers with its question.
    vi.spyOn(api, 'appState').mockResolvedValue(app);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(App, { target });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await vi.waitFor(() => expect(target.querySelector('.tablet-status')).not.toBeNull());
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await vi.waitFor(() => expect(target.querySelector('.tablet-panel button[data-action="live"]')).not.toBeNull());
    target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('section.live-view[data-state="live"]')).not.toBeNull());
    send('quit');
    await vi.waitFor(() => expect(dialog()?.textContent).toContain('Quitter Ardoise ?'));
  },
};

/** The entries the control window handles: the keys of the projected window have their own tests. */
const ENTRIES = LIVE_SHORTCUTS.filter((e) => e.group !== 'presentation');

describe('every shortcut the help of the control window shows acts in the window', () => {
  it('has a probe for every entry of the table, and no probe without one', () => {
    expect(Object.keys(PROBES).sort()).toEqual(ENTRIES.map((e) => e.id).sort());
  });

  it.each(ENTRIES.map((e) => e.id))('%s', async (id) => {
    await PROBES[id]();
  });
});

describe('the Aide tab of the control window', () => {
  it('lists each entry of the table, the keys of the projected window apart', async () => {
    const view = render();
    await live(view);
    await openSideTab(view.target, 'help');
    const help = view.q('[data-tab-panel="help"]')!;
    expect(help.hidden).toBe(false);
    expect(help.querySelector('h3')!.textContent).toBe('Raccourcis du mode direct');
    const ids = [...help.querySelectorAll<HTMLElement>('dt[data-shortcut]')].map((dt) => dt.dataset.shortcut);
    expect(ids).toEqual(LIVE_SHORTCUTS.map((e) => e.id));
    // The keys that do nothing in this window sit under their own heading, with the corrected texts.
    const marks = help.querySelector<HTMLElement>('dt[data-shortcut="marks"]')!;
    expect(help.querySelector('h4')!.textContent).toBe('Dans la fenêtre projetée');
    expect(marks.textContent).toContain('Suppr');
    expect(marks.textContent).toContain('Retour arrière');
    expect(marks.nextElementSibling!.textContent).toBe('Effacer les annotations tracées sur la présentation');
    expect(help.querySelector('dt[data-shortcut="delete"]')!.nextElementSibling!.textContent).toBe('Supprimer la sélection');
    expect(help.querySelector('p.pane-help')!.textContent).toContain('Espace tourne la page même quand un bouton a le focus');
  });

  it('shows the same table in English', async () => {
    setLocale('en');
    const view = render();
    await live(view);
    const help = view.q('[data-tab-panel="help"]')!;
    expect(help.querySelector('h3')!.textContent).toBe('Live mode shortcuts');
    expect(help.querySelector('h4')!.textContent).toBe('In the projected window');
    expect(help.querySelector('dt[data-shortcut="pan"]')!.textContent).toContain('Shift + arrows');
    expect(help.querySelector('dt[data-shortcut="next"]')!.textContent).toContain('Page Down');
  });

  it('does not promise Suppr and Retour arrière without a selection: in this window they clear nothing', async () => {
    const view = render();
    await live(view);
    mockLiveTick(150);
    await tick();
    const strokes = view.canvas().dataset.strokes;
    expect(Number(strokes)).toBeGreaterThan(0);
    for (const key of ['Delete', 'Backspace']) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
      await tick();
      expect(view.canvas().dataset.strokes).toBe(strokes);
    }
  });
});
