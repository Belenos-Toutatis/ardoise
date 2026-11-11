// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M14, the independent review: in the live control window, « Annuler l'effacement » waits in the computer's
// queue and goes when a merge dropped the history; hiding a layer drops its strokes from the selection; Ctrl+Z right
// after the first stroke of a page undoes it; a merge is asked first (and says when it shows a hidden layer); a refused
// layer operation or stroke says so in its own words; the window going away mid-stroke cancels that stroke.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale, t } from '../../lib/i18n.svelte';
import type { LiveComputerPenInput, LiveSnapshot } from '../../lib/live';
import { FakeContext } from '../../lib/live-fake-canvas';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
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
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
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
  resetMockLive();
  vi.restoreAllMocks();
});

const PAGE = { width: 29700, height: 21600 };
const TRANSFORM = fitTransform(PAGE, { width: 800, height: 560 }, 12);
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, resume: null, onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`button[data-action="${name}"]`);
  const canvas = () => q<HTMLCanvasElement>('canvas.live-canvas')!;
  const zone = () => q('section.layers-zone[data-where="live"]')!;
  const row = (index: number) => zone().querySelector<HTMLLIElement>(`li.layer[data-layer="${index}"]`)!;
  const inRow = (index: number, name: string) => row(index).querySelector<HTMLButtonElement>(`[data-action="${name}"]`)!;
  const notice = () => [...target.querySelectorAll('.banner[role="alert"] p')].map((p) => p.textContent?.trim());
  const session = () => mockLiveInvoke('live_start', {}) as LiveSnapshot;
  const question = () => document.querySelector<HTMLElement>('[role="alertdialog"]');
  const fire = (type: string, p: { x: number; y: number }) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    const props = { pointerType: 'mouse', pointerId: 1, pressure: 0.5, timeStamp: performance.now() };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas().dispatchEvent(event);
  };
  /** A stroke of the computer across the page at height `y`. */
  const write = (y = 15000) => {
    fire('pointerdown', at(4000, y));
    for (let x = 5000; x <= 9000; x += 1000) fire('pointermove', at(x, y + (x % 2000)));
    fire('pointerup', at(9000, y));
  };
  const lasso = (x0: number, y0: number, x1: number, y1: number) => {
    const corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1), at(x0, y0 + 10)];
    fire('pointerdown', corners[0]);
    for (const p of corners.slice(1)) fire('pointermove', p);
    fire('pointerup', corners[4]);
  };
  return { target, q, action, canvas, zone, row, inRow, notice, session, question, fire, write, lasso };
}

type View = ReturnType<typeof render>;

async function live(view: View) {
  await vi.waitFor(() => expect(view.q('.live-view')!.dataset.state).toBe('live'));
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
}

/** The pen's `end` waits for `release()`; what reached the session is logged in `log`. */
function holdEnds(log: string[]) {
  const original = api.liveComputerPen;
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  const spy = vi.spyOn(api, 'liveComputerPen').mockImplementation(async (input: LiveComputerPenInput) => {
    if (input.kind === 'end') await held;
    const answer = await original(input);
    log.push(input.kind);
    return answer;
  });
  return { spy, release: () => release() };
}

/** Pen on, one stroke written and recorded. */
async function penStroke(view: View, y = 15000) {
  press('p');
  await tick();
  const before = view.session().page.strokes.length;
  view.write(y);
  await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(before + 1));
}

describe('« Annuler l\'effacement » (finding 1)', () => {
  it('waits in the queue behind the end of a stroke written before it', async () => {
    const view = render();
    await live(view);
    await penStroke(view);
    view.action('clear-page')!.click();
    await vi.waitFor(() => expect(view.q('[data-slot="undo-clear"]')).not.toBeNull());
    const log: string[] = [];
    const { release } = holdEnds(log);
    const original = api.liveRestoreCleared;
    const restore = vi.spyOn(api, 'liveRestoreCleared').mockImplementation(async () => {
      log.push('restore');
      return original();
    });
    view.write(18000);
    await vi.waitFor(() => expect(log).toContain('begin'));
    view.action('undo-clear')!.click();
    await new Promise((resolve) => setTimeout(resolve, 30));
    // The stroke's end has not been answered: the restore waits behind it.
    expect(restore).not.toHaveBeenCalled();
    release();
    await vi.waitFor(() => expect(restore).toHaveBeenCalledOnce());
    expect(log.slice(log.indexOf('end'))).toEqual(['end', 'restore']);
  });

  it('goes when a merge dropped the page\'s history', async () => {
    const view = render();
    await live(view);
    await penStroke(view);
    view.action('clear-page')!.click();
    await vi.waitFor(() => expect(view.q('[data-slot="undo-clear"]')).not.toBeNull());
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.zone().querySelectorAll('li.layer')).toHaveLength(2));
    expect(view.q('[data-slot="undo-clear"]')).not.toBeNull();
    view.inRow(1, 'layer-merge').click();
    await tick();
    document.querySelector<HTMLButtonElement>('[data-action="merge-confirm"]')!.click();
    await vi.waitFor(() => expect(view.zone().querySelectorAll('li.layer')).toHaveLength(1));
    await vi.waitFor(() => expect(view.q('[data-slot="undo-clear"]')).toBeNull());
  });
});

describe('the selection and a hidden layer (finding 3)', () => {
  it('hiding the layer of the chosen strokes drops them from the selection; hiding another keeps it', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.row(1).dataset.chosen).toBe('true'));
    await penStroke(view);
    press('v');
    await tick();
    view.lasso(3000, 13000, 10000, 20000);
    await vi.waitFor(() => expect(view.q('[data-slot="select-bar"]')).not.toBeNull());
    view.inRow(0, 'layer-visible').click();
    await vi.waitFor(() => expect(view.row(0).dataset.hidden).toBe('true'));
    await tick();
    expect(view.q('[data-slot="select-bar"]')).not.toBeNull();
    view.inRow(1, 'layer-visible').click();
    await vi.waitFor(() => expect(view.row(1).dataset.hidden).toBe('true'));
    await tick();
    expect(view.q('[data-slot="select-bar"]')).toBeNull();
    // Nothing is left to delete: Delete does what it does without a selection.
    press('Delete');
    await tick();
    expect(view.notice()).toEqual([]);
    expect(view.session().page.strokes).toHaveLength(1);
  });
});

describe('Ctrl+Z right after the first stroke of a page (finding 4)', () => {
  it('undoes it, though the session had not yet said there was something to undo', async () => {
    const view = render();
    await live(view);
    const log: string[] = [];
    const { release } = holdEnds(log);
    press('p');
    await tick();
    view.write();
    await vi.waitFor(() => expect(log).toContain('begin'));
    expect(view.action('undo-edit')!.disabled).toBe(true);
    press('z', { ctrlKey: true });
    release();
    await vi.waitFor(() => expect(log).toContain('end'));
    await vi.waitFor(() => expect(view.session().canRedo).toBe(true));
    expect(view.session().page.strokes).toHaveLength(0);
  });
});

describe('merging during live is asked first (finding 6)', () => {
  it('says Ctrl+Z cannot undo it; « Annuler », Esc and Ctrl+W keep both layers; « Fusionner » merges', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.row(1).dataset.chosen).toBe('true'));
    const ask = async () => {
      view.inRow(1, 'layer-merge').click();
      await tick();
      expect(view.question()).not.toBeNull();
    };
    await ask();
    expect(view.question()!.textContent).toContain(t('live.merge.title', { upper: 'Calque 2', lower: 'Calque 1' }));
    expect(view.question()!.textContent).toContain("Ctrl+Z ne pourra pas l'annuler.");
    expect(view.question()!.querySelector('[data-slot="merge-reveals"]')).toBeNull();
    document.querySelector<HTMLButtonElement>('[data-action="merge-cancel"]')!.click();
    await tick();
    expect(view.question()).toBeNull();
    for (const key of [{ key: 'Escape' }, { key: 'w', ctrlKey: true }]) {
      await ask();
      press(key.key, key);
      await tick();
      expect(view.question()).toBeNull();
      // The question answered the key: live mode was not asked to end.
      expect(document.querySelector('[data-action="leave-confirm"]')).toBeNull();
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(view.session().page.layers).toHaveLength(2);
    await ask();
    document.querySelector<HTMLButtonElement>('[data-action="merge-confirm"]')!.click();
    await vi.waitFor(() => expect(view.session().page.layers).toHaveLength(1));
  });

  it('says when the merge will show a hidden layer on the projector and the share', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.row(1).dataset.chosen).toBe('true'));
    view.inRow(0, 'layer-visible').click();
    await vi.waitFor(() => expect(view.row(0).dataset.hidden).toBe('true'));
    view.inRow(1, 'layer-merge').click();
    await tick();
    expect(view.question()!.querySelector('[data-slot="merge-reveals"]')!.textContent).toBe(
      '« Calque 1 » est masqué : une fois fusionné, son contenu sera projeté et partagé.',
    );
    document.querySelector<HTMLButtonElement>('[data-action="merge-confirm"]')!.click();
    await vi.waitFor(() => expect(view.session().page.layers).toEqual([{ name: 'Calque 1', visible: true }]));
  });

  it('speaks English', async () => {
    setLocale('en');
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.row(1).dataset.chosen).toBe('true'));
    view.inRow(1, 'layer-merge').click();
    await tick();
    expect(view.question()!.textContent).toContain('Merge “Calque 2” into “Calque 1”?');
    expect(view.question()!.textContent).toContain('Ctrl+Z will not be able to undo it.');
    expect(document.querySelector('[data-action="merge-confirm"]')!.textContent?.trim()).toBe('Merge');
  });
});

describe('refusals in their own words (finding 9)', () => {
  it('a layer operation on a page that changed does not ask to select again', async () => {
    const view = render();
    await live(view);
    vi.spyOn(api, 'liveLayers').mockRejectedValueOnce({ code: 'liveEditStale', message: 'stale' });
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.notice()).toEqual([t('live.failed.layersStale')]));
  });

  it('a stroke the session refuses says the stroke was not written', async () => {
    const view = render();
    await live(view);
    vi.spyOn(api, 'liveComputerPen').mockRejectedValueOnce({ code: 'liveEditInvalid', message: 'no sample' });
    press('p');
    await tick();
    view.write();
    await vi.waitFor(() => expect(view.notice()).toEqual([t('live.failed.pen')]));
  });
});

describe('the window going away mid-stroke (finding 7)', () => {
  it('cancels the stroke under way', async () => {
    const view = render();
    await live(view);
    const log: string[] = [];
    holdEnds(log);
    press('p');
    await tick();
    view.fire('pointerdown', at(4000, 15000));
    view.fire('pointermove', at(6000, 15000));
    await vi.waitFor(() => expect(log).toContain('begin'));
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(log).toContain('cancel'));
    expect(log).not.toContain('end');
    expect(view.session().page.strokes).toHaveLength(0);
  });
});
