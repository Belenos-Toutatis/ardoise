// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M14: correcting during live in the control window. The layers zone is wired to `live_layers` (choose,
// the tablet icon, add, rename, hide and its warnings); P and V switch the computer's tools; the Pen writes on the
// chosen layer in the computer's ink, which its panel sets and this PC remembers; Ctrl+Z undoes the computer's
// stroke; choosing another layer drops the selection; the « Nouveau calque » button is gone. The session is the
// browser stand-in of Rust, which follows package B's rules.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale, t } from '../../lib/i18n.svelte';
import { COMPUTER_STYLE_KEY } from '../../lib/live-computer-style';
import { FakeContext } from '../../lib/live-fake-canvas';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import type { LiveSnapshot } from '../../lib/live';
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
  const rows = () => [...zone().querySelectorAll<HTMLLIElement>('li.layer')];
  const row = (index: number) => zone().querySelector<HTMLLIElement>(`li.layer[data-layer="${index}"]`)!;
  const inRow = (index: number, name: string) => row(index).querySelector<HTMLButtonElement>(`[data-action="${name}"]`)!;
  const chosen = () => Number(zone().querySelector('li.layer[data-chosen="true"]')?.getAttribute('data-layer'));
  const tabletRow = () => Number(zone().querySelector('li.layer[data-tablet="true"]')?.getAttribute('data-layer'));
  const warnings = () => [...zone().querySelectorAll<HTMLElement>('[data-slot="layer-warning"]')].map((p) => p.dataset.kind);
  const notice = () => [...target.querySelectorAll('.banner[role="alert"] p')].map((p) => p.textContent?.trim());
  const session = () => mockLiveInvoke('live_start', {}) as LiveSnapshot;
  const fire = (type: string, p: { x: number; y: number }, init: { pointerType?: string; pointerId?: number; buttons?: number } = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : (init.buttons ?? 1) });
    const props = { pointerType: init.pointerType ?? 'mouse', pointerId: init.pointerId ?? 1, pressure: 0.5, timeStamp: performance.now() };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas().dispatchEvent(event);
  };
  /** A stroke of the computer across the page's foot. */
  const write = (y = 18000, init: { pointerType?: string; pointerId?: number; buttons?: number } = {}) => {
    fire('pointerdown', at(4000, y), init);
    for (let x = 5000; x <= 9000; x += 1000) fire('pointermove', at(x, y + (x % 2000)), init);
    fire('pointerup', at(9000, y), init);
  };
  return { target, q, action, canvas, zone, rows, row, inRow, chosen, tabletRow, warnings, notice, session, fire, write };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.q('.live-view')!.dataset.state).toBe('live'));
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
}

describe('the layers zone of the live window', () => {
  it('opens the side panel, one row chosen with the tablet on it; the « Nouveau calque » button is gone', async () => {
    const view = render();
    await live(view);
    // Calques is the tab the side panel opens on: the zone is what it shows, and nothing else comes before it.
    const layers = view.q('aside.live-side [data-tab-panel="layers"]')!;
    expect(layers.hidden).toBe(false);
    expect(layers.firstElementChild).toBe(view.zone());
    expect(view.rows()).toHaveLength(1);
    expect([view.chosen(), view.tabletRow()]).toEqual([0, 0]);
    expect(view.action('new-layer')).toBeNull();
    // The toolbar says whose each group is.
    expect(view.q('[data-slot="touch-up"]')!.textContent).toContain(t('live.computer.label'));
    expect(view.q('.live-toolbar')!.textContent).toContain(t('live.tabletStyle'));
  });

  it('adds a layer, which the computer then acts on while the tablet stays', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    // Top first.
    expect(view.rows().map((r) => r.dataset.layer)).toEqual(['1', '0']);
    expect([view.chosen(), view.tabletRow()]).toEqual([1, 0]);
    expect(view.session()).toMatchObject({ layer: 0, computerLayer: 1 });
  });

  it('a click on a row chooses it; the tablet icon sends the tablet there', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.chosen()).toBe(1));
    view.row(0).click();
    await vi.waitFor(() => expect(view.chosen()).toBe(0));
    expect(view.session().computerLayer).toBe(0);
    view.inRow(1, 'tablet-layer').click();
    await vi.waitFor(() => expect(view.tabletRow()).toBe(1));
    expect(view.session().layer).toBe(1);
    expect(view.chosen()).toBe(0);
  });

  it('renames with the pencil and Enter', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.inRow(1, 'rename-layer').click();
    await tick();
    const field = view.row(1).querySelector<HTMLInputElement>('input.layer-rename')!;
    field.value = '  Correction ';
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(view.row(1).textContent).toContain('Correction'));
    expect(view.session().page.layers.map((l) => l.name)).toEqual(['Calque 1', 'Correction']);
  });

  it("hides a layer: the tablet's and the chosen one's warnings; a pen down there writes nothing and says why", async () => {
    const view = render();
    await live(view);
    view.inRow(0, 'layer-visible').click();
    await vi.waitFor(() => expect(view.warnings()).toEqual(['tabletHidden', 'chosenHidden']));
    expect(view.row(0).dataset.hidden).toBe('true');
    press('p');
    await tick();
    view.write();
    await vi.waitFor(() => expect(view.notice()).toContain(t('layers.warn.chosenHidden', { name: 'Calque 1' })));
    expect(view.session().page.strokes).toHaveLength(0);
    view.inRow(0, 'layer-visible').click();
    await vi.waitFor(() => expect(view.warnings()).toEqual([]));
  });

  it('is disabled once the session is over', async () => {
    const view = render();
    await live(view);
    mockLiveInvoke('live_stop', {});
    await vi.waitFor(() => expect(view.action('add-layer')!.disabled).toBe(true));
    expect(view.action('pen')!.disabled).toBe(true);
    expect(view.action('computer-style')!.disabled).toBe(true);
  });
});

describe("the computer's tools", () => {
  it('P and V switch Pen and Select, never both; Pen shows its hint', async () => {
    const view = render();
    await live(view);
    expect(view.canvas().dataset.tool).toBe('none');
    press('p');
    await tick();
    expect(view.action('pen')!.getAttribute('aria-pressed')).toBe('true');
    expect(view.canvas().dataset.tool).toBe('pen');
    expect(view.q('[data-slot="pen-hint"]')!.textContent).toBe(t('live.pen.help'));
    press('v');
    await tick();
    expect(view.action('pen')!.getAttribute('aria-pressed')).toBe('false');
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('true');
    expect(view.canvas().dataset.tool).toBe('select');
    expect(view.q('[data-slot="pen-hint"]')).toBeNull();
    view.action('pen')!.click();
    await tick();
    expect(view.action('select')!.getAttribute('aria-pressed')).toBe('false');
    press('p');
    await tick();
    expect(view.canvas().dataset.tool).toBe('none');
    // The keys list says it.
    expect(view.q('dl.keys')!.textContent).toContain(t('live.keys.list.pen.action'));
  });

  it("writes with the computer's ink on the chosen layer, and Ctrl+Z undoes it, never the tablet's writing", async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.chosen()).toBe(1));
    press('p');
    await tick();
    view.write();
    await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(1));
    const stroke = view.session().page.strokes[0];
    expect(stroke).toMatchObject({ tool: 'feltTip', width: 120, layer: 1, color: { r: 0xd8, g: 0, b: 0, a: 255 } });
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('1'));
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
    press('z', { ctrlKey: true });
    await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(0));
    await vi.waitFor(() => expect(view.action('redo-edit')!.disabled).toBe(false));
    press('y', { ctrlKey: true });
    await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(1));
  });

  it('ignores Ctrl+Z while a gesture is under way, and Esc drops the gesture first', async () => {
    const view = render();
    await live(view);
    press('p');
    await tick();
    view.write(16000);
    await vi.waitFor(() => expect(view.action('undo-edit')!.disabled).toBe(false));
    view.fire('pointerdown', at(4000, 19000));
    view.fire('pointermove', at(6000, 19000));
    press('z', { ctrlKey: true });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(view.session().page.strokes).toHaveLength(1);
    const escape = press('Escape');
    expect(escape.defaultPrevented).toBe(true);
    // Esc did not ask to leave live mode.
    await tick();
    expect(view.q('[data-action="leave-confirm"]')).toBeNull();
    view.fire('pointerup', at(6000, 19000));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(view.session().page.strokes).toHaveLength(1);
    expect(view.session().page.strokes[0].points[0].y).toBeCloseTo(16000, -2);
  });

  it("sets the computer's ink in its panel, remembered on this PC; Esc and a click outside close it", async () => {
    const view = render();
    await live(view);
    view.action('computer-style')!.click();
    await tick();
    const panel = () => view.q('[data-slot="computer-style"]');
    expect(panel()).not.toBeNull();
    const blue = panel()!.querySelector<HTMLButtonElement>(`button.swatch[aria-label="${t('editor.colors.blue')}"]`)!;
    blue.click();
    await tick();
    const select = panel()!.querySelector<HTMLSelectElement>('select')!;
    select.value = 'highlighter';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(JSON.parse(localStorage.getItem(COMPUTER_STYLE_KEY)!)).toEqual({ tool: 'highlighter', color: '#51a7f9', width: 500 });
    // The tablet's colour did not change.
    expect(view.session().style.color).toEqual({ r: 0, g: 0, b: 0, a: 255 });
    expect(view.action('computer-style')!.textContent).toContain(t('editor.ink.tools.highlighter'));
    press('Escape');
    await tick();
    expect(panel()).toBeNull();
    expect(view.q('[data-action="leave-confirm"]')).toBeNull();
    view.action('computer-style')!.click();
    await tick();
    expect(panel()).not.toBeNull();
    view.q('.live-top')!.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    await tick();
    expect(panel()).toBeNull();
  });

  it('starts from the ink remembered', async () => {
    localStorage.setItem(COMPUTER_STYLE_KEY, JSON.stringify({ tool: 'brush', color: '#84bd00', width: 300 }));
    const view = render();
    await live(view);
    press('p');
    await tick();
    view.write();
    await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(1));
    expect(view.session().page.strokes[0]).toMatchObject({ tool: 'brush', width: 300, color: { r: 0x84, g: 0xbd, b: 0, a: 255 } });
  });

  it('drops the selection when another layer is chosen', async () => {
    const view = render();
    await live(view);
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.chosen()).toBe(1));
    press('p');
    await tick();
    view.write(15000);
    await vi.waitFor(() => expect(view.session().page.strokes).toHaveLength(1));
    press('v');
    await tick();
    // A lasso round the computer's stroke.
    const corners = [at(3000, 13000), at(10000, 13000), at(10000, 20000), at(3000, 20000), at(3000, 13010)];
    view.fire('pointerdown', corners[0]);
    for (const p of corners.slice(1)) view.fire('pointermove', p);
    view.fire('pointerup', corners[4]);
    await vi.waitFor(() => expect(view.q('[data-slot="select-bar"]')).not.toBeNull());
    view.row(0).click();
    await vi.waitFor(() => expect(view.chosen()).toBe(0));
    await tick();
    expect(view.q('[data-slot="select-bar"]')).toBeNull();
  });
});
