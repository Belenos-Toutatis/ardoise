// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M16, package C: the live control window's side panel is the shared tabbed rail
// (docs/design/side-panel.md, §8). Four tabs (Calques, Son, Projection, Aide) and no Page or Fond until M17; Calques
// open by default, then the window remembers its tab and fold; each tab holds what the old panel had; the selection's
// bar and the Select and Pen hints sit above the open tab (a selection reopens the pane, a hint does not); the badges
// follow the layers, the recording, the projection and the share; the blackboard and laser are switches of the
// projected area as well as the toolbar's buttons.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, resetMockLive } from '../../lib/mock-live';
import { mockNarration, resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { resetMockTablet } from '../../lib/mock-tablet';
import { share } from '../../lib/share-state.svelte';
import { sideStorageKey } from '../../lib/side-panel.svelte';
import { tablet } from '../../lib/tablet-state.svelte';
import { openSideTab, rememberSide, sideOf, tabPanelOf } from '../side/side-test-helpers';
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
  mockNarration.receivingAfterMs = 0;
  mockNarration.silentAfterMs = 0;
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
  const side = () => q('aside.live-side')!;
  const tabs = () => [...side().querySelectorAll<HTMLButtonElement>('.side-rail [role="tab"]')];
  const tab = (id: string) => q<HTMLButtonElement>(`.side-rail [role="tab"][data-tab="${id}"]`)!;
  const panel = (id: string) => q(`[data-tab-panel="${id}"]`)!;
  const above = () => q('[data-slot="side-above"]');
  const action = (name: string) => q<HTMLButtonElement>(`button[data-action="${name}"]`);
  const field = (name: string) => q<HTMLInputElement>(`input[data-field="${name}"]`)!;
  const canvas = () => q<HTMLCanvasElement>('canvas.live-canvas')!;
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
  return { target, q, side, tabs, tab, panel, above, action, field, canvas, lasso };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.q('.live-view')!.dataset.state).toBe('live'));
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
}

const stored = () => JSON.parse(localStorage.getItem(sideStorageKey('live')) ?? 'null');

describe('the live window side panel: tabs', () => {
  it('has the rail Calques, Son, Projection, Aide and no Page or Fond, in an aside with its label', async () => {
    const view = render();
    expect(view.side().getAttribute('aria-label')).toBe('Panneau de côté');
    expect(view.q('[data-side-panel="live"]')).not.toBeNull();
    expect(view.tabs().map((b) => b.dataset.tab)).toEqual(['layers', 'sound', 'projection', 'help']);
    expect(view.q('[data-tab-panel="page"], [data-tab-panel="background"]')).toBeNull();
  });

  it('opens on Calques by default, the zone shown and the other tabs hidden', async () => {
    const view = render();
    await live(view);
    expect(sideOf(view.target)).toEqual({ tab: 'layers', collapsed: false });
    expect(view.panel('layers').hidden).toBe(false);
    expect(view.panel('layers').querySelector('section.layers-zone[data-where="live"]')).not.toBeNull();
    for (const id of ['sound', 'projection', 'help']) expect(view.panel(id).hidden).toBe(true);
    expect(view.q('[data-slot="side-title"]')!.textContent).toBe('Calques');
  });

  it('holds each old block in its tab, with its hooks', async () => {
    const view = render();
    await live(view);
    const where = (selector: string) => tabPanelOf(view.q(selector)!);
    expect(where('[data-section="narration"]')).toBe('sound');
    expect(where('button[data-action="narration-open"]')).toBe('sound');
    expect(where('[data-section="projector"]')).toBe('projection');
    expect(where('button[data-action="project"]')).toBe('projection');
    expect(where('[data-presentation]')).toBe('projection');
    expect(where('[data-section="zone"]')).toBe('projection');
    expect(where('[data-zoom]')).toBe('projection');
    expect(where('button[data-action="whole-page"]')).toBe('projection');
    expect(where('button[data-action="page-width"]')).toBe('projection');
    expect(where('input[data-field="board"]')).toBe('projection');
    expect(where('input[data-field="laser"]')).toBe('projection');
    expect(where('[data-section="share"]')).toBe('projection');
    expect(where('button[data-action="share"]')).toBe('projection');
    expect(where('dl.keys')).toBe('help');
    // The preview's hint is under the zone block; the screen list is the first select of the side panel.
    expect(view.q('[data-section="zone"]')!.textContent).toContain('Faites glisser le rectangle bleu');
    expect(view.q('aside.live-side select')).toBe(view.panel('projection').querySelector('select'));
    // The toolbar keeps the blackboard and the laser buttons, and no longer the share.
    expect(view.q('.live-toolbar [data-action="board"]')).not.toBeNull();
    expect(view.q('.live-toolbar [data-action="laser"]')).not.toBeNull();
    expect(view.q('.live-toolbar [data-action="share"]')).toBeNull();
    expect(view.q('[data-slot="share"]')).toBeNull();
  });

  it('shows another tab when its icon is clicked, folds on the open one, and remembers both', async () => {
    const view = render();
    await live(view);
    await openSideTab(view.target, 'projection');
    expect(sideOf(view.target)).toEqual({ tab: 'projection', collapsed: false });
    expect(view.panel('projection').hidden).toBe(false);
    expect(view.panel('layers').hidden).toBe(true);
    expect(stored()).toEqual({ tab: 'projection', collapsed: false });
    // The open tab folds the pane; the rail stays.
    view.tab('projection').click();
    await tick();
    expect(sideOf(view.target)).toEqual({ tab: 'projection', collapsed: true });
    expect(stored()).toEqual({ tab: 'projection', collapsed: true });
    expect(view.q<HTMLElement>('.side-pane')!.hidden).toBe(true);
    expect(view.tabs()).toHaveLength(4);
    // « › » folds as well.
    await openSideTab(view.target, 'help');
    view.q<HTMLButtonElement>('[data-action="side-collapse"]')!.click();
    await tick();
    expect(stored()).toEqual({ tab: 'help', collapsed: true });
  });

  it('finds its tab and fold again when it opens', async () => {
    rememberSide('live', { tab: 'projection', collapsed: true });
    const view = render();
    await live(view);
    expect(sideOf(view.target)).toEqual({ tab: 'projection', collapsed: true });
    expect(view.q<HTMLElement>('.side-pane')!.hidden).toBe(true);
    expect(view.panel('projection').hidden).toBe(false);
    // Tabs hold their content while folded: the projector button is there.
    expect(view.action('project')).not.toBeNull();
  });

  it('shows Calques for a remembered tab it lacks, and keeps the remembered one until another is chosen', async () => {
    rememberSide('live', { tab: 'page', collapsed: false });
    const view = render();
    await live(view);
    expect(sideOf(view.target)).toEqual({ tab: 'layers', collapsed: false });
    expect(view.panel('layers').hidden).toBe(false);
    expect(stored()).toEqual({ tab: 'page', collapsed: false });
    await openSideTab(view.target, 'sound');
    expect(stored()).toEqual({ tab: 'sound', collapsed: false });
  });

  it('has the Space key for the pages, never for the tab of the rail that has the focus', async () => {
    const view = render();
    await live(view);
    expect(view.canvas().dataset.page).toBe('1/1');
    const tabButton = view.tab('projection');
    tabButton.focus();
    const down = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    tabButton.dispatchEvent(down);
    const up = new KeyboardEvent('keyup', { key: ' ', bubbles: true, cancelable: true });
    tabButton.dispatchEvent(up);
    expect(down.defaultPrevented).toBe(true);
    expect(up.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    expect(sideOf(view.target).tab).toBe('layers');
  });

  it('speaks English', async () => {
    setLocale('en');
    const view = render();
    expect(view.side().getAttribute('aria-label')).toBe('Side panel');
    expect(view.tab('layers').textContent).toContain('Layers');
    expect(view.tab('sound').textContent).toContain('Sound');
    expect(view.q('[data-section="projector"] h3')!.textContent).toBe('Projector');
    expect(view.q('[data-section="zone"] h3')!.textContent).toBe('Projected area');
    expect(view.q('[data-section="share"] h3')!.textContent).toBe('Network sharing');
    expect(view.q('[data-section="shortcuts"] h3')!.textContent).toBe('Live mode shortcuts');
  });
});

describe('the live window side panel: the block above the open tab', () => {
  it('shows no above block while no tool is on and nothing is selected', async () => {
    const view = render(sheetPath());
    await live(view);
    expect(view.above()).toBeNull();
  });

  it('shows the Select hint and the Pen hint above the tab, whatever it is, without reopening a folded pane', async () => {
    rememberSide('live', { tab: 'projection', collapsed: true });
    const view = render();
    await live(view);
    press('v');
    await tick();
    expect(view.above()!.querySelector('[data-slot="select-hint"]')!.textContent).toContain('Lasso');
    expect(sideOf(view.target)).toEqual({ tab: 'projection', collapsed: true });
    press('p');
    await tick();
    expect(view.above()!.querySelector('[data-slot="pen-hint"]')!.textContent).toContain('Stylet ou souris');
    expect(view.above()!.querySelector('[data-slot="select-hint"]')).toBeNull();
    expect(sideOf(view.target)).toEqual({ tab: 'projection', collapsed: true });
    press('p');
    await tick();
    expect(view.above()).toBeNull();
  });

  it('puts the selection bar above the open tab and reopens a folded pane on the tab it shows', async () => {
    rememberSide('live', { tab: 'help', collapsed: true });
    const view = render(sheetPath());
    await live(view);
    press('v');
    await tick();
    expect(sideOf(view.target).collapsed).toBe(true);
    view.lasso();
    await vi.waitFor(() => expect(view.above()!.querySelector('[data-slot="select-bar"]')).not.toBeNull());
    expect(sideOf(view.target)).toEqual({ tab: 'help', collapsed: false });
    expect(stored()).toEqual({ tab: 'help', collapsed: false });
    const bar = view.above()!.querySelector<HTMLElement>('[data-slot="select-bar"]')!;
    expect(bar.getAttribute('role')).toBe('toolbar');
    expect(bar.querySelector<HTMLElement>('.select-count')!.dataset.count).toBe('1');
    expect(bar.querySelector('button[data-action="delete-selection"]')).not.toBeNull();
    expect(bar.querySelector('button[data-action="deselect"]')).not.toBeNull();
    expect(bar.textContent).toContain('Les pastilles de couleur recolorent la sélection.');
    // Above the tab panels, not over the preview.
    expect(view.above()!.compareDocumentPosition(view.panel('help')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(view.q('.live-preview [data-slot="select-bar"]')).toBeNull();
    // The bar acts: Deselect removes it, and the pane stays open.
    view.action('deselect')!.click();
    await vi.waitFor(() => expect(view.above()!.querySelector('[data-slot="select-bar"]')).toBeNull());
    expect(sideOf(view.target).collapsed).toBe(false);
  });

  it('reopens the pane at a new selection, not at every change of it', async () => {
    const view = render(sheetPath());
    await live(view);
    press('v');
    await tick();
    view.lasso();
    await vi.waitFor(() => expect(view.above()!.querySelector('[data-slot="select-bar"]')).not.toBeNull());
    // The user folds the pane while the selection lasts: choosing more strokes does not reopen it.
    view.tab('layers').click();
    await tick();
    expect(sideOf(view.target).collapsed).toBe(true);
    press('Escape');
    await tick();
    expect(view.above()!.querySelector('[data-slot="select-bar"]')).toBeNull();
    expect(sideOf(view.target).collapsed).toBe(true);
  });
});

describe('the live window side panel: badges', () => {
  it("counts the layers on Calques and warns, with an orange dot, while the tablet's layer is hidden", async () => {
    const view = render();
    await live(view);
    await vi.waitFor(() => expect(view.tab('layers').dataset.count).toBe('1'));
    expect(view.tab('layers').dataset.dot).toBeUndefined();
    expect(view.tab('layers').querySelector('.side-tab-count')!.textContent).toBe('1');
    view.action('add-layer')!.click();
    await vi.waitFor(() => expect(view.tab('layers').dataset.count).toBe('2'));
    // The tablet's layer is the first one: hiding it warns.
    view.q<HTMLButtonElement>('li.layer[data-layer="0"] [data-action="layer-visible"]')!.click();
    await vi.waitFor(() => expect(view.tab('layers').dataset.dot).toBe('warn'));
    expect(view.tab('layers').textContent).toContain('un calque utilisé est masqué');
    view.q<HTMLButtonElement>('li.layer[data-layer="0"] [data-action="layer-visible"]')!.click();
    await vi.waitFor(() => expect(view.tab('layers').dataset.dot).toBeUndefined());
  });

  it('has a red dot on Son while the narration records', async () => {
    const view = render();
    await live(view);
    expect(view.tab('sound').dataset.dot).toBeUndefined();
    view.action('narration-open')!.click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start')!.click();
    await vi.waitFor(() => expect(view.tab('sound').dataset.dot).toBe('rec'));
    // Visible while the pane is folded: that is the point of the dot.
    await openSideTab(view.target, 'sound');
    view.tab('sound').click();
    await tick();
    expect(sideOf(view.target).collapsed).toBe(true);
    expect(view.tab('sound').dataset.dot).toBe('rec');
    view.action('narration-stop')!.click();
    await vi.waitFor(() => expect(view.tab('sound').dataset.dot).toBeUndefined());
  });

  it('has a blue dot on Projection while the projector is on, and none on Aide', async () => {
    const view = render();
    await live(view);
    await vi.waitFor(() => expect(view.q<HTMLSelectElement>('aside.live-side select')!.options.length).toBeGreaterThan(0));
    expect(view.tab('projection').dataset.dot).toBeUndefined();
    view.action('project')!.click();
    await vi.waitFor(() => expect(view.tab('projection').dataset.dot).toBe('live'));
    expect(view.tab('projection').textContent).toContain('projection ou partage en cours');
    view.action('stop-projecting')!.click();
    await vi.waitFor(() => expect(view.tab('projection').dataset.dot).toBeUndefined());
    expect(view.tab('help').dataset.dot).toBeUndefined();
    expect(view.tab('help').dataset.count).toBeUndefined();
  });

  it('has the blue dot while a share runs, and says so in its block', async () => {
    const view = render();
    await live(view);
    view.action('share')!.click();
    await vi.waitFor(() => expect(share.status.active).toBe(true));
    await tick();
    expect(view.tab('projection').dataset.dot).toBe('live');
    await share.stop();
    await vi.waitFor(() => expect(view.tab('projection').dataset.dot).toBeUndefined());
  });
});

describe('the live window side panel: the switches of the projected area', () => {
  it('are the blackboard and the laser pointer, in step with the toolbar buttons and the keys', async () => {
    const view = render();
    await live(view);
    const board = view.field('board');
    const laser = view.field('laser');
    expect([board.type, board.getAttribute('role'), board.checked, laser.checked]).toEqual(['checkbox', 'switch', false, false]);
    board.click();
    await vi.waitFor(() => expect(view.action('board')!.getAttribute('aria-pressed')).toBe('true'));
    expect(board.checked).toBe(true);
    laser.click();
    await vi.waitFor(() => expect(view.action('laser')!.getAttribute('aria-pressed')).toBe('true'));
    expect(laser.checked).toBe(true);
    // The toolbar and the keys move the switches.
    view.action('board')!.click();
    await vi.waitFor(() => expect(board.checked).toBe(false));
    press('l');
    await vi.waitFor(() => expect(laser.checked).toBe(false));
    press('b');
    await vi.waitFor(() => expect(board.checked).toBe(true));
  });

  it('keep the zoom as − / value / + beside the two fits', async () => {
    const view = render();
    await live(view);
    const zone = view.q('[data-section="zone"]')!;
    const level = () => Number(zone.querySelector<HTMLElement>('[data-zoom]')!.dataset.zoom);
    expect(level()).toBe(1);
    zone.querySelector<HTMLButtonElement>('button[aria-label="Agrandir"]')!.click();
    await vi.waitFor(() => expect(level()).toBeCloseTo(1.25, 6));
    expect(zone.querySelector('[data-zoom]')!.textContent?.trim()).toBe('× 1,3');
    zone.querySelector<HTMLButtonElement>('button[data-action="whole-page"]')!.click();
    await vi.waitFor(() => expect(level()).toBe(1));
    expect(zone.querySelector('input[type="range"]')).toBeNull();
  });
});
