// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveTick, resetMockLive } from '../../lib/mock-live';
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
};

let cleanup: (() => void) | null = null;
const contexts: FakeContext[] = [];

beforeEach(() => {
  resetMockLive();
  resetMockLibrary();
  setLocale('fr');
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
  contexts.length = 0;
  resetMockLive();
  vi.restoreAllMocks();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(LiveView, { target, props: { app, onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find(
      (b) => b.getAttribute('aria-label') === label || b.textContent?.trim() === label || b.title === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`)!;
  return { target, onclose, root, canvas, button, action };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
}

describe('live view (control window)', () => {
  it('starts live mode, waits for the tablet, then shows the page', async () => {
    mockLiveConfig.waitMs = 400;
    const view = render();
    expect(view.root().dataset.state).toBe('idle');
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    expect(view.target.textContent).toContain('Appuyez brièvement sur le bouton de la tablette');
    await live(view);
    expect(view.target.textContent).not.toContain('Appuyez brièvement');
    expect(view.canvas().dataset.page).toBe('1/1');
    expect(view.canvas().dataset.strokes).toBe('0');
    expect(view.target.querySelector('.page-position')?.textContent).toBe('Page 1 / 1');
  });

  it('draws what the pen writes and counts the strokes', async () => {
    const view = render();
    await live(view);
    mockLiveTick(120);
    await tick();
    const strokes = Number(view.canvas().dataset.strokes);
    expect(strokes).toBeGreaterThan(0);
    // The canvas painted the strokes (cache + frame).
    await vi.waitFor(() => expect(contexts.some((c) => c.count('fill') > 0)).toBe(true));
    // A saved page is counted.
    await vi.waitFor(() => expect(view.target.querySelector('[data-saved]')?.textContent).toBe('1 note enregistrée'));
  });

  it('changes the tool, the colour and the width', async () => {
    const view = render();
    await live(view);
    const select = view.target.querySelector<HTMLSelectElement>('.live-toolbar select')!;
    select.value = 'feltTip';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    const red = view.button('Rouge');
    red.click();
    await tick();
    expect(red.getAttribute('aria-pressed')).toBe('true');
    expect(select.value).toBe('feltTip');
    const slider = view.target.querySelector<HTMLInputElement>('.live-toolbar input[type="range"]')!;
    slider.value = '300';
    slider.dispatchEvent(new Event('input', { bubbles: true }));
    slider.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(view.target.textContent).toContain('3.0 mm');
    // The pen writes with it.
    mockLiveTick(120);
    const subscribed = await api.liveSubscribe(() => {});
    expect(subscribed.style).toEqual({ tool: 'feltTip', color: { r: 0xd8, g: 0, b: 0, a: 255 }, width: 300 });
  });

  it('turns pages, adds pages and layers, clears the page', async () => {
    const view = render();
    await live(view);
    view.button('Page suivante').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    view.button('Page précédente').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    view.action('new-page').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('3/3'));
    view.action('new-layer').click();
    await vi.waitFor(() => expect(view.action('new-layer').title).toBe('Calque 2 / 2'));
    mockLiveTick(150);
    await tick();
    expect(Number(view.canvas().dataset.strokes)).toBeGreaterThan(0);
    view.action('clear-page').click();
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('0'));
  });

  it('switches the blackboard and the laser pointer, zooms the projected area', async () => {
    const view = render();
    await live(view);
    view.action('board').click();
    await tick();
    expect(view.action('board').getAttribute('aria-pressed')).toBe('true');
    // The blackboard writes in white chalk.
    await vi.waitFor(() => expect(view.button('Blanc craie').getAttribute('aria-pressed')).toBe('true'));
    view.action('laser').click();
    await tick();
    expect(view.action('laser').getAttribute('aria-pressed')).toBe('true');
    const level = () => view.target.querySelector<HTMLElement>('[data-zoom]')!;
    view.button('Agrandir').click();
    await tick();
    expect(Number(level().dataset.zoom)).toBeCloseTo(1.25, 6);
    expect(level().textContent?.trim()).toBe('× 1,3');
    view.button('Agrandir').click();
    view.button('Réduire').click();
    await tick();
    expect(Number(level().dataset.zoom)).toBeCloseTo(1.25, 6);
    view.action('whole-page').click();
    await tick();
    expect(Number(level().dataset.zoom)).toBe(1);
    expect(view.button('Réduire').disabled).toBe(true);
  });

  it('moves the projected area by dragging in the preview', async () => {
    const view = render();
    await live(view);
    view.button('Agrandir').click();
    view.button('Agrandir').click();
    view.button('Agrandir').click();
    await tick();
    const canvas = view.canvas();
    const pointer = (type: string, x: number, y: number) =>
      canvas.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0 }) as PointerEvent);
    const sent = vi.spyOn(api, 'liveSetView');
    // The preview is 800 × 560 in jsdom; the area sits on the page's centre.
    pointer('pointerdown', 400, 280);
    pointer('pointermove', 300, 220);
    pointer('pointerup', 300, 220);
    await vi.waitFor(() => expect(sent).toHaveBeenCalled());
    const last = sent.mock.calls.at(-1)![0];
    expect(last.x).toBeLessThan(29700 / 2);
    expect(last.y).toBeLessThan(21600 / 2);
  });

  it('chooses a screen and projects, then stops projecting', async () => {
    const view = render();
    await live(view);
    const select = () => view.target.querySelector<HTMLSelectElement>('.live-side select')!;
    await vi.waitFor(() => expect(select().options).toHaveLength(2));
    expect([...select().options].map((o) => o.textContent?.trim())).toEqual([
      'Écran 1 (principal) — 1920 × 1080',
      'Écran 2 — 1280 × 800',
    ]);
    // The projector is offered first.
    expect(select().selectedIndex).toBe(1);
    view.action('project').click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeTruthy());
    expect(view.target.textContent).toContain('Projeté sur Écran 2.');
    view.action('stop-projecting').click();
    await vi.waitFor(() => expect(view.action('project')).toBeTruthy());
    expect(view.target.textContent).toContain("Rien n'est projeté.");
  });

  it('says when there is a single screen or none', async () => {
    mockLiveConfig.monitors = 1;
    const view = render();
    await vi.waitFor(() => expect(view.target.textContent).toContain('Un seul écran est détecté'));
    cleanup?.();
    mockLiveConfig.monitors = 0;
    const none = render();
    await vi.waitFor(() => expect(none.target.textContent).toContain("Aucun écran n'est détecté."));
    expect(none.action('project').disabled).toBe(true);
  });

  it('shows the measured latency and exposes it to the end-to-end journey', async () => {
    const view = render();
    await live(view);
    mockLiveTick(60);
    await vi.waitFor(() => expect(window.__ardoiseLatency?.samples().length ?? 0).toBeGreaterThan(0), { timeout: 2000 });
    const median = window.__ardoiseLatency!.median()!;
    expect(median).toBeGreaterThanOrEqual(0);
    expect(median).toBeLessThan(1000);
    await vi.waitFor(() => expect(view.target.querySelector<HTMLElement>('[data-latency-median]')!.dataset.latencyMedian).not.toBe(''), {
      timeout: 2000,
    });
    const shown = view.target.querySelector<HTMLElement>('[data-latency-median]')!;
    expect(shown.dataset.latencyMedian).toMatch(/^-?\d+\.\d$/);
    expect(shown.textContent).toMatch(/^-?\d+,\d ms$/);
    // The tooltip adds the p95 and the interface's share.
    const tooltip = view.target.querySelector<HTMLElement>('.latency')!.title;
    expect(tooltip).toMatch(/95 % sous -?\d+,\d ms ; part de l'interface \(de l'arrivée du lot à la fin du dessin\) : -?\d+,\d ms\.$/);
    expect(tooltip).not.toContain('ms ms');
    // The hint says what is measured: up to the frame after the drawing.
    expect(tooltip).toContain("réception du point par l'ordinateur et le début de l'image suivante");
  });

  it('works from the keyboard', async () => {
    const view = render();
    await live(view);
    const press = (key: string, init: KeyboardEventInit = {}, target: EventTarget = window) =>
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
    press('ArrowRight');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    press('PageUp');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    press('b');
    await tick();
    expect(view.action('board').getAttribute('aria-pressed')).toBe('true');
    press('l');
    await tick();
    expect(view.action('laser').getAttribute('aria-pressed')).toBe('true');
    press('+');
    await tick();
    expect(Number(view.target.querySelector<HTMLElement>('[data-zoom]')!.dataset.zoom)).toBeCloseTo(1.25, 6);
    press('0');
    await tick();
    expect(Number(view.target.querySelector<HTMLElement>('[data-zoom]')!.dataset.zoom)).toBe(1);
    // A key typed in a field is the field's.
    const slider = view.target.querySelector<HTMLInputElement>('.live-toolbar input[type="range"]')!;
    press('ArrowRight', {}, slider);
    await tick();
    expect(view.canvas().dataset.page).toBe('1/2');
    mockLiveTick(150);
    await tick();
    press('Delete', { ctrlKey: true });
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('0'));
    press('Escape');
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
  });

  it('finishes: stops live mode, closes the presentation, and the pages are notes', async () => {
    const view = render();
    await live(view);
    const before = mockLibrary().notes.size;
    mockLiveTick(150);
    view.action('project').click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeTruthy());
    const closing = vi.spyOn(api, 'closePresentation');
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
    expect(closing).toHaveBeenCalled();
    expect(mockLibrary().notes.size).toBe(before + 1);
    // Live mode is over in Rust (the stand-in) too.
    expect((await api.liveSubscribe(() => {})).state).toBe('idle');
  });

  it('reports a start failure and tries again', async () => {
    vi.spyOn(api, 'liveStart').mockRejectedValueOnce({ code: 'tabletNotRegistered', message: '' });
    const view = render();
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct n'a pas pu démarrer."));
    expect(view.target.textContent).toContain("Aucune tablette n'est enregistrée");
    expect(view.root().dataset.state).toBe('idle');
    view.button('Réessayer').click();
    await live(view);
    expect(view.target.textContent).not.toContain("Le mode direct n'a pas pu démarrer.");
  });

  it('keeps the window open when stopping fails, and can close anyway', async () => {
    const view = render();
    await live(view);
    vi.spyOn(api, 'liveStop').mockRejectedValueOnce({ code: 'tablet', message: 'link lost' });
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct ne s'est pas arrêté correctement."));
    expect(view.onclose).not.toHaveBeenCalled();
    view.button('Fermer quand même').click();
    expect(view.onclose).toHaveBeenCalledTimes(1);
  });
});
