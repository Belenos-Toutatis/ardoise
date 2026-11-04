// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M12: touching up during live mode in the control window: the Select tool, the bar of the
// selection, delete and recolour, undo and redo, the keys, and what drops the selection. The session is
// the browser stand-in of Rust (a resumed sheet of two strokes, to which the "tablet" adds more).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale, t } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { SELECTION_COLORS } from '../../lib/live-render';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveTick, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import LiveView from './LiveView.svelte';
import Presentation from './Presentation.svelte';

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
};

let cleanup: (() => void) | null = null;
let contexts: FakeContext[] = [];

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  tablet.reset();
  setLocale('fr');
  contexts = [];
  // jsdom has no canvas: every canvas draws on a recording fake.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
    const ctx = new FakeContext();
    contexts.push(ctx);
    return ctx as unknown as RenderingContext;
  });
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];
/** The sheet's two strokes are waves at y = 3000 and y = 6000, from x = 2000. */
const PAGE = { width: 29700, height: 21600 };
const TRANSFORM = fitTransform(PAGE, { width: 800, height: 560 }, 12);
/** Where a point of the page is on the preview (the jsdom canvas has no offset). */
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

interface Init {
  pointerType?: string;
  shiftKey?: boolean;
  ctrlKey?: boolean;
}

function render(resume: string | null = sheetPath()) {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(LiveView, { target, props: { app, resume, onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
  const bar = () => target.querySelector<HTMLElement>('[data-slot="select-bar"]');
  const strokes = () => Number(canvas().dataset.strokes);
  const swatch = (label: string) => target.querySelector<HTMLButtonElement>(`button.swatch[aria-label="${label}"]`)!;
  const fire = (type: string, p: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: p.x,
      clientY: p.y,
      button: 0,
      buttons: type === 'pointerup' ? 0 : 1,
      shiftKey: init.shiftKey,
      ctrlKey: init.ctrlKey,
    });
    const props: Record<string, unknown> = { pointerType: init.pointerType ?? 'mouse', pointerId: 1, pressure: 0.5, timeStamp: 0 };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas().dispatchEvent(event);
  };
  const lasso = (x0: number, y0: number, x1: number, y1: number, init: Init = {}) => {
    const corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    fire('pointerdown', corners[0], init);
    for (const corner of corners.slice(1)) fire('pointermove', corner, init);
    fire('pointermove', at(x0, y0 + 10), init);
    fire('pointerup', at(x0, y0 + 10), init);
  };
  const click = (x: number, y: number, init: Init = {}) => {
    fire('pointerdown', at(x, y), init);
    fire('pointerup', at(x, y), init);
  };
  /** A lasso round the first stroke (the wave at y = 3000), the second one (y = 6000), both. */
  const first = (init: Init = {}) => lasso(1500, 1800, 22500, 4200, init);
  const second = (init: Init = {}) => lasso(1500, 5000, 20500, 7000, init);
  const both = () => lasso(1500, 1800, 22500, 7000);
  return { target, onclose, root, canvas, action, bar, strokes, swatch, fire, lasso, click, first, second, both };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
}

/** The view with the Select tool on and the first stroke chosen. */
async function selected() {
  const view = render();
  await live(view);
  press('v');
  await tick();
  view.first();
  await vi.waitFor(() => expect(view.bar()).not.toBeNull());
  return view;
}

describe('the Select tool', () => {
  it('is a toggle of the toolbar, off at first, with the editor\'s name and the V key', async () => {
    const view = render();
    // Nothing to touch up before the session runs.
    expect(view.action('select')!.disabled).toBe(true);
    await live(view);
    const toggle = view.action('select')!;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent?.trim()).toBe('Sélection');
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(view.canvas().classList.contains('selecting')).toBe(false);

    toggle.click();
    await tick();
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(view.canvas().classList.contains('selecting')).toBe(true);
    expect(view.target.querySelector('[data-slot="select-hint"]')?.textContent).toContain('Lasso');

    expect(press('v').defaultPrevented).toBe(true);
    await tick();
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(view.target.querySelector('[data-slot="select-hint"]')).toBeNull();
  });

  it('speaks English with the same words as the editor', async () => {
    setLocale('en');
    const view = render();
    await live(view);
    expect(view.action('select')!.textContent?.trim()).toBe('Select');
    expect(view.action('undo-edit')!.textContent?.trim()).toBe('Undo');
    expect(view.action('redo-edit')!.textContent?.trim()).toBe('Redo');
    press('v');
    await tick();
    view.first();
    await vi.waitFor(() => expect(view.bar()).not.toBeNull());
    expect(view.bar()!.textContent).toContain('Selected strokes: 1');
    expect(view.action('delete-selection')!.textContent?.trim()).toBe('Delete');
    expect(view.action('deselect')!.textContent?.trim()).toBe('Deselect');
    expect(view.bar()!.getAttribute('aria-label')).toBe('Touch up the selection');
    expect(view.target.querySelector('[data-slot="select-hint"]')?.textContent).toContain('Lasso: select');
  });

  it('chooses with a lasso or a click, shows the bar, and turning the tool off drops the selection', async () => {
    const view = render();
    await live(view);
    view.action('select')!.click();
    await tick();
    expect(view.bar()).toBeNull();

    view.second();
    await vi.waitFor(() => expect(view.bar()).not.toBeNull());
    expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1');
    view.click(2000, 3000, { shiftKey: true });
    await vi.waitFor(() => expect(view.bar()!.textContent).toContain('Traits sélectionnés : 2'));
    view.click(2000, 3000, { ctrlKey: true });
    await vi.waitFor(() => expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1'));
    // A click on nothing clears.
    view.click(25000, 18000);
    await vi.waitFor(() => expect(view.bar()).toBeNull());

    view.both();
    await vi.waitFor(() => expect(view.bar()!.textContent).toContain('Traits sélectionnés : 2'));
    view.action('select')!.click();
    await vi.waitFor(() => expect(view.bar()).toBeNull());
  });

  it('draws the highlight on the preview of this window only: nothing is sent anywhere', async () => {
    const sent = [vi.spyOn(api, 'liveEditStrokes'), vi.spyOn(api, 'liveCommand'), vi.spyOn(api, 'liveSetView'), vi.spyOn(api, 'liveSetStyle')];
    const view = render();
    await live(view);
    view.action('select')!.click();
    await tick();
    view.first();
    await vi.waitFor(() =>
      expect(contexts.some((c) => c.ops('stroke').length >= 2 && c.count('strokeRect') === 1 && c.count('fillRect') === 1)).toBe(true),
    );
    expect(sent.map((spy) => spy.mock.calls.length)).toEqual([0, 0, 0, 0]);
  });
});

describe('deleting', () => {
  it('deletes the selection with the button; the strokes go, the bar goes, undo comes', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const view = await selected();
    expect(view.strokes()).toBe(2);
    expect(view.action('undo-edit')!.disabled).toBe(true);
    view.action('delete-selection')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
    expect(edit).toHaveBeenCalledExactlyOnceWith(0, expect.any(Number), { kind: 'delete', indices: [0] });
    await vi.waitFor(() => expect(view.bar()).toBeNull());
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
    expect(view.action('redo-edit')!.disabled).toBe(true);
  });

  it.each(['Delete', 'Backspace'])('deletes the selection with %s', async (key) => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const view = await selected();
    const event = press(key);
    expect(event.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
    expect(edit).toHaveBeenCalledExactlyOnceWith(0, expect.any(Number), { kind: 'delete', indices: [0] });
  });

  it('keeps clearing the marks without a selection, and Ctrl+Delete the page', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const command = vi.spyOn(api, 'liveCommand');
    const view = render();
    await live(view);
    press('v');
    await tick();
    // The tool is on but nothing is chosen: the key does what it did (the control window has no marks).
    press('Delete');
    press('Backspace');
    expect(edit).not.toHaveBeenCalled();
    expect(view.strokes()).toBe(2);
    expect(press('Delete', { ctrlKey: true }).defaultPrevented).toBe(true);
    expect(command).toHaveBeenCalledWith({ kind: 'clearPage' });
  });
});

describe('recolouring', () => {
  it('a swatch recolours the selection, which stays, and the pen keeps its colour', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const style = vi.spyOn(api, 'liveSetStyle');
    const view = await selected();
    view.swatch(t('editor.colors.red')).click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    const [page, , change] = edit.mock.calls[0];
    expect(page).toBe(0);
    expect(change).toMatchObject({ kind: 'recolour', indices: [0], color: { r: 216, g: 0, b: 0, a: 255 } });
    // The ink was redrawn in the new colour; the selection is still there and nothing was deleted.
    await vi.waitFor(() => expect(contexts.some((c) => c.ops('fill').some((f) => f.fillStyle === 'rgb(216, 0, 0)'))).toBe(true));
    expect(view.strokes()).toBe(2);
    expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1');
    expect(style).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));

    // Another colour on the same selection.
    view.swatch(t('editor.colors.blue')).click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledTimes(2));
    expect(edit.mock.calls[1][1]).toBe(edit.mock.calls[0][1]);
  });

  it('without a selection a swatch only sets the pen, as before', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const style = vi.spyOn(api, 'liveSetStyle');
    const view = render();
    await live(view);
    press('v');
    await tick();
    view.swatch(t('editor.colors.red')).click();
    expect(style).toHaveBeenCalledOnce();
    expect(style.mock.calls[0][0].color).toMatchObject({ r: 216, g: 0, b: 0 });
    expect(edit).not.toHaveBeenCalled();
  });
});

describe('Esc', () => {
  it('deselects first, then does what it did: it asks before ending live mode', async () => {
    const view = await selected();
    expect(press('Escape').defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.bar()).toBeNull());
    expect(view.target.querySelector('[data-action="leave-confirm"]')).toBeNull();
    // The Select tool stays on; the second press leaves.
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('true');
    press('Escape');
    await vi.waitFor(() => expect(document.querySelector('[data-action="leave-confirm"]')).not.toBeNull());
  });

  it('deselects before it closes the projection, which then closes as before', async () => {
    const view = render();
    await live(view);
    await vi.waitFor(() => expect(view.action('project')!.disabled).toBe(false));
    view.action('project')!.click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).not.toBeNull());
    press('v');
    await tick();
    view.first();
    await vi.waitFor(() => expect(view.bar()).not.toBeNull());

    press('Escape');
    await vi.waitFor(() => expect(view.bar()).toBeNull());
    expect(view.action('stop-projecting')).not.toBeNull();
    press('Escape');
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeNull());
  });
});

describe('undo and redo', () => {
  it('have buttons that follow what the session can undo and redo', async () => {
    const view = await selected();
    expect([view.action('undo-edit')!.disabled, view.action('redo-edit')!.disabled]).toEqual([true, true]);
    view.action('delete-selection')!.click();
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
    expect(view.action('redo-edit')!.disabled).toBe(true);

    view.action('undo-edit')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    await vi.waitFor(() => expect(view.action('redo-edit')!.disabled).toBe(false));
    expect(view.action('undo-edit')!.disabled).toBe(true);

    view.action('redo-edit')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
  });

  it('answer to Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z', async () => {
    const view = await selected();
    press('Delete');
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
    expect(press('z', { ctrlKey: true }).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    press('y', { ctrlKey: true });
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
    press('Z', { ctrlKey: true, shiftKey: true });
    // Nothing left to redo: it stays as it is, and undo works again.
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
    press('z', { ctrlKey: true });
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    press('Z', { ctrlKey: true, shiftKey: true });
    await vi.waitFor(() => expect(view.strokes()).toBe(1));
  });

  it('do not ask the session for a step it does not have', async () => {
    const undo = vi.spyOn(api, 'liveUndoEdit');
    const redo = vi.spyOn(api, 'liveRedoEdit');
    const view = render();
    await live(view);
    press('z', { ctrlKey: true });
    press('y', { ctrlKey: true });
    expect([undo, redo].map((spy) => spy.mock.calls.length)).toEqual([0, 0]);
  });

  it('undo « Clear the page » too, and the offer to undo the clearing keeps working', async () => {
    const view = render();
    await live(view);
    expect(view.strokes()).toBe(2);
    view.action('clear-page')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(0));
    await vi.waitFor(() => expect(view.target.querySelector('[data-slot="undo-clear"]')).not.toBeNull());
    // The offer goes through the stack...
    view.action('undo-clear')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    expect(view.target.querySelector('[data-slot="undo-clear"]')).toBeNull();

    // ... and so does Ctrl+Z, which makes the offer go away.
    view.action('clear-page')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(0));
    await vi.waitFor(() => expect(view.target.querySelector('[data-slot="undo-clear"]')).not.toBeNull());
    press('z', { ctrlKey: true });
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    expect(view.target.querySelector('[data-slot="undo-clear"]')).toBeNull();
  });
});

describe('what drops the selection', () => {
  it('keeps it while the tablet writes: the indices stay right', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const view = await selected();
    // The tablet adds strokes after the two of the sheet (and one is in progress at times).
    mockLiveTick(120);
    await tick();
    expect(view.strokes()).toBeGreaterThan(2);
    expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1');
    view.swatch(t('editor.colors.red')).click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    expect(edit.mock.calls[0][2]).toMatchObject({ kind: 'recolour', indices: [0] });
    mockLiveTick(120);
    await tick();
    expect(view.bar()).not.toBeNull();
    view.action('delete-selection')!.click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledTimes(2));
    expect(edit.mock.calls[1][2]).toEqual({ kind: 'delete', indices: [0] });
    expect(edit.mock.calls[1][1]).toBe(edit.mock.calls[0][1]);
  });

  it('drops it when the page turns', async () => {
    const view = await selected();
    view.action('new-page')!.click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    expect(view.bar()).toBeNull();
    // Back on the first page the old choice does not come back.
    press('ArrowLeft');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    expect(view.bar()).toBeNull();
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('true');
  });

  it('drops it when another action changes the revision (here « Clear the page »), and when an undo does', async () => {
    const view = await selected();
    view.action('clear-page')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(0));
    expect(view.bar()).toBeNull();

    view.action('undo-edit')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(2));
    view.first();
    await vi.waitFor(() => expect(view.bar()).not.toBeNull());
    // An undo of the clearing... is another step; a delete made elsewhere moves the indices.
    mockLiveInvoke('live_command', { command: { kind: 'clearPage' } });
    await vi.waitFor(() => expect(view.strokes()).toBe(0));
    expect(view.bar()).toBeNull();
  });

  it.each([
    ['fr', 'La page a changé entre-temps : refaites la sélection.'],
    ['en', 'The page changed in the meantime: select again.'],
  ] as const)('a stale edit drops it and says so in the translated words (%s)', async (locale, sentence) => {
    setLocale(locale);
    const view = render();
    await live(view);
    press('v');
    await tick();
    view.first();
    await vi.waitFor(() => expect(view.bar()).not.toBeNull());
    vi.spyOn(api, 'liveEditStrokes').mockRejectedValueOnce({ code: 'liveEditStale', message: 'stale' });
    view.action('delete-selection')!.click();
    await vi.waitFor(() => expect(view.bar()).toBeNull());
    const alert = view.target.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain(sentence);
    expect(view.strokes()).toBe(2);
  });

  it('keeps the selection of an edit that is refused for another reason, and says why', async () => {
    const view = await selected();
    vi.spyOn(api, 'liveEditStrokes').mockRejectedValueOnce({ code: 'liveEditInvalid', message: 'invalid' });
    view.swatch(t('editor.colors.red')).click();
    await vi.waitFor(() => expect(view.target.querySelector('[role="alert"]')?.textContent).toContain("Cette retouche n'est pas possible"));
    expect(view.bar()).not.toBeNull();
  });
});

describe('the blackboard', () => {
  it('shows the selection in a light colour on the dark paper, and edits as on any page', async () => {
    const view = render();
    await live(view);
    press('b');
    await tick();
    press('v');
    await tick();
    view.both();
    await vi.waitFor(() => expect(view.bar()!.textContent).toContain('Traits sélectionnés : 2'));
    // On the dark paper the highlight uses its light colour.
    await vi.waitFor(() =>
      expect(contexts.some((c) => c.ops('stroke').some((call) => call.strokeStyle === SELECTION_COLORS.dark.halo))).toBe(true),
    );
    view.action('delete-selection')!.click();
    await vi.waitFor(() => expect(view.strokes()).toBe(0));
  });
});

describe('the Select tool once the session is over', () => {
  /** Live mode ends for real while the view stays open (a notice is waiting): the session is idle. */
  async function overWithSelection() {
    const view = await selected();
    const real = api.liveStop;
    vi.spyOn(api, 'liveStop').mockImplementationOnce(async () => ({
      ...(await real()),
      forks: [{ pageIndex: 0, reason: 'changed', original: 'a.ardoise', path: 'a (2).ardoise' }],
    }));
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('idle'));
    await tick();
    return view;
  }

  it('turns the tool off, and the selection with it', async () => {
    const view = await overWithSelection();
    expect(view.action('select')!.disabled).toBe(true);
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('false');
    expect(view.canvas().classList.contains('selecting')).toBe(false);
    expect(view.bar()).toBeNull();
    expect(view.target.querySelector('[data-slot="select-hint"]')).toBeNull();
  });

  it('leaves V, Delete and the undo keys alone, as the button is disabled', async () => {
    const liveEdit = vi.spyOn(api, 'liveEditStrokes');
    const undo = vi.spyOn(api, 'liveUndoEdit');
    const view = await overWithSelection();
    expect(press('v').defaultPrevented).toBe(false);
    expect(press('z', { ctrlKey: true }).defaultPrevented).toBe(false);
    await tick();
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('false');
    expect(view.canvas().classList.contains('selecting')).toBe(false);
    // Nothing is asked of a session that is over: no « live mode is not running » in red.
    press('Delete');
    await tick();
    expect(liveEdit).not.toHaveBeenCalled();
    expect(undo).not.toHaveBeenCalled();
    expect(view.target.querySelector('.banner.error')).toBeNull();
  });

  it('does not turn the tool on before the first snapshot either', () => {
    const view = render();
    expect(view.action('select')!.disabled).toBe(true);
    expect(press('v').defaultPrevented).toBe(false);
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('false');
  });
});

describe('the presentation window', () => {
  it('has no selection tool and its keys are what they were', async () => {
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(Presentation, { target });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    mockLiveInvoke('live_start', {});
    const main = target.querySelector<HTMLElement>('main.presentation')!;
    await vi.waitFor(() => expect(main.dataset.state).toBe('live'));
    expect(target.querySelector('[data-action="select"], [data-slot="select-bar"], [data-action="undo-edit"]')).toBeNull();
    expect(target.querySelector('canvas.live-canvas')!.classList.contains('selecting')).toBe(false);
    // V and Ctrl+Z mean nothing there (the event is left alone), Delete still clears the marks.
    expect(press('v').defaultPrevented).toBe(false);
    expect(press('z', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press('Delete').defaultPrevented).toBe(true);
  });
});
