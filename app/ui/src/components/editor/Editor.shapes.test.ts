// @vitest-environment jsdom
/// <reference types="node" />
// The editor's milestone M15 feature: the setting of "Redresser les formes" (split button, panel,
// preview on the page, values kept). The recognition is stubbed: it follows the minimum size, so
// that the count and the preview can be seen to follow the sliders.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { hline, line } from '../../lib/editor/test-fixtures';
import { setLocale } from '../../lib/i18n.svelte';
import type { ShapeStroke, ShapeTuning, Straightened } from '../../lib/ink';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { ShapeSettings, Stroke } from '../../lib/note';
import { DEFAULT_SHAPE_SETTINGS, SHAPES_SAVE_DELAY_MS } from '../../lib/shape-settings';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

const seen = vi.hoisted(() => ({ tunings: [] as (ShapeTuning | undefined)[], results: [] as Straightened[][] }));

// The real stroke geometry (the outlines), and a recognition that takes a flat stroke for a line
// when it is at least `minSizeMm` long (the real one is tested in Rust and in `ink.test.ts`).
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const real = await actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  const straighten = (strokes: readonly ShapeStroke[], tuning?: ShapeTuning): Straightened[] => {
    seen.tunings.push(tuning);
    const groups = strokes.flatMap((stroke, i): Straightened[] => {
      const [first, last] = [stroke.points[0], stroke.points[stroke.points.length - 1]];
      const flat = stroke.points.every((p) => p.y === first.y);
      const millimetres = Math.abs(last.x - first.x) / 100;
      if (!flat || millimetres < (tuning?.minSizeMm ?? 10)) return [];
      return [{ sources: [i], shape: 'line', strokes: [{ ...stroke, points: [first, { ...last, y: first.y }] }] }];
    });
    seen.results.push(groups);
    return groups;
  };
  return { ...actual, loadInk: () => Promise.resolve({ ...real, straighten }) };
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
  shapes: { ...DEFAULT_SHAPE_SETTINGS },
};

let cleanup: (() => void) | null = null;
let saved: MockInstance<typeof api.setShapes>;

beforeEach(() => {
  resetMockLibrary();
  setLocale('fr');
  seen.tunings.length = 0;
  seen.results.length = 0;
  saved = vi.spyOn(api, 'setShapes').mockImplementation(async (shapes) => ({ ...app, shapes }));
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

/** The strokes of the test note: a 7 mm line, a 30 mm line and a wavy stroke that is no shape. */
const seven = hline(1000, 1700, 1000, 50);
const thirty = hline(1000, 4000, 3000, 50);
const wavy = line([
  [1000, 6000],
  [2000, 6300],
  [3000, 5800],
  [4000, 6400],
]);

async function render(shapes: Partial<ShapeSettings> = {}, strokes: Stroke[] = [seven, thirty, wavy]) {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes('Liste de courses'))!;
  const entry = mockLibrary().notes.get(path)!;
  entry.note = { ...entry.note, strokes };
  const target = document.createElement('div');
  document.body.append(target);
  const onsaved = vi.fn();
  const onchange = vi.fn();
  const component = mount(Editor, {
    target,
    props: { path, app: { ...app, shapes: { ...DEFAULT_SHAPE_SETTINGS, ...shapes } }, onclose: vi.fn(), onsaved, onchange },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const q = <T extends Element>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => {
    const found = q<HTMLButtonElement>(`button[data-action="${name}"]`);
    if (!found) throw new Error(`no button ${name}`);
    return found;
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const panel = () => q<HTMLElement>('[data-panel="straighten"]');
  const more = () => action('straighten-settings');
  const count = () => q('[data-slot="straighten-count"]')?.textContent?.trim();
  const slider = (id: string) => q<HTMLInputElement>(`#straighten-panel-${id}`)!;
  const slide = async (id: string, value: number) => {
    const input = slider(id);
    input.value = String(value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const check = async (field: string, on: boolean) => {
    const box = q<HTMLInputElement>(`input[data-field="${field}"]`)!;
    box.checked = on;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
  };
  const notice = () => q('.editor-notice')?.textContent?.trim();
  /** The strokes drawn on the page (the outlines), as path data. */
  const paths = () => [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')].map((p) => p.getAttribute('d'));
  const previewLines = () => [...target.querySelectorAll<SVGPathElement>('g.straighten-preview path')];
  const faded = () => target.querySelectorAll('g.ink path.straighten-faded').length;
  const selectAll = async () => {
    await key('v');
    await key('a', { ctrlKey: true });
  };
  const openPanel = async () => {
    more().click();
    await tick();
    await vi.waitFor(() => expect(panel()).toBeTruthy());
  };
  const stored = () => mockLibrary().notes.get(path)!.note.strokes;
  const save = async () => {
    onsaved.mockClear();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
  };
  return { target, onchange, q, action, key, panel, more, count, slider, slide, check, notice, paths, previewLines, faded, selectAll, openPanel, stored, save };
}

describe('the split button and its panel', () => {
  it('has a small button beside the main one, which straightens as before', async () => {
    const { action, more, panel, key, selectAll, notice, paths } = await render();
    await key('v');
    // Nothing selected: the main button waits, the setting can still be opened.
    expect(action('straighten').disabled).toBe(true);
    expect(more().disabled).toBe(false);
    expect(more().getAttribute('aria-label')).toBe('Réglage du redressement');
    expect(more().getAttribute('aria-expanded')).toBe('false');
    expect(more().getAttribute('aria-controls')).toBe('straighten-panel');
    expect(panel()).toBeNull();

    await selectAll();
    const before = paths();
    action('straighten').click();
    await tick();
    // Default values: the 30 mm line only (7 mm is under the minimum).
    expect(notice()).toBe('1 forme redressée (Ctrl+Z pour annuler).');
    expect(paths()).not.toEqual(before);
    expect(panel()).toBeNull();
  });

  it('opens with the small button and closes with it, with Escape and with a click outside', async () => {
    const { more, panel, key, selectAll, openPanel, action } = await render();
    await selectAll();
    await openPanel();
    expect(more().getAttribute('aria-expanded')).toBe('true');
    expect(panel()?.getAttribute('role')).toBe('dialog');

    more().click();
    await tick();
    expect(panel()).toBeNull();
    expect(more().getAttribute('aria-expanded')).toBe('false');

    // Escape closes it, without straightening and without deselecting, and the focus goes back to the small button.
    await openPanel();
    await key('Escape');
    expect(panel()).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(more()));
    expect(document.body.textContent).toContain('Traits sélectionnés : 3');
    expect(action('straighten').disabled).toBe(false);

    // A press anywhere else closes it too.
    await openPanel();
    document.body.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    await tick();
    expect(panel()).toBeNull();
    // ...but not a press inside it.
    await openPanel();
    panel()!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    await tick();
    expect(panel()).not.toBeNull();
  });

  it('goes with the selection tool', async () => {
    const { panel, key, selectAll, openPanel } = await render();
    await selectAll();
    await openPanel();
    await key('p');
    expect(panel()).toBeNull();
  });

  it('shows the values of the app state, brought within their limits', async () => {
    const { q, openPanel, selectAll, slider } = await render({ tolerance: 5, minSizeMm: 7, snapAngle: false, snapRoundPercent: 15 });
    await selectAll();
    await openPanel();
    expect(slider('tolerance').value).toBe('5');
    expect(q('[data-slot="tolerance-value"]')?.textContent).toBe('souple');
    expect(slider('size').value).toBe('7');
    expect(q('[data-slot="size-value"]')?.textContent).toBe('7 mm');
    expect(q<HTMLInputElement>('input[data-field="snap-angle"]')?.checked).toBe(false);
    expect(slider('angle').disabled).toBe(true);
    expect(slider('round').value).toBe('15');
  });

  it('brings a value of the file that is out of limits back within them', async () => {
    const { q, openPanel, selectAll, slider } = await render({ tolerance: 9, minSizeMm: 99, snapAngleDeg: 0, snapRoundPercent: 99 });
    await selectAll();
    await openPanel();
    expect(slider('tolerance').value).toBe('5');
    expect(slider('size').value).toBe('25');
    expect(q('[data-slot="size-help"]')?.textContent?.trim()).toBe(
      "Droites et flèches d'au moins 25 mm ; cercles, ellipses et rectangles d'au moins 28 mm.",
    );
    expect(slider('angle').value).toBe('1');
    expect(slider('round').value).toBe('20');
  });
});

describe('the count and the preview', () => {
  it('says there is nothing to preview without a selection', async () => {
    const { key, openPanel, count, action } = await render();
    await key('v');
    await openPanel();
    expect(count()).toBe('Sélectionnez des traits avec le lasso pour voir ce qui serait redressé.');
    expect(action('straighten-apply').disabled).toBe(true);
  });

  it('counts the shapes and draws them dashed over the faded originals, following the minimum size', async () => {
    const { selectAll, openPanel, count, slide, previewLines, faded, action, q } = await render();
    await selectAll();
    await openPanel();
    expect(count()).toBe('Avec ces valeurs : 1 forme sur 3 traits sélectionnés.');
    expect(previewLines()).toHaveLength(1);
    expect(faded()).toBe(1);
    expect(q('[data-slot="size-help"]')?.textContent?.trim()).toBe(
      "Droites et flèches d'au moins 10 mm ; cercles, ellipses et rectangles d'au moins 13 mm.",
    );
    // The preview is a line from the first point to the last one of the straightened stroke.
    expect(previewLines()[0].getAttribute('d')).toBe('M1000 3000L4000 3000');

    await slide('size', 5);
    await vi.waitFor(() => expect(count()).toBe('Avec ces valeurs : 2 formes sur 3 traits sélectionnés.'));
    expect(previewLines()).toHaveLength(2);
    expect(faded()).toBe(2);
    expect(action('straighten-apply').disabled).toBe(false);

    await slide('size', 25);
    await vi.waitFor(() => expect(count()).toBe('Avec ces valeurs : 1 forme sur 3 traits sélectionnés.'));
    await slide('size', 25);
    // The 30 mm line is still long enough at 25 mm; nothing is faded but it.
    expect(faded()).toBe(1);
  });

  it('says so when no shape is recognised, and « Redresser » waits', async () => {
    const { selectAll, openPanel, count, slide, previewLines, faded, action } = await render({}, [seven, wavy]);
    await selectAll();
    await openPanel();
    expect(count()).toBe("Avec ces valeurs, aucune forme n'est reconnue dans la sélection.");
    expect(previewLines()).toHaveLength(0);
    expect(faded()).toBe(0);
    expect(action('straighten-apply').disabled).toBe(true);
    expect(action('straighten-apply').title).toBe('Aucune forme à redresser avec ces valeurs.');
    // The 7 mm line is a shape once the minimum goes down to 7 mm.
    await slide('size', 7);
    await vi.waitFor(() => expect(count()).toBe('Avec ces valeurs : 1 forme sur 2 traits sélectionnés.'));
    expect(action('straighten-apply').disabled).toBe(false);
  });

  it('draws the preview at most once per frame while a slider moves', async () => {
    const { selectAll, openPanel, slide, count } = await render();
    await selectAll();
    await openPanel();
    const before = seen.tunings.length;
    for (const value of [9, 8, 7, 6, 5]) await slide('size', value);
    await vi.waitFor(() => expect(count()).toBe('Avec ces valeurs : 2 formes sur 3 traits sélectionnés.'));
    // Five changes in a row: far fewer recognitions than changes (the last frame's one at least).
    const runs = seen.tunings.length - before;
    expect(runs).toBeGreaterThanOrEqual(1);
    expect(runs).toBeLessThan(5);
    expect(seen.tunings[seen.tunings.length - 1]).toMatchObject({ minSizeMm: 5 });
  });

  it('gives the recognition the tuning of the panel, an unticked box meaning no alignment', async () => {
    const { selectAll, openPanel, slide, check, count } = await render();
    await selectAll();
    await openPanel();
    expect(seen.tunings[seen.tunings.length - 1]).toEqual({ tolerance: 3, minSizeMm: 10, snapAngleDeg: 4, snapRoundPercent: 10 });
    await slide('tolerance', 5);
    await slide('angle', 7);
    await check('snap-round', false);
    await vi.waitFor(() =>
      expect(seen.tunings[seen.tunings.length - 1]).toEqual({ tolerance: 5, minSizeMm: 10, snapAngleDeg: 7, snapRoundPercent: null }),
    );
    expect(count()).toContain('1 forme');
  });

  it('draws nothing on the page when the panel is closed, and changes nothing in the note', async () => {
    const { selectAll, openPanel, more, previewLines, faded, paths, slide } = await render();
    await selectAll();
    const before = paths();
    await openPanel();
    await slide('size', 5);
    expect(paths()).toEqual(before);
    more().click();
    await tick();
    expect(previewLines()).toHaveLength(0);
    expect(faded()).toBe(0);
    expect(paths()).toEqual(before);
  });

  it('warns that letters may be straightened below 1 cm', async () => {
    const { selectAll, openPanel, slide, q } = await render();
    await selectAll();
    await openPanel();
    expect(q('[data-slot="size-warning"]')).toBeNull();
    await slide('size', 9);
    expect(q('[data-slot="size-warning"]')?.textContent).toContain('En dessous de 1 cm, des lettres peuvent être redressées');
    await slide('size', 10);
    expect(q('[data-slot="size-warning"]')).toBeNull();
  });

  it('disables the slider of an alignment whose box is unticked', async () => {
    const { selectAll, openPanel, check, slider } = await render();
    await selectAll();
    await openPanel();
    expect(slider('angle').disabled).toBe(false);
    expect(slider('round').disabled).toBe(false);
    await check('snap-angle', false);
    expect(slider('angle').disabled).toBe(true);
    expect(slider('round').disabled).toBe(false);
    await check('snap-round', false);
    expect(slider('round').disabled).toBe(true);
    await check('snap-angle', true);
    expect(slider('angle').disabled).toBe(false);
  });

  it('is in English too', async () => {
    setLocale('en');
    const { selectAll, openPanel, count, more, q } = await render();
    await selectAll();
    await openPanel();
    expect(more().getAttribute('aria-label')).toBe('Straightening settings');
    expect(count()).toBe('With these values: 1 shape out of 3 selected strokes.');
    expect(q('[data-slot="tolerance-value"]')?.textContent).toBe('normal');
  });
});

describe('straightening with the values', () => {
  it('« Redresser » applies exactly what the preview showed, as one undoable step', async () => {
    const { selectAll, openPanel, slide, action, key, paths, stored, save, notice, panel, previewLines } = await render();
    await selectAll();
    const drawn = paths();
    await openPanel();
    await slide('size', 5);
    await vi.waitFor(() => expect(previewLines()).toHaveLength(2));
    const shown = previewLines().map((p) => p.getAttribute('d'));
    expect(shown).toEqual(['M1000 1000L1700 1000', 'M1000 3000L4000 3000']);

    action('straighten-apply').click();
    await tick();
    expect(notice()).toBe('2 formes redressées (Ctrl+Z pour annuler).');
    // The panel and the preview are gone, the focus is back on the small button.
    expect(panel()).toBeNull();
    expect(previewLines()).toHaveLength(0);
    const straightened = paths();
    expect(straightened).toHaveLength(3);
    expect(straightened[2]).toBe(drawn[2]);

    await save();
    const strokes = stored();
    expect(strokes).toHaveLength(3);
    expect(strokes.slice(0, 2).map((s: Stroke) => `M${s.points.map((p) => `${p.x} ${p.y}`).join('L')}`)).toEqual(shown);
    expect(strokes[2]).toEqual(wavy);

    await key('z', { ctrlKey: true });
    expect(paths()).toEqual(drawn);
    await key('y', { ctrlKey: true });
    expect(paths()).toEqual(straightened);
  });

  it('keeps the setting for the F key and the button of the selection panel', async () => {
    const { selectAll, openPanel, slide, key, action, notice, paths, panel, more } = await render();
    await selectAll();
    await openPanel();
    await slide('size', 5);
    await key('Escape');
    expect(panel()).toBeNull();
    const drawn = paths();

    await key('f');
    expect(notice()).toBe('2 formes redressées (Ctrl+Z pour annuler).');
    expect(seen.tunings[seen.tunings.length - 1]).toMatchObject({ minSizeMm: 5 });
    await key('z', { ctrlKey: true });
    expect(paths()).toEqual(drawn);

    action('straighten-selection').click();
    await tick();
    expect(notice()).toBe('2 formes redressées (Ctrl+Z pour annuler).');
    expect(seen.tunings[seen.tunings.length - 1]).toMatchObject({ minSizeMm: 5 });
    expect(more().getAttribute('aria-expanded')).toBe('false');
  });

  it('with the default values gives what straightening gave before the setting', async () => {
    const { selectAll, key, notice, paths } = await render();
    await selectAll();
    const drawn = paths();
    await key('f');
    expect(seen.tunings[seen.tunings.length - 1]).toEqual({ tolerance: 3, minSizeMm: 10, snapAngleDeg: 4, snapRoundPercent: 10 });
    expect(notice()).toBe('1 forme redressée (Ctrl+Z pour annuler).');
    expect(paths()[0]).toBe(drawn[0]);
  });

  it('tells in the notice which sizes are asked when nothing is recognised', async () => {
    const { selectAll, key, notice } = await render({ minSizeMm: 12 }, [seven, wavy]);
    await selectAll();
    await key('f');
    expect(notice()).toContain('Aucune forme reconnue dans la sélection.');
    expect(notice()).toContain("d'au moins 12 mm");
    expect(notice()).toContain("d'au moins 15 mm");
  });
});

describe('keeping the values', () => {
  it('keeps them through api.setShapes a moment after the last change, once', async () => {
    const { selectAll, openPanel, slide } = await render();
    await selectAll();
    await openPanel();
    await slide('size', 8);
    await slide('size', 7);
    await slide('tolerance', 4);
    expect(saved).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(saved).toHaveBeenCalledTimes(1), { timeout: SHAPES_SAVE_DELAY_MS * 5 });
    expect(saved).toHaveBeenCalledWith({ ...DEFAULT_SHAPE_SETTINGS, tolerance: 4, minSizeMm: 7 });
    await new Promise((resolve) => setTimeout(resolve, SHAPES_SAVE_DELAY_MS + 100));
    expect(saved).toHaveBeenCalledTimes(1);
  });

  it('keeps them at once when the panel closes, and tells the screen holding the app state', async () => {
    const { selectAll, openPanel, slide, key, onchange } = await render();
    await selectAll();
    await openPanel();
    await slide('size', 6);
    expect(saved).not.toHaveBeenCalled();
    await key('Escape');
    await vi.waitFor(() => expect(saved).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ shapes: expect.objectContaining({ minSizeMm: 6 }) })));
  });

  it('keeps them when the editor goes away before the delay', async () => {
    const { selectAll, openPanel, slide } = await render();
    await selectAll();
    await openPanel();
    await slide('size', 12);
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(saved).toHaveBeenCalledWith({ ...DEFAULT_SHAPE_SETTINGS, minSizeMm: 12 }));
  });

  it('finds them again in the next editor, from the app state they were kept in', async () => {
    const first = await render();
    await first.selectAll();
    await first.openPanel();
    await first.slide('size', 7);
    await first.slide('tolerance', 2);
    await first.key('Escape');
    const kept = saved.mock.calls[0][0];
    cleanup?.();
    cleanup = null;

    const second = await render(kept);
    await second.selectAll();
    await second.openPanel();
    expect(second.slider('size').value).toBe('7');
    expect(second.slider('tolerance').value).toBe('2');
    expect(second.count()).toBe('Avec ces valeurs : 2 formes sur 3 traits sélectionnés.');
  });

  it('puts the default values back, which keep what straightening gave before', async () => {
    const { selectAll, openPanel, slide, check, action, slider, count, q } = await render({ tolerance: 5, minSizeMm: 5, snapAngle: false });
    await selectAll();
    await openPanel();
    await slide('angle', 9);
    expect(count()).toContain('2 formes');
    action('straighten-defaults').click();
    await tick();
    expect(slider('tolerance').value).toBe('3');
    expect(slider('size').value).toBe('10');
    expect(q<HTMLInputElement>('input[data-field="snap-angle"]')?.checked).toBe(true);
    expect(slider('angle').value).toBe('4');
    expect(slider('round').value).toBe('10');
    await vi.waitFor(() => expect(count()).toBe('Avec ces valeurs : 1 forme sur 3 traits sélectionnés.'));
    await check('snap-angle', true);
    await vi.waitFor(() => expect(saved).toHaveBeenCalledWith({ ...DEFAULT_SHAPE_SETTINGS }));
  });

  it('says so when the values cannot be kept, and keeps them on the page', async () => {
    saved.mockRejectedValue({ code: 'settings', message: 'disque plein' });
    const { selectAll, openPanel, slide, key, notice, slider } = await render();
    await selectAll();
    await openPanel();
    await slide('size', 6);
    await key('Escape');
    await vi.waitFor(() => expect(notice()).toContain("Le réglage n'a pas pu être enregistré"));
    await openPanel();
    expect(slider('size').value).toBe('6');
  });
});

describe('fitting in the window', () => {
  /** The room the panel has: the button's place is given, jsdom having no layout. */
  async function open(bottom: number, left: number) {
    const view = await render();
    await view.selectAll();
    const split = view.more().parentElement!;
    split.getBoundingClientRect = () => ({ left, right: left + 200, top: bottom - 30, bottom, width: 200, height: 30, x: left, y: bottom - 30, toJSON: () => ({}) });
    await view.openPanel();
    return view;
  }
  const set = (property: 'innerHeight' | 'innerWidth', value: number) => Object.defineProperty(window, property, { value, configurable: true, writable: true });
  const room = (view: Awaited<ReturnType<typeof open>>) => view.panel()!.style.getPropertyValue('--room');
  const shift = (view: Awaited<ReturnType<typeof open>>) => view.panel()!.style.getPropertyValue('--shift');
  let height = 0;
  let width = 0;

  beforeEach(() => {
    height = window.innerHeight;
    width = window.innerWidth;
  });
  afterEach(() => {
    set('innerHeight', height);
    set('innerWidth', width);
  });

  it('is never taller than the room down to the bottom of the window, and follows the window', async () => {
    set('innerHeight', 700);
    const view = await open(60, 20);
    // 700 - 60 (the button's bottom) - 6 (the gap) - 12 (the margin).
    expect(room(view)).toBe('622px');
    set('innerHeight', 400);
    window.dispatchEvent(new Event('resize'));
    expect(room(view)).toBe('322px');
  });

  it('stays usable on a very low window: it scrolls, never shorter than 160 px', async () => {
    set('innerHeight', 200);
    const view = await open(60, 20);
    expect(room(view)).toBe('160px');
  });

  it('keeps its buttons at its bottom, whatever its height, and scrolls inside', async () => {
    // jsdom applies no stylesheet of the component: the rules are read from the source.
    const { readFileSync } = await import('node:fs');
    const { resolve } = await import('node:path');
    const source = readFileSync(resolve(process.cwd(), 'ui/src/components/editor/StraightenPanel.svelte'), 'utf8');
    const rule = (selector: string) => source.match(new RegExp(`\\n  ${selector.replace('.', '\\.')} \\{([^}]*)\\}`))?.[1] ?? '';
    expect(rule('.panel')).toMatch(/max-height: var\(--room/);
    expect(rule('.panel')).toMatch(/overflow-y: auto/);
    expect(rule('.panel')).toMatch(/overscroll-behavior: contain/);
    expect(rule('.panel')).toMatch(/width: min\(360px, calc\(100vw - 24px\)\)/);
    expect(rule('.actions')).toMatch(/position: sticky/);
    expect(rule('.actions')).toMatch(/bottom: 0/);
    expect(rule('.actions')).toMatch(/background: var\(--surface\)/);
  });

  it('moves left when its button is so far right that it would leave the window', async () => {
    set('innerWidth', 500);
    set('innerHeight', 700);
    const view = await open(60, 400);
    // Width 360 (500 - 24 margin is 476): the panel ends 12 px from the right edge.
    expect(shift(view)).toBe('-272px');
    set('innerWidth', 300);
    window.dispatchEvent(new Event('resize'));
    // Width 276: it starts 12 px from the left edge.
    expect(shift(view)).toBe('-388px');
  });

  it('is not moved when its button is near the left', async () => {
    set('innerWidth', 1200);
    const view = await open(60, 100);
    expect(shift(view)).toBe('0px');
  });
});
