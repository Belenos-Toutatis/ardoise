// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The editor's side panel (milestone M16; docs/design/side-panel.md, §2 to §5 and §7): the shared rail with the
 * tabs Calques, Page, Fond, Son, Projection and Aide, what each tab holds, the selection and the split panel above
 * the open tab (and the pane reopening for them), the replay showing Son and giving the tab back, the badges and
 * what is remembered. The rail's own behaviour (keys, focus) is `SidePanel.test.ts`.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import type { Stroke } from '../../lib/note';
import { share } from '../../lib/share-state.svelte';
import { SIDE_TABS, sideStorageKey } from '../../lib/side-panel.svelte';
import { openSideTab, rememberSide, sideOf, tabPanelOf } from '../side/side-test-helpers';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real stroke geometry, from the module's bytes (jsdom cannot fetch it).
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
  resetMockLibrary();
  resetMockNarration();
  resetMockShare();
  share.reset();
  setLocale('fr');
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.useRealTimers();
  share.reset();
  resetMockShare();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(title = 'Cours de physique') {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector)!;
  const panel = (tab: string) => q(`[data-tab-panel="${tab}"]`);
  const tab = (id: string) => q<HTMLButtonElement>(`[role="tab"][data-tab="${id}"]`);
  const action = (name: string) => q<HTMLButtonElement>(`[data-action="${name}"]`);
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const toolbarButton = (label: string) =>
    [...target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.includes(label))!;
  const stored = () => JSON.parse(localStorage.getItem(sideStorageKey('editor')) ?? 'null');
  return { path, target, q, panel, tab, action, key, toolbarButton, stored };
}

const BLACK = { r: 0, g: 0, b: 0, a: 255 };

/** A stroke of 21 samples spread over [t0, t1] ms. */
function slowStroke(y: number, t0: number, t1: number): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({
      x: 2000 + i * 500,
      y,
      pressure: 0.5,
      t: Math.round(t0 + ((t1 - t0) * i) / 20),
    })),
  };
}

describe('editor side panel: the rail and its tabs', () => {
  it('is the shared rail in an aside, with the six tabs in order and Calques open', async () => {
    const { target, q } = await render();
    const aside = q('aside.editor-side');
    expect(aside.getAttribute('aria-label')).toBe('Panneau de côté');
    expect(aside.querySelector('[data-side-panel="editor"]')).toBeTruthy();
    const ids = [...target.querySelectorAll<HTMLElement>('.side-rail [role="tab"]')].map((b) => b.dataset.tab);
    expect(ids).toEqual([...SIDE_TABS]);
    expect(sideOf(target)).toEqual({ tab: 'layers', collapsed: false });
    expect(q('[data-slot="side-title"]').textContent).toBe('Calques');
    expect(q('[data-tab-panel="layers"]').hidden).toBe(false);
    expect(q('[data-tab-panel="page"]').hidden).toBe(true);
    // The side sections of old are gone.
    expect(target.querySelector('.side-section')).toBeNull();
  });

  it('holds each block in its tab', async () => {
    const { target, panel } = await render();
    const where = (selector: string) => {
      const el = target.querySelector(selector);
      expect(el, selector).toBeTruthy();
      return tabPanelOf(el!);
    };
    expect(where('section.layers-zone[data-where="editor"]')).toBe('layers');
    expect(where('[data-section="paper"]')).toBe('page');
    expect(where('[data-section="page-format"]')).toBe('page');
    expect(where('[data-section="turn-crop"]')).toBe('page');
    expect(where('[data-section="original"]')).toBe('page');
    expect(where('[data-section="background-image"]')).toBe('background');
    expect(where('[data-section="narration"]')).toBe('sound');
    expect(where('button[data-action="sound-replay"]')).toBe('sound');
    expect(where('[data-section="share"] button[data-action="share"]')).toBe('projection');
    expect(where('[data-section="shortcuts"]')).toBe('help');
    // The page's controls keep their hooks.
    for (const hook of ['orientation-portrait', 'orientation-landscape', 'apply-size', 'rotate-ccw', 'rotate-cw', 'auto-crop', 'manual-crop']) {
      expect(panel('page').querySelector(`[data-action="${hook}"]`), hook).toBeTruthy();
    }
    for (const field of ['paper-size', 'width-mm', 'height-mm', 'scale-ink', 'template', 'texture']) {
      expect(panel('page').querySelector(`[data-field="${field}"]`), field).toBeTruthy();
    }
    expect(panel('background').querySelector('[data-action="background-choose"]')).toBeTruthy();
    // The shortcuts help lists the editor's table.
    expect(panel('help').querySelector('dl.keys dt[data-shortcut="pen"]')).toBeTruthy();
  });

  it('has no share button in the toolbar any more', async () => {
    const { target } = await render();
    expect(target.querySelector('.toolbar [data-action="share"]')).toBeNull();
    // The replay, the split, the copy, the exports and the resume stay there.
    expect(target.querySelector('.toolbar [data-action="replay-toggle"]')).toBeTruthy();
    expect(target.querySelector('.toolbar [data-action="print"]')).toBeTruthy();
  });

  it('opens a tab on a click, collapses on a second click, and remembers both', async () => {
    const { target, tab, stored } = await render();
    await openSideTab(target, 'page');
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: false });
    expect(stored()).toEqual({ tab: 'page', collapsed: false });
    tab('page').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: true });
    expect(target.querySelector<HTMLElement>('.side-pane')!.hidden).toBe(true);
    expect(stored()).toEqual({ tab: 'page', collapsed: true });
  });

  it('starts where it was left on this PC', async () => {
    rememberSide('editor', { tab: 'background', collapsed: true });
    const { target } = await render();
    expect(sideOf(target)).toEqual({ tab: 'background', collapsed: true });
  });
});

describe('editor side panel: Son', () => {
  it('says that the narration lives in a replay and starts one', async () => {
    const { target, q, action, panel } = await render();
    expect(panel('sound').textContent).toContain("La narration s'écoute et s'enregistre pendant une relecture.");
    expect(target.querySelector('[data-action="narration-open"]')).toBeNull();
    action('sound-replay').click();
    await tick();
    expect(q('.replay-bar')).toBeTruthy();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: false });
    expect(panel('sound').querySelector('[data-action="narration-open"]')).toBeTruthy();
    expect(target.querySelector('[data-action="sound-replay"]')).toBeNull();
  });

  it('is disabled while the split panel is open', async () => {
    const { action, toolbarButton } = await render();
    expect(action('sound-replay').disabled).toBe(false);
    toolbarButton('Découper').click();
    await tick();
    expect(action('sound-replay').disabled).toBe(true);
  });

  it('is shown by a replay, which gives back the tab it found and its collapsed state', async () => {
    const { target, toolbarButton, key, tab } = await render();
    await openSideTab(target, 'page');
    tab('page').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: true });
    // R starts the replay: Son opens, the pane too.
    await key('r');
    expect(target.querySelector('.replay-bar')).toBeTruthy();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: false });
    await key('r');
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: true });
    // The button of the toolbar does the same.
    await openSideTab(target, 'help');
    toolbarButton('Relecture').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: false });
    toolbarButton('Relecture').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'help', collapsed: false });
  });

  it('locks the tabs that edit the note during a replay (they used to disappear)', async () => {
    const { target, toolbarButton, panel } = await render();
    toolbarButton('Relecture').click();
    await tick();
    const controls = (tabId: string) =>
      [...panel(tabId).querySelectorAll<HTMLElement>('button, input, select')] as (HTMLButtonElement | HTMLInputElement)[];
    expect(controls('page').every((c) => c.disabled)).toBe(true);
    expect(controls('background').every((c) => c.disabled)).toBe(true);
    expect(controls('layers').filter((c) => c.dataset.action === 'add-layer').every((c) => c.disabled)).toBe(true);
    // The share stays as it is.
    expect(panel('projection').querySelector<HTMLButtonElement>('[data-action="share"]')!.disabled).toBe(false);
    expect(target.querySelector('[data-action="narration-open"]')).toBeTruthy();
  });
});

describe('editor side panel: the block above the open tab', () => {
  it('shows the selection above the tab, whatever the tab, and its actions', async () => {
    const { target, q, key, action } = await render();
    expect(target.querySelector('[data-slot="side-above"]')).toBeNull();
    await key('v');
    await key('a', { ctrlKey: true });
    const above = q('[data-slot="side-above"]');
    const block = above.querySelector<HTMLElement>('[data-section="selection"]')!;
    expect(block).toBeTruthy();
    expect(block.textContent).toContain('Traits sélectionnés : 2');
    // Above the tab panels, in the pane.
    expect(above.compareDocumentPosition(q('.side-pane-body')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    for (const name of ['copy-selection', 'cut-selection', 'paste-selection', 'duplicate-selection', 'straighten-selection', 'delete-selection', 'clear-selection']) {
      expect(block.querySelector(`[data-action="${name}"]`), name).toBeTruthy();
    }
    await openSideTab(target, 'help');
    expect(q('[data-slot="side-above"] [data-section="selection"]')).toBeTruthy();
    action('clear-selection').click();
    await tick();
    expect(target.querySelector('[data-section="selection"]')).toBeNull();
    expect(target.querySelector('[data-slot="side-above"]')).toBeNull();
  });

  it('has the « ✕ » named « Désélectionner », and duplicate and delete hooks that act', async () => {
    const { target, q, key, action } = await render();
    await key('v');
    await key('a', { ctrlKey: true });
    const clear = action('clear-selection');
    expect(clear.getAttribute('aria-label')).toBe('Désélectionner');
    expect(clear.title).toBe('Désélectionner');
    const strokes = () => target.querySelectorAll('g.ink path:not(.live)').length;
    expect(strokes()).toBe(2);
    action('duplicate-selection').click();
    await tick();
    expect(strokes()).toBe(4);
    action('delete-selection').click();
    await tick();
    expect(strokes()).toBe(2);
    expect(q('.status').textContent).toContain('non enregistrées');
  });

  it('reopens a collapsed pane when a selection appears, and not at each change of it', async () => {
    const { target, key, tab } = await render();
    await openSideTab(target, 'page');
    tab('page').click();
    await tick();
    expect(sideOf(target).collapsed).toBe(true);
    await key('v');
    await key('a', { ctrlKey: true });
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: false });
    // Collapsed by the user while the selection stays: it stays so.
    tab('page').click();
    await tick();
    expect(sideOf(target).collapsed).toBe(true);
    await key('a', { ctrlKey: true });
    expect(sideOf(target).collapsed).toBe(true);
    // The selection goes, comes back: reopens.
    await key('Escape');
    expect(target.querySelector('[data-section="selection"]')).toBeNull();
    await key('a', { ctrlKey: true });
    expect(sideOf(target).collapsed).toBe(false);
  });

  it('shows the split panel above the tab, reopens the pane for it, and leaves with it', async () => {
    const { target, q, toolbarButton, tab } = await render();
    tab('layers').click();
    await tick();
    expect(sideOf(target).collapsed).toBe(true);
    toolbarButton('Découper').click();
    await tick();
    expect(sideOf(target).collapsed).toBe(false);
    const split = q('[data-slot="side-above"] .split-panel');
    expect(split).toBeTruthy();
    expect(split.classList.contains('pane-block')).toBe(true);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await tick();
    expect(target.querySelector('.split-panel')).toBeNull();
    expect(target.querySelector('[data-slot="side-above"]')).toBeNull();
  });
});

describe('editor side panel: badges', () => {
  it('counts the layers and warns when the chosen one is hidden', async () => {
    const { target, tab, panel } = await render();
    expect(tab('layers').dataset.count).toBe('1');
    expect(tab('layers').hasAttribute('data-dot')).toBe(false);
    panel('layers').querySelector<HTMLInputElement>('[data-action="layer-visible"]')!.click();
    await tick();
    expect(tab('layers').dataset.dot).toBe('warn');
    expect(tab('layers').textContent).toContain('un calque utilisé est masqué');
    panel('layers').querySelector<HTMLInputElement>('[data-action="layer-visible"]')!.click();
    await tick();
    expect(tab('layers').hasAttribute('data-dot')).toBe(false);
    panel('layers').querySelector<HTMLButtonElement>('[data-action="add-layer"]')!.click();
    await tick();
    expect(tab('layers').dataset.count).toBe('2');
    for (const id of ['page', 'background', 'help']) {
      expect(tab(id).hasAttribute('data-count') || tab(id).hasAttribute('data-dot'), id).toBe(false);
    }
    expect(target.querySelector('.side-tab-count')?.textContent).toBe('2');
  });

  it('shows the blue dot on Projection while a share runs, even collapsed', async () => {
    const { tab } = await render();
    expect(tab('projection').hasAttribute('data-dot')).toBe(false);
    share.follow();
    await share.start();
    await vi.waitFor(() => expect(tab('projection').dataset.dot).toBe('live'));
    await share.stop();
    await vi.waitFor(() => expect(tab('projection').hasAttribute('data-dot')).toBe(false));
  });

  it('shows the red dot on Son while a narration records, and only then', async () => {
    const path = demoPath('Liste de courses');
    const entry = mockLibrary().notes.get(path)!;
    entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000)] };
    const { target, tab, action } = await render('Liste de courses');
    vi.useFakeTimers();
    action('sound-replay').click();
    await tick();
    expect(tab('sound').hasAttribute('data-dot')).toBe(false);
    action('narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    action('narration-start').click();
    await vi.waitFor(() => expect(tab('sound').dataset.dot).toBe('rec'));
    expect(tab('sound').textContent).toContain('enregistrement en cours');
    await vi.advanceTimersByTimeAsync(300);
    action('narration-cancel').click();
    await vi.waitFor(() => expect(tab('sound').hasAttribute('data-dot')).toBe(false));
    expect(target.querySelector('.replay-bar')).toBeTruthy();
  });
});
