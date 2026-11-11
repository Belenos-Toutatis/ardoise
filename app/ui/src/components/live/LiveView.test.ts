// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveTick, resetMockLive } from '../../lib/mock-live';
import { mockNarration, resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import { quit } from '../../lib/editor/window-close';
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
const contexts: FakeContext[] = [];

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
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
  const onchange = vi.fn();
  const component = mount(LiveView, { target, props: { app, onclose, onchange } });
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
  return { target, onclose, onchange, root, canvas, button, action };
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

  it('switches the tablet position at once: the page in progress turns with its ink', async () => {
    const view = render();
    await live(view);
    const positions = () => [...view.target.querySelectorAll<HTMLButtonElement>('[data-slot="orientation"] button')];
    const pressed = () => positions().map((b) => b.getAttribute('aria-pressed'));
    expect(positions().map((b) => b.dataset.orientation)).toEqual([
      'landscape',
      'landscapeFlipped',
      'portraitLeft',
      'portraitRight',
    ]);
    expect(positions().map((b) => b.getAttribute('aria-label'))).toEqual([
      'Paysage, bouton en bas',
      "Paysage à l'envers, bouton en haut",
      'Portrait, bouton à gauche',
      'Portrait, bouton à droite',
    ]);
    expect(pressed()).toEqual(['true', 'false', 'false', 'false']);

    mockLiveConfig.pen = true;
    mockLiveTick(150);
    await tick();
    const strokes = Number(view.canvas().dataset.strokes);
    expect(strokes).toBeGreaterThan(0);
    // The page of the session, as written in the library.
    const written = async () => {
      const saved = (await api.liveSubscribe(() => {})).saved;
      return mockLibrary().notes.get(saved[0])?.note.page;
    };
    await vi.waitFor(async () => expect(await written()).toEqual({ width: 29700, height: 21600 }));

    view.button('Portrait, bouton à gauche').click();
    await vi.waitFor(() => expect(pressed()).toEqual(['false', 'false', 'true', 'false']));
    // The same strokes (the one in progress ended) on a portrait page, saved so.
    expect(Number(view.canvas().dataset.strokes)).toBeGreaterThanOrEqual(strokes);
    await vi.waitFor(async () => expect(await written()).toEqual({ width: 21600, height: 29700 }));
    expect((await api.tabletStatus()).options.orientation).toBe('portraitLeft');
    expect((await api.liveSubscribe(() => {})).orientation).toBe('portraitLeft');

    // Back to landscape; a refused switch is reported and changes nothing.
    view.button('Paysage, bouton en bas').click();
    await vi.waitFor(() => expect(pressed()[0]).toBe('true'));
    vi.spyOn(api, 'tabletSetOrientation').mockRejectedValue({ code: 'tablet', message: 'no tablet' });
    view.button('Portrait, bouton à droite').click();
    await vi.waitFor(() => expect(view.target.textContent).toContain('no tablet'));
    expect(pressed()[0]).toBe('true');
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
    // The tablet's « new layer » (its button, or the command): the tablet goes there, the computer stays (M14).
    expect(view.action('new-layer')).toBeNull();
    mockLiveInvoke('live_command', { command: { kind: 'newLayer' } });
    await vi.waitFor(() => expect(view.target.querySelectorAll('li.layer')).toHaveLength(2));
    expect(view.target.querySelector('li.layer[data-tablet="true"]')!.getAttribute('data-layer')).toBe('1');
    expect(view.target.querySelector('li.layer[data-chosen="true"]')!.getAttribute('data-layer')).toBe('0');
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
    const level = () => view.target.querySelector<HTMLElement>('[data-section="zone"] [data-zoom]')!;
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

  it('shows the page width from the button and from the W key (M8)', async () => {
    const view = render();
    await live(view);
    const level = () => view.target.querySelector<HTMLElement>('[data-section="zone"] [data-zoom]')!;
    expect(Number(level().dataset.zoom)).toBe(1);
    view.action('page-width').click();
    await tick();
    const wide = Number(level().dataset.zoom);
    // The demo page is taller than the preview's shape allows: the width fit zooms in.
    expect(wide).toBeGreaterThanOrEqual(1);
    view.action('whole-page').click();
    await tick();
    expect(Number(level().dataset.zoom)).toBe(1);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', bubbles: true, cancelable: true }));
    await tick();
    expect(Number(level().dataset.zoom)).toBeCloseTo(wide, 6);
    expect(view.target.textContent).toContain('Largeur de la page');
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
    const select = () => view.target.querySelector<HTMLSelectElement>('.live-side [data-tab-panel="projection"] select')!;
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

  it('shows a banner when a page cannot be saved, until a save succeeds', async () => {
    mockLiveConfig.saveError = 'Accès refusé (os error 5)';
    const view = render();
    await live(view);
    expect(view.target.querySelector('[data-slot="save-failed"]')).toBeNull();
    mockLiveTick(120);
    await tick();
    const banner = () => view.target.querySelector<HTMLElement>('[data-slot="save-failed"]');
    await vi.waitFor(() => expect(banner()).not.toBeNull());
    expect(banner()!.getAttribute('role')).toBe('alert');
    const text = banner()!.textContent!;
    expect(text).toContain("Une page n'a pas pu être enregistrée dans la bibliothèque.");
    // The code says it in words; the system's detail stays in brackets.
    expect(text).toContain('Page 1 : la bibliothèque ne peut pas être lue ou modifiée (Accès refusé (os error 5))');
    expect(text).toContain('Les pages restent en mémoire');
    expect(text).toContain('vérifiez le dossier de la bibliothèque');
    expect(text).toContain("une copie de secours est écrite dans le dossier local d'Ardoise");
    // It cannot be dismissed by mistake: nothing is saved while it shows.
    expect(banner()!.querySelector('button')).toBeNull();
    expect(view.target.querySelector('[data-saved]')?.textContent).toBe('Aucune note enregistrée pour l\'instant');

    // The next page fails as well: two pages are listed.
    view.button('Page suivante').click();
    await tick();
    mockLiveTick(120);
    await tick();
    await vi.waitFor(() => expect(banner()!.textContent).toContain('2 pages n'));
    expect(banner()!.textContent).toContain('Page 2 : la bibliothèque ne peut pas être lue ou modifiée');

    // The folder is fixed: the next save writes both pages and the banner goes away.
    mockLiveConfig.saveError = null;
    view.button('Page précédente').click();
    await vi.waitFor(() => expect(banner()).toBeNull());
    await vi.waitFor(() => expect(view.target.querySelector('[data-saved]')?.textContent).toBe('2 notes enregistrées'));
  });

  it('says it in English', async () => {
    setLocale('en');
    mockLiveConfig.saveError = 'disk full';
    const view = render();
    await live(view);
    mockLiveTick(120);
    await tick();
    await vi.waitFor(() => expect(view.target.querySelector('[data-slot="save-failed"]')).not.toBeNull());
    const text = view.target.querySelector('[data-slot="save-failed"]')!.textContent!;
    expect(text).toContain('A page could not be saved to the library.');
    expect(text).toContain('Page 1: the library cannot be read or changed (disk full)');
    expect(text).toContain('The pages stay in memory');
    expect(text).toContain("a backup copy is written to Ardoise's local folder");
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
    expect(Number(view.target.querySelector<HTMLElement>('[data-section="zone"] [data-zoom]')!.dataset.zoom)).toBeCloseTo(1.25, 6);
    press('0');
    await tick();
    expect(Number(view.target.querySelector<HTMLElement>('[data-section="zone"] [data-zoom]')!.dataset.zoom)).toBe(1);
    // A key typed in a field is the field's.
    const slider = view.target.querySelector<HTMLInputElement>('.live-toolbar input[type="range"]')!;
    press('ArrowRight', {}, slider);
    await tick();
    expect(view.canvas().dataset.page).toBe('1/2');
    mockLiveTick(150);
    await tick();
    press('Delete', { ctrlKey: true });
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('0'));
    // Esc asks before ending live mode (nothing is projected here).
    press('Escape');
    await vi.waitFor(() => expect(document.querySelector('[role="alertdialog"], [role="dialog"]')).toBeTruthy());
    expect(view.onclose).not.toHaveBeenCalled();
  });

  const press = (key: string, init: KeyboardEventInit = {}, target: EventTarget = window) =>
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');
  const dialogButton = (label: string) =>
    [...(dialog()?.querySelectorAll('button') ?? [])].find((b) => b.textContent?.trim() === label)!;

  it('Ctrl + W and Ctrl + F4 do what Esc does', async () => {
    const view = render();
    await live(view);
    view.action('project').click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeTruthy());
    press('w', { ctrlKey: true });
    await vi.waitFor(() => expect(view.action('project')).toBeTruthy());
    expect(dialog()).toBeNull();

    press('F4', { ctrlKey: true });
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    expect(dialog()!.textContent).toContain('Terminer le mode direct ?');
    // Under the question they answer « Continuer », as Esc does.
    press('w', { ctrlKey: true });
    await vi.waitFor(() => expect(dialog()).toBeNull());
    expect(view.onclose).not.toHaveBeenCalled();
  });

  it('Ctrl + Q asks before quitting, and keys do not reach the view behind the question', async () => {
    const view = render();
    await live(view);
    quit();
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    expect(dialog()!.textContent).toContain('Quitter Ardoise ?');
    expect(document.activeElement).toBe(dialogButton('Continuer'));
    press(' ');
    await tick();
    expect(view.canvas().dataset.page).toBe('1/1');
    dialogButton('Continuer').click();
    await vi.waitFor(() => expect(dialog()).toBeNull());
    expect(view.onclose).not.toHaveBeenCalled();
  });

  it('Esc closes the projection when it is open, and otherwise asks before ending live mode', async () => {
    const view = render();
    await live(view);
    view.action('project').click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeTruthy());
    press('Escape');
    await vi.waitFor(() => expect(view.action('project')).toBeTruthy());
    // Live mode itself goes on.
    expect(dialog()).toBeNull();
    expect(view.onclose).not.toHaveBeenCalled();

    // Nothing projected: a question, answered « Continuer » first (the focused answer).
    press('Escape');
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    expect(dialog()!.textContent).toContain('Terminer le mode direct ?');
    expect(document.activeElement).toBe(dialogButton('Continuer'));
    // Keys do not reach the live view behind the question.
    press(' ');
    await tick();
    expect(view.canvas().dataset.page).toBe('1/1');
    dialogButton('Continuer').click();
    await tick();
    expect(dialog()).toBeNull();
    expect(view.onclose).not.toHaveBeenCalled();

    press('Escape');
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    dialogButton('Terminer').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
  });

  it('Space is the next page even when a button has the focus, and does not press it', async () => {
    const view = render();
    await live(view);
    mockLiveTick(150);
    await tick();
    const strokes = view.canvas().dataset.strokes;
    expect(Number(strokes)).toBeGreaterThan(0);
    // « Clear the page » was just used: it keeps the focus.
    view.action('clear-page').focus();
    const down = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    view.action('clear-page').dispatchEvent(down);
    const up = new KeyboardEvent('keyup', { key: ' ', bubbles: true, cancelable: true });
    view.action('clear-page').dispatchEvent(up);
    expect(down.defaultPrevented).toBe(true);
    // The browser would click the button on key up: it is prevented there too.
    expect(up.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    // The first page kept its ink.
    view.button('Page précédente').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    expect(view.canvas().dataset.strokes).toBe(strokes);
    // A key typed in a text field stays the field's.
    const field = document.createElement('input');
    field.type = 'text';
    view.target.append(field);
    const typed = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    field.dispatchEvent(typed);
    expect(typed.defaultPrevented).toBe(false);
  });

  it('offers to undo « Clear the page » for a few seconds, and gives the ink back', async () => {
    const view = render();
    await live(view);
    mockLiveTick(150);
    await tick();
    const strokes = view.canvas().dataset.strokes;
    expect(Number(strokes)).toBeGreaterThan(0);
    const undo = () => view.target.querySelector<HTMLButtonElement>('button[data-action="undo-clear"]');
    expect(undo()).toBeNull();

    view.action('clear-page').click();
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('0'));
    await vi.waitFor(() => expect(undo()).not.toBeNull());
    expect(undo()!.textContent?.trim()).toBe("Annuler l'effacement");
    undo()!.click();
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe(strokes));
    expect(undo()).toBeNull();

    // Clearing again offers it again; changing page withdraws the offer, and an empty page offers nothing.
    view.action('clear-page').click();
    await vi.waitFor(() => expect(undo()).not.toBeNull());
    view.button('Page suivante').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    await vi.waitFor(() => expect(undo()).toBeNull());
    view.action('clear-page').click();
    await tick();
    await tick();
    expect(undo()).toBeNull();
  });

  it('withdraws the offer to undo after eight seconds', async () => {
    const view = render();
    await live(view);
    mockLiveTick(150);
    await tick();
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      view.action('clear-page').click();
      for (let i = 0; i < 10; i++) await tick();
      expect(view.target.querySelector('[data-slot="undo-clear"]')).not.toBeNull();
      vi.advanceTimersByTime(7_900);
      await tick();
      expect(view.target.querySelector('[data-slot="undo-clear"]')).not.toBeNull();
      vi.advanceTimersByTime(200);
      await tick();
      expect(view.target.querySelector('[data-slot="undo-clear"]')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
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
    // Closing anyway also stops the projection and the session in Rust, best effort.
    view.action('project').click();
    await vi.waitFor(() => expect(view.action('stop-projecting')).toBeTruthy());
    const stopping = vi.spyOn(api, 'liveStop');
    const closing = vi.spyOn(api, 'closePresentation');
    view.button('Fermer quand même').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
    expect(closing).toHaveBeenCalled();
    expect(stopping).toHaveBeenCalled();
    expect((await api.liveSubscribe(() => {})).state).toBe('idle');
  });

  it('closes anyway even when the stop and the projection fail', async () => {
    const view = render();
    await live(view);
    vi.spyOn(api, 'liveStop').mockRejectedValue({ code: 'tablet', message: 'link lost' });
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct ne s'est pas arrêté correctement."));
    vi.spyOn(api, 'closePresentation').mockRejectedValue({ code: 'tablet', message: 'gone' });
    view.button('Fermer quand même').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
  });

  it('cannot be ended while live mode is still starting, and stops a session that starts too late', async () => {
    let release!: (snapshot: Awaited<ReturnType<typeof api.liveStart>>) => void;
    const start = api.liveStart;
    vi.spyOn(api, 'liveStart').mockImplementation(
      () => new Promise((resolve) => (release = resolve as typeof release)),
    );
    const view = render();
    await vi.waitFor(() => expect(release).toBeDefined());
    expect(view.action('stop-live').disabled).toBe(true);
    const stopping = vi.spyOn(api, 'liveStop');
    // The view goes away while `live_start` is pending, then Rust answers.
    cleanup?.();
    cleanup = null;
    release(await start());
    await vi.waitFor(() => expect(stopping).toHaveBeenCalledTimes(1));
  });
});

describe('live view: narration', () => {
  beforeEach(() => {
    mockNarration.receivingAfterMs = 0;
    mockNarration.silentAfterMs = 0;
  });

  const section = (view: ReturnType<typeof render>) => view.target.querySelector<HTMLElement>('[data-section="narration"]')!;
  const savedText = (view: ReturnType<typeof render>) =>
    view.target.querySelector('[data-slot="narration-saved"]')?.textContent?.trim();

  it('records the microphone during the session and says on how many pages it was saved', async () => {
    mockNarration.livePages = 2;
    const monitor = vi.spyOn(api, 'microphoneMonitor');
    const start = vi.spyOn(api, 'narrationStart');
    const stop = vi.spyOn(api, 'narrationStop');
    const setMicrophone = vi.spyOn(api, 'setMicrophone');
    const view = render();
    await live(view);
    expect(section(view).querySelector('h3')?.textContent).toBe('Narration');
    expect(monitor).not.toHaveBeenCalled();
    view.action('narration-open').click();
    await vi.waitFor(() => expect(monitor).toHaveBeenCalledWith(null));
    const select = section(view).querySelector<HTMLSelectElement>('select[data-field="microphone"]')!;
    await vi.waitFor(() => expect(select.options).toHaveLength(3));
    select.value = 'mock-mic-1';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(setMicrophone).toHaveBeenCalledWith('mock-mic-1'));
    // The app state that holds the choice is told, so that Settings and the editor show the same one.
    await vi.waitFor(() => expect(view.onchange).toHaveBeenCalledWith(expect.objectContaining({ microphone: 'mock-mic-1' })));

    view.action('narration-start').click();
    await vi.waitFor(() => expect(start).toHaveBeenCalledWith('mock-mic-1', { kind: 'live' }));
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    await vi.waitFor(() => expect(section(view).querySelector('.elapsed')?.textContent).toContain('Enregistrement : 0:00'));
    // The level shows while recording.
    await vi.waitFor(() =>
      expect(Number(section(view).querySelector('[role="meter"]')?.getAttribute('aria-valuenow'))).toBeGreaterThan(0),
    );
    // The session goes on meanwhile.
    mockLiveTick(120);
    await tick();
    expect(Number(view.canvas().dataset.strokes)).toBeGreaterThan(0);

    view.action('narration-stop').click();
    await vi.waitFor(() => expect(savedText(view)).toBe('Narration enregistrée sur 2 pages.'));
    expect(stop).toHaveBeenCalledWith(true, null, null, null);
    // Ready for another take.
    expect(view.action('narration-open')).toBeTruthy();
    expect(view.root().dataset.state).toBe('live');
  });

  it('says when a single page, or none, got the narration', async () => {
    const view = render();
    await live(view);
    const record = async () => {
      view.action('narration-open').click();
      await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
      view.action('narration-start').click();
      await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
      view.action('narration-stop').click();
      await vi.waitFor(() => expect(view.action('narration-open')).toBeTruthy());
    };
    mockNarration.livePages = 1;
    await record();
    expect(savedText(view)).toBe('Narration enregistrée sur 1 page.');
    mockNarration.livePages = 0;
    await record();
    expect(savedText(view)).toBe("Aucune page de la séance ne contient de tracé : la narration n'a pas été conservée.");
  });

  it('throws a cancelled recording away', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-cancel')).toBeTruthy());
    view.action('narration-cancel').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    expect(stop).toHaveBeenCalledWith(false, null, null, null);
    expect(savedText(view)).toBeUndefined();
    expect((await api.narrationStatus()).state).not.toBe('recording');
  });

  it('stops the narration before leaving live mode, so that the pages are saved with it', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const liveStop = vi.spyOn(api, 'liveStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    mockLiveTick(120);
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
    const saving = stop.mock.calls.findIndex(([save]) => save === true);
    expect(saving).toBeGreaterThanOrEqual(0);
    expect(stop.mock.calls[saving]).toEqual([true, null, null, null]);
    expect(stop.mock.invocationCallOrder[saving]).toBeLessThan(liveStop.mock.invocationCallOrder[0]);
    expect((await api.narrationStatus()).state).toBe('idle');
  });

  it('keeps live mode open when the narration cannot be saved', async () => {
    const liveStop = vi.spyOn(api, 'liveStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    vi.spyOn(api, 'narrationStop').mockRejectedValueOnce({ code: 'microphoneFailed', message: 'device lost' });
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct ne s'est pas arrêté correctement."));
    expect(view.onclose).not.toHaveBeenCalled();
    expect(liveStop).not.toHaveBeenCalled();
    // Nothing is left recording; a new try finishes the session.
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
    expect(liveStop).toHaveBeenCalledTimes(1);
  });

  it('offers only to discard a replay recording kept from before: the retry belongs to the editor', async () => {
    const path = [...mockLibrary().notes.keys()][0];
    mockNarration.saveConflict = true;
    await api.narrationStart(null, { kind: 'replay', path, speed: 1, maxPauseMs: null, startMs: 0 });
    await expect(api.narrationStop(true, null, path, null)).rejects.toMatchObject({ code: 'noteConflict' });
    const discard = vi.spyOn(api, 'narrationDiscard');
    const view = render();
    await live(view);
    await vi.waitFor(() => expect(section(view).querySelector('[data-slot="narration-pending"]')).toBeTruthy());
    expect(section(view).textContent).toContain('Ouvrez une note dans l');
    expect(view.action('narration-retry')).toBeNull();
    expect(view.action('narration-open')).toBeNull();
    view.action('narration-discard').click();
    await tick();
    view.action('narration-discard-yes').click();
    await vi.waitFor(() => expect(discard).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(view.action('narration-open')).toBeTruthy());
  });

  it('explains when Windows blocks the microphone', async () => {
    mockNarration.silent = true;
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(section(view).querySelector('[data-capture="silent"]')).toBeTruthy());
    expect(section(view).textContent).toContain("Windows bloque peut-être l'accès au micro");
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    await vi.waitFor(() => expect(section(view).querySelector('[data-capture="silent"]')).toBeTruthy());
    expect(section(view).textContent).toContain('Un interrupteur de coupure du micro donne le même résultat.');
  });

  it('reports a recording that cannot start', async () => {
    mockNarration.microphones = [];
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(section(view).textContent).toContain('Aucun micro détecté'));
    view.action('narration-start').click();
    await vi.waitFor(() => expect(section(view).querySelector('[data-slot="record-error"]')).toBeTruthy());
    expect(view.action('narration-start')).toBeTruthy();
  });

  /** Starts a narration, then leaves live mode while stopping it fails with `code`. */
  async function leaveWhileStoppingFails(code: string) {
    const liveStop = vi.spyOn(api, 'liveStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    vi.spyOn(api, 'narrationStop').mockRejectedValueOnce({ code, message: 'nothing' });
    view.action('stop-live').click();
    return { view, liveStop };
  }

  it.each(['emptyRecording', 'microphoneSilent'])(
    'leaves live mode anyway when the narration has nothing to keep (%s)',
    async (code) => {
      const { view, liveStop } = await leaveWhileStoppingFails(code);
      await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
      expect(liveStop).toHaveBeenCalledTimes(1);
      expect(view.target.textContent).not.toContain("Le mode direct ne s'est pas arrêté correctement.");
      expect((await api.narrationStatus()).state).toBe('idle');
    },
  );

  it('leaves live mode when the mock recording is empty, as Rust says emptyRecording', async () => {
    mockNarration.empty = true;
    const liveStop = vi.spyOn(api, 'liveStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    view.action('stop-live').click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledTimes(1));
    expect(liveStop).toHaveBeenCalledTimes(1);
  });

  it('keeps live mode open for any other failure of the narration', async () => {
    const { view, liveStop } = await leaveWhileStoppingFails('noteConflict');
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct ne s'est pas arrêté correctement."));
    expect(view.onclose).not.toHaveBeenCalled();
    expect(liveStop).not.toHaveBeenCalled();
  });

  it('saves a recording left behind when the view goes away, instead of discarding it', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const view = render();
    await live(view);
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(stop).toHaveBeenCalledWith(true, null, null, null));
    expect(stop).not.toHaveBeenCalledWith(false, null, null, null);
    expect((await api.narrationStatus()).state).toBe('idle');
  });

  it('speaks English', async () => {
    setLocale('en');
    mockNarration.livePages = 3;
    const view = render();
    await live(view);
    expect(view.action('narration-open').textContent?.trim()).toBe('Record the narration');
    view.action('narration-open').click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    expect(view.action('narration-start').textContent?.trim()).toBe('Start');
    view.action('narration-start').click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    expect(view.action('narration-stop').textContent?.trim()).toBe('Stop and save');
    view.action('narration-stop').click();
    await vi.waitFor(() => expect(savedText(view)).toBe('Narration saved on 3 pages.'));
  });
});
