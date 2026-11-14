// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { freshStamp, mockLibrary, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { mockClipboard } from '../../lib/mock-export';
import { mockNarration, resetMockNarration, setMockNarration } from '../../lib/mock-narration';
import { BLACK, bigNote } from '../../lib/editor/test-fixtures';
import type { WindowClose } from '../../lib/editor/window-close';
import type { Stroke } from '../../lib/note';
import Editor from './Editor.svelte';
import EditorHarness from './EditorHarness.svelte';
import { bridgeScreens } from '../../lib/app-bridge.svelte';

// The video export dialog is another work package: the editor is tested against a stand-in that
// shows what the editor gave it.
vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

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
  resetMockLibrary();
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
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** Lets pending promises (mock commands, the write queue) run. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A stand-in for the desktop window: `request()` is the user clicking its close button. */
function fakeWindow() {
  let guard: (() => boolean) | null = null;
  const unlisten = vi.fn();
  const destroy = vi.fn(async () => {});
  const closer: WindowClose = {
    subscribe: async (g) => {
      guard = g;
      return unlisten;
    },
    destroy,
  };
  return { closer, request: () => guard?.() ?? false, destroy, unlisten };
}

async function render(
  title: string,
  extra: { windowClose?: WindowClose; onchange?: (next: AppState) => void } = {},
) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved, ...extra } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;

  const paths = () => [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')];
  const fills = () => paths().map((p) => p.getAttribute('fill'));
  /** Visible name of a button, without its decorative (aria-hidden) icon. */
  const name = (b: HTMLButtonElement) => {
    const copy = b.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[aria-hidden="true"]').forEach((icon) => icon.remove());
    return copy.textContent?.trim();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find(
      (b) => b.getAttribute('aria-label') === label || b.title === label || name(b) === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  const input = (label: string) => {
    const found = target.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
    if (!found) throw new Error(`no input "${label}"`);
    return found;
  };
  /** Page coordinates → client coordinates (the canvas sits at the top-left corner in jsdom). */
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number, init: { button?: number; shiftKey?: boolean; ctrlKey?: boolean } = {}) => {
    const at = client(x, y);
    const options = {
      bubbles: true,
      cancelable: true,
      clientX: at.x,
      clientY: at.y,
      button: init.button ?? 0,
      buttons: 1,
      shiftKey: init.shiftKey ?? false,
      ctrlKey: init.ctrlKey ?? false,
      pointerId: 1,
      pressure: 0.5,
    };
    const Ctor = (globalThis as { PointerEvent?: typeof MouseEvent }).PointerEvent ?? MouseEvent;
    const event = new Ctor(type, options);
    for (const key of ['pointerId', 'pressure'] as const) {
      if (!(key in event)) Object.defineProperty(event, key, { value: options[key] });
    }
    svg.dispatchEvent(event);
  };
  /** A drag through page points. */
  const drag = async (points: [number, number][], init: { shiftKey?: boolean; ctrlKey?: boolean } = {}) => {
    const [first, ...rest] = points;
    pointer('pointerdown', first[0], first[1], init);
    for (const [x, y] of rest) pointer('pointermove', x, y, init);
    const last = points[points.length - 1];
    pointer('pointerup', last[0], last[1], init);
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const status = () => target.querySelector('.status')?.textContent?.trim();
  const titleInput = () => target.querySelector<HTMLInputElement>('.title-field input')!;
  /** Types a new title in the title field (not committed yet). */
  const typeTitle = (text: string) => {
    const field = titleInput();
    field.focus();
    field.value = text;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const blurTitle = () => titleInput().dispatchEvent(new FocusEvent('blur'));
  return {
    path,
    target,
    svg,
    paths,
    fills,
    button,
    input,
    pointer,
    drag,
    key,
    status,
    typeTitle,
    blurTitle,
    titleInput,
    onclose,
    onsaved,
  };
}

const line = (x0: number, x1: number, y: number, steps = 20): [number, number][] =>
  Array.from({ length: steps + 1 }, (_, i) => [x0 + ((x1 - x0) * i) / steps, y]);

describe('note editor', () => {
  it('opens a demo note and draws its strokes over the background, in French', async () => {
    const { target, svg, paths, button, status } = await render('Liste de courses');
    expect(paths()).toHaveLength(1);
    expect(paths()[0].getAttribute('d')).toMatch(/^M.*Z$/);
    expect(paths()[0].getAttribute('fill-rule')).toBe('nonzero');
    expect(svg.getAttribute('data-page-width')).toBe('29700');
    await vi.waitFor(() =>
      expect(target.querySelector('image')?.getAttribute('href')).toMatch(/^data:image\/svg\+xml/),
    );
    expect(target.querySelector<HTMLInputElement>('.title-field input')?.value).toBe('Liste de courses');
    expect(status()).toBe('Enregistrée');
    for (const label of ['Bibliothèque', 'Enregistrer', 'Annuler (Ctrl+Z)', 'Rétablir (Ctrl+Y)', 'Stylo', 'Gomme', 'Rouge']) {
      expect(button(label)).toBeTruthy();
    }
    expect(target.textContent).toContain('Calques');
    expect(target.textContent).toContain('Épaisseur');
    expect(target.textContent).toContain('Stylo à bille');
  });

  it('shows the editor in English', async () => {
    setLocale('en');
    const { target, button } = await render('Liste de courses');
    for (const label of ['Library', 'Save', 'Undo (Ctrl+Z)', 'Pen', 'Eraser', 'Select', 'Black', 'Orange']) {
      expect(button(label)).toBeTruthy();
    }
    expect(target.textContent).toContain('Layers');
    expect(target.textContent).toContain('Ballpoint pen');
    expect(target.textContent).toContain('Crop around the content');
  });

  it('recolours the whole note, then undoes and redoes it', async () => {
    const { fills, button, key, status } = await render('Cours de physique');
    const original = fills();
    expect(original).toHaveLength(2);
    button('Rouge').click();
    await tick();
    // Picking a colour with nothing selected only changes the pen.
    expect(fills()).toEqual(original);
    button('Couleur').click();
    await tick();
    expect(fills()).toEqual(['rgba(216, 0, 0, 1.000)', 'rgba(216, 0, 0, 1.000)']);
    expect(status()).toBe('Modifications non enregistrées');
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(fills()).toEqual(original);
    expect(status()).toBe('Enregistrée');
    await key('y', { ctrlKey: true });
    expect(fills()).toEqual(['rgba(216, 0, 0, 1.000)', 'rgba(216, 0, 0, 1.000)']);
    await key('z', { ctrlKey: true });
    expect(fills()).toEqual(original);
    await key('z', { ctrlKey: true, shiftKey: true });
    expect(fills()[0]).toBe('rgba(216, 0, 0, 1.000)');
  });

  it('draws strokes with the pen, erases one, and saves', async () => {
    const { path, paths, drag, button, status, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(3);
    expect(status()).toBe('Modifications non enregistrées');

    button('Gomme').click();
    await tick();
    await drag(line(4000, 16000, 18000));
    expect(paths()).toHaveLength(2);

    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledWith(path));
    expect(status()).toBe('Enregistrée');
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes).toHaveLength(2);
    const drawn = stored.strokes[1];
    expect(drawn.tool).toBe('ballpoint');
    expect(drawn.layer).toBe(0);
    expect(drawn.points.length).toBeGreaterThan(10);
    expect(drawn.points[0].x).toBeCloseTo(5000, -1);
    expect(drawn.points[0].y).toBeCloseTo(15000, -1);
    // Drawn after the existing strokes on the replay timeline.
    expect(drawn.points[0].t).toBeGreaterThan(Math.max(...stored.strokes[0].points.map((p) => p.t)));
  });

  it('erases part of a stroke, cutting it in two', async () => {
    const { paths, drag, button, target } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Gomme').click();
    await tick();
    const partial = [...target.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find((r) =>
      r.parentElement?.textContent?.includes('Partielle'),
    )!;
    partial.click();
    await tick();
    await drag([
      [10000, 14000],
      [10000, 16000],
    ]);
    expect(paths()).toHaveLength(3);
  });

  it('shows a clear banner when the note changed on disk meanwhile', async () => {
    const { path, target, drag, key, status, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    const banner = target.querySelector('[role="alert"]')!.textContent ?? '';
    expect(banner).toContain("Vos modifications n'ont pas été enregistrées");
    expect(banner).toContain('la note a été modifiée entre-temps');
    expect(onsaved).not.toHaveBeenCalled();
    expect(status()).toBe('Modifications non enregistrées');
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(1);
  });

  it('hides and shows layers, and draws on a new layer', async () => {
    const { paths, button, drag } = await render('Cours de physique');
    expect(paths()).toHaveLength(2);
    button('Masquer « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(0);
    button('Afficher « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(2);

    button('+ Ajouter un calque').click();
    await tick();
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    button('Masquer « Calque 2 »').click();
    await tick();
    expect(paths()).toHaveLength(2);
    button('Masquer « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(0);
  });

  it('rotates the page a quarter turn', async () => {
    const { svg, button } = await render('Liste de courses');
    button("Tourner la page d'un quart de tour vers la droite").click();
    await tick();
    expect(svg.getAttribute('data-page-width')).toBe('21600');
    expect(svg.getAttribute('data-page-height')).toBe('29700');
    button("Tourner la page d'un quart de tour vers la gauche").click();
    await tick();
    expect(svg.getAttribute('data-page-width')).toBe('29700');
  });

  it('crops automatically around the content', async () => {
    const { svg, button } = await render('Liste de courses');
    button('Recadrer autour du contenu').click();
    await tick();
    expect(Number(svg.getAttribute('data-page-width'))).toBeLessThan(29700);
    expect(Number(svg.getAttribute('data-page-height'))).toBeLessThan(21600);
  });

  it('crops by hand to a dragged rectangle', async () => {
    const { svg, button, drag } = await render('Liste de courses');
    button('Recadrer à la main').click();
    await tick();
    await drag([
      [1000, 1000],
      [11000, 9000],
    ]);
    expect(svg.getAttribute('data-page-width')).toBe('10000');
    expect(svg.getAttribute('data-page-height')).toBe('8000');
  });

  it('selects with the lasso, deletes, undoes, and duplicates with Ctrl+C / Ctrl+V', async () => {
    const { target, paths, button, drag, key } = await render('Liste de courses');
    button('Sélection').click();
    await tick();
    await drag([
      [2000, 3000],
      [13000, 3000],
      [13000, 5000],
      [2000, 5000],
      [2000, 3000],
    ]);
    expect(target.querySelector('rect.selection')).toBeTruthy();
    expect(target.textContent).toContain('Traits sélectionnés : 1');
    await key('Delete');
    expect(paths()).toHaveLength(0);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(1);
    await key('a', { ctrlKey: true });
    // The browser turns Ctrl+C / Ctrl+V into clipboard events; the system clipboard is faked.
    let text = '';
    const clipboardData = { setData: (_type: string, value: string) => (text = value), getData: () => text };
    const fire = async (type: 'copy' | 'paste') => {
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'clipboardData', { value: clipboardData });
      document.body.dispatchEvent(event);
      await tick();
      return event.defaultPrevented;
    };
    expect(await fire('copy')).toBe(true);
    expect(text).toContain('ardoise-strokes');
    expect(await fire('paste')).toBe(true);
    expect(paths()).toHaveLength(2);
    await key('Escape');
    expect(target.querySelector('rect.selection')).toBeNull();
  });

  it('removes strokes from the selection with Ctrl+lasso, even started on its frame', async () => {
    const { path, target, button, drag, key } = await render('Liste de courses');
    button('Sélection').click();
    await tick();
    // The Selection tool explains its gestures.
    expect(target.querySelector('.select-help')?.textContent).toBe(
      'Lasso : sélectionner · Maj+lasso : ajouter · Ctrl+lasso : retirer · Double-clic sur un texte ou une équation : le modifier · F : redresser les formes · Échap : tout désélectionner',
    );
    await key('a', { ctrlKey: true });
    expect(target.textContent).toContain('Traits sélectionnés : 1');
    const before = structuredClone(mockLibrary().notes.get(path)!.note.strokes[0].points[0]);
    const around: [number, number][] = [
      [7500, 3000],
      [13000, 3000],
      [13000, 5000],
      [2000, 5000],
      [2000, 3000],
      [7500, 3000],
    ];
    // Shift+lasso started inside the frame: a lasso adding what is already selected, not a move.
    await drag(around, { shiftKey: true });
    expect(target.textContent).toContain('Traits sélectionnés : 1');
    expect(target.querySelector('polyline.lasso')).toBeNull();
    // Ctrl+lasso started inside the frame: the stroke leaves the selection.
    await drag(around, { ctrlKey: true });
    expect(target.querySelector('rect.selection')).toBeNull();
    // Nothing moved.
    await key('s', { ctrlKey: true });
    expect(mockLibrary().notes.get(path)!.note.strokes[0].points[0]).toEqual(before);
  });

  it('moves the selection by dragging it, in one undo step', async () => {
    const { path, target, button, drag, key } = await render('Liste de courses');
    button('Sélection').click();
    await tick();
    await key('a', { ctrlKey: true });
    const before = mockLibrary().notes.get(path)!.note.strokes[0].points[0];
    await drag([
      [7500, 4000],
      [8000, 5000],
      [9500, 6000],
    ]);
    expect(target.querySelector('path[transform]')).toBeNull();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(mockLibrary().notes.get(path)!.note.strokes[0].points[0].x).not.toBe(before.x));
    const after = mockLibrary().notes.get(path)!.note.strokes[0].points[0];
    expect(after.x - before.x).toBeCloseTo(2000, -1);
    expect(after.y - before.y).toBeCloseTo(2000, -1);
    await key('z', { ctrlKey: true });
    expect(target.querySelector('.status')?.textContent?.trim()).toBe('Modifications non enregistrées');
  });

  it('remembers custom colours', async () => {
    const { target, button } = await render('Liste de courses');
    const picker = target.querySelector<HTMLInputElement>('input[type="color"]')!;
    picker.value = '#123456';
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(button('Couleur personnalisée #123456')).toBeTruthy();
    expect(localStorage.getItem('ardoise.editor.customColors')).toBe('["#123456"]');
  });

  it('renames the note from its title', async () => {
    const { path, target, onsaved } = await render('Liste de courses');
    const title = target.querySelector<HTMLInputElement>('.title-field input')!;
    title.value = 'Courses du samedi';
    title.dispatchEvent(new Event('input', { bubbles: true }));
    title.dispatchEvent(new FocusEvent('blur'));
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const renamed = onsaved.mock.calls[0][0] as string;
    expect(renamed).not.toBe(path);
    expect(renamed).toContain('Courses du samedi');
    expect(mockLibrary().notes.get(renamed)?.note.title).toBe('Courses du samedi');
  });

  it('asks before leaving with unsaved changes', async () => {
    const { target, button, drag, onclose } = await render('Liste de courses');
    button('Bibliothèque').click();
    expect(onclose).toHaveBeenCalledTimes(1);

    await drag(line(5000, 15000, 15000));
    button('Bibliothèque').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeTruthy();
    button('Rester').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(onclose).toHaveBeenCalledTimes(1);

    button('Bibliothèque').click();
    await tick();
    button('Revenir sans enregistrer').click();
    expect(onclose).toHaveBeenCalledTimes(2);
  });

  it('Ctrl + W and Ctrl + F4 leave by the rules of the back button, and answer « Rester » to its question', async () => {
    const { target, drag, key, onclose } = await render('Liste de courses');
    await key('w', { ctrlKey: true });
    expect(onclose).toHaveBeenCalledTimes(1);
    await key('F4', { ctrlKey: true });
    expect(onclose).toHaveBeenCalledTimes(2);
    // W alone is not a close.
    await key('w');
    expect(onclose).toHaveBeenCalledTimes(2);

    await drag(line(5000, 15000, 15000));
    await key('w', { ctrlKey: true });
    expect(target.querySelector('[role="alertdialog"]')).toBeTruthy();
    expect(onclose).toHaveBeenCalledTimes(2);
    await key('F4', { ctrlKey: true });
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(onclose).toHaveBeenCalledTimes(2);
  });

  it('Ctrl + W works from the title field too, where W is a letter only without Ctrl', async () => {
    const { target, onclose } = await render('Liste de courses');
    const title = target.querySelector<HTMLInputElement>('input[type="text"]')!;
    title.focus();
    title.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', bubbles: true, cancelable: true }));
    await tick();
    expect(onclose).not.toHaveBeenCalled();
    title.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true, cancelable: true }));
    await tick();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('keeps the focus inside the confirmation, and gives it back to the button when answered « Rester »', async () => {
    const { target, button, drag } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    const back = button('Bibliothèque');
    back.focus();
    back.click();
    await tick();
    const dialog = target.ownerDocument.querySelector<HTMLElement>('[role="alertdialog"]')!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    // Tab from the last answer wraps to the first one: it never leaves the dialog.
    const buttons = [...dialog.querySelectorAll('button')];
    buttons[buttons.length - 1].focus();
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(buttons[0]);
    // Escape answers « Rester » from anywhere.
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await tick();
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(document.activeElement).toBe(back);
  });

  it('counts a title typed but not committed as an unsaved change', async () => {
    const { target, button, typeTitle, onclose } = await render('Liste de courses');
    typeTitle('Courses du samedi');
    // The assistant is told too.
    expect(bridgeScreens.editor?.blocker()).toContain('unsaved changes');
    button('Bibliothèque').click();
    await tick();
    expect(target.ownerDocument.querySelector('[role="alertdialog"]')).toBeTruthy();
    expect(onclose).not.toHaveBeenCalled();
  });

  it('does not leave when the rename that the title field started fails, and says why', async () => {
    vi.spyOn(api, 'retitleNote').mockRejectedValue({ code: 'invalidName', message: 'a:b' });
    const { target, button, typeTitle, blurTitle, onclose } = await render('Liste de courses');
    typeTitle('a:b');
    blurTitle();
    button('Bibliothèque').click();
    await flush();
    await flush();
    expect(onclose).not.toHaveBeenCalled();
    expect(target.textContent).toContain("Ce nom n'est pas utilisable".toLowerCase());
  });

  it('tells the assistant why the editor cannot be left while a dialog or the split panel is open', async () => {
    const { button, target } = await render('Liste de courses');
    expect(bridgeScreens.editor?.blocker()).toBeNull();
    button('Exporter…').click();
    await tick();
    expect(bridgeScreens.editor?.blocker()).toContain('dialog');
    // Closed (Escape from anywhere), the editor can be left again.
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(bridgeScreens.editor?.blocker()).toBeNull();
  });

  it('says so when the background image of the note cannot be loaded', async () => {
    const path = demoPath('Liste de courses');
    const stored = mockLibrary().notes.get(path)!;
    stored.note.backgroundImage = { name: 'fiche.png', rect: { x: 0, y: 0, width: 100, height: 100 }, quarterTurns: 0, opacity: 1 } as never;
    vi.spyOn(api, 'stageNoteBackground').mockRejectedValue({ code: 'backgroundMissing', message: '' });
    const { target } = await render('Liste de courses');
    await vi.waitFor(() => expect(target.textContent).toContain("L'image de fond de cette note n'a pas pu être chargée"));
  });

  it('asks before another note replaces one with unsaved changes', async () => {
    const first = demoPath('Liste de courses');
    const second = demoPath('Réunion parents');
    const target = document.createElement('div');
    document.body.append(target);
    const onclose = vi.fn();
    const onsaved = vi.fn();
    const component = mount(EditorHarness, { target, props: { initial: first, app, onclose, onsaved } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
    const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
    // An edit: a stroke on the page.
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    const fire = (type: string, x: number, y: number) => {
      const e = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x * scale + vx, clientY: y * scale + vy, button: 0, buttons: 1 });
      Object.defineProperty(e, 'pointerId', { value: 1 });
      Object.defineProperty(e, 'pressure', { value: 0.5 });
      svg.dispatchEvent(e);
    };
    fire('pointerdown', 5000, 15000);
    fire('pointermove', 9000, 15000);
    fire('pointerup', 12000, 15000);
    await tick();
    expect(target.querySelector('.editor')?.getAttribute('data-dirty')).toBe('yes');

    component.go(second);
    await tick();
    // Not replaced: the edits are asked about first.
    expect(document.querySelector('[role="alertdialog"]')).toBeTruthy();
    expect(target.querySelector('.editor')?.getAttribute('data-path')).toBe(first);
    document.querySelector<HTMLButtonElement>('[role="alertdialog"] button:last-child')!.click();
    await tick();
    expect(target.querySelector('.editor')?.getAttribute('data-path')).toBe(first);
    expect(onsaved).toHaveBeenCalledWith(first);

    component.go(second);
    await tick();
    const discard = [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find((b) => b.textContent?.trim() === 'Ouvrir sans enregistrer')!;
    discard.click();
    await vi.waitFor(() => expect(target.querySelector('.editor')?.getAttribute('data-path')).toBe(second));
  });

  it('saves and leaves from the confirmation', async () => {
    const { path, button, drag, onclose, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Bibliothèque').click();
    await tick();
    button('Enregistrer et revenir').click();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(onsaved).toHaveBeenCalledWith(path);
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
  });

  it('stays responsive with a big note (300 strokes, 20 000 samples)', async () => {
    const library = mockLibrary();
    const big = { ...bigNote(), title: 'Grosse note', layers: [{ name: 'Calque 1', visible: true }] };
    library.notes.set(`${library.root}\\2026-10-24 Grosse note.ardoise`, { note: big, stamp: freshStamp(), binder: '' });
    let start = performance.now();
    const { paths, button, drag } = await render('Grosse note');
    const opened = performance.now() - start;
    expect(paths()).toHaveLength(300);

    // A recolour redraws every stroke (new stroke objects); a selection change redraws none.
    button('Rouge').click();
    start = performance.now();
    button('Couleur').click();
    await tick();
    const recolour = performance.now() - start;

    button('Gomme').click();
    await tick();
    start = performance.now();
    await drag(line(1000, 29000, 5000, 60));
    const erase = performance.now() - start;
    expect(paths().length).toBeLessThan(300);

    button('Stylo').click();
    await tick();
    start = performance.now();
    await drag(line(1000, 29000, 20000, 200));
    const pen = performance.now() - start;
    // Bounds catch accidental quadratic work (jsdom is far slower than WebView2).
    expect(opened).toBeLessThan(8000);
    expect(recolour).toBeLessThan(3000);
    expect(erase).toBeLessThan(3000);
    expect(pen).toBeLessThan(3000);
  });

  it('saves after a partial erase: the cut strokes have whole-millisecond times', async () => {
    const { path, target, drag, button, onsaved } = await render('Liste de courses');
    button('Gomme').click();
    await tick();
    const partial = [...target.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find((r) =>
      r.parentElement?.textContent?.includes('Partielle'),
    )!;
    partial.click();
    await tick();
    await drag([
      [7500, 3000],
      [7500, 5000],
    ]);
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledWith(path));
    expect(target.querySelector('[role="alert"]')).toBeNull();
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes.length).toBeGreaterThan(1);
    const times = stored.strokes.flatMap((s) => s.points.map((p) => p.t));
    expect(times.every((t) => Number.isInteger(t) && t >= 0)).toBe(true);
  });

  it('holds back closing the window with unsaved changes, and asks', async () => {
    const win = fakeWindow();
    const { path, target, button, drag, onclose } = await render('Liste de courses', { windowClose: win.closer });
    // Nothing to save: the window closes.
    expect(win.request()).toBe(false);

    await drag(line(5000, 15000, 15000));
    expect(win.request()).toBe(true);
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeTruthy();
    button('Rester').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(win.destroy).not.toHaveBeenCalled();

    expect(win.request()).toBe(true);
    await tick();
    button('Fermer sans enregistrer').click();
    await vi.waitFor(() => expect(win.destroy).toHaveBeenCalledTimes(1));
    expect(onclose).not.toHaveBeenCalled();
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(1);
  });

  it('saves, then closes the window, from the close confirmation', async () => {
    const win = fakeWindow();
    const { path, button, drag, onsaved } = await render('Liste de courses', { windowClose: win.closer });
    await drag(line(5000, 15000, 15000));
    expect(win.request()).toBe(true);
    await tick();
    button('Enregistrer et fermer').click();
    await vi.waitFor(() => expect(win.destroy).toHaveBeenCalledTimes(1));
    expect(onsaved).toHaveBeenCalledWith(path);
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    // The close request listener goes away with the editor.
    expect(win.unlisten).not.toHaveBeenCalled();
    cleanup?.();
    cleanup = null;
    expect(win.unlisten).toHaveBeenCalledTimes(1);
  });

  it('asks the browser to confirm leaving the page with unsaved changes', async () => {
    const { drag } = await render('Liste de courses');
    const unload = () => {
      const event = new Event('beforeunload', { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(unload()).toBe(false);
    await drag(line(5000, 15000, 15000));
    expect(unload()).toBe(true);
    cleanup?.();
    cleanup = null;
    expect(unload()).toBe(false);
  });

  it('saves the edits as a copy in the same binder after a conflict', async () => {
    const { path, target, drag, key, button, status, onsaved } = await render('Cours de physique');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copyPath = onsaved.mock.calls[0][0] as string;
    expect(copyPath).not.toBe(path);
    const copy = mockLibrary().notes.get(copyPath)!;
    expect(copy.binder).toBe('Classe/3e B');
    expect(copy.note.title).toBe('Cours de physique (copie)');
    expect(copy.note.strokes).toHaveLength(3);
    expect(copy.note.id).not.toBe(mockLibrary().notes.get(path)!.note.id);
    // The original is untouched; the editor now edits the copy.
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    expect(target.querySelector('[role="alert"]')).toBeNull();
    expect(status()).toBe('Enregistrée');
    expect(target.querySelector('.editor')?.getAttribute('data-path')).toBe(copyPath);
    expect(target.querySelector<HTMLInputElement>('.title-field input')?.value).toBe('Cours de physique (copie)');
    await drag(line(5000, 15000, 18000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    expect(onsaved).toHaveBeenLastCalledWith(copyPath);
    expect(mockLibrary().notes.get(copyPath)!.note.strokes).toHaveLength(4);
  });

  it('offers to save as a copy when the note was deleted meanwhile', async () => {
    const { path, target, drag, button, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.delete(path);
    button('Enregistrer').click();
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')?.textContent).toContain("n'existe plus"));
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copy = mockLibrary().notes.get(onsaved.mock.calls[0][0] as string)!;
    expect(copy.binder).toBe('');
    expect(copy.note.strokes).toHaveLength(2);
  });

  it('notices when the open note changes on disk, and reloads it', async () => {
    const { path, target, paths, button, drag, onsaved } = await render('Liste de courses');
    // Its own saves do not count.
    await drag(line(5000, 15000, 15000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    await flush();
    await flush();
    expect(target.querySelector('[data-disk]')).toBeNull();

    const stored = mockLibrary().notes.get(path)!;
    stored.note = { ...stored.note, strokes: [] };
    stored.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('Rechargez-la');
    button('Recharger').click();
    await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
    await vi.waitFor(() => expect(paths()).toHaveLength(0));
    expect(target.querySelector('[data-disk]')).toBeNull();
  });

  it('warns that saving would conflict when the note changes on disk with unsaved edits', async () => {
    const { path, target, button, drag, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('serait refusé');
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copyPath = onsaved.mock.calls[0][0] as string;
    expect(mockLibrary().notes.get(copyPath)!.note.strokes).toHaveLength(2);
    expect(target.querySelector('[data-disk]')).toBeNull();
  });

  it('notices when the open note disappears from the library', async () => {
    const { path, target } = await render('Liste de courses');
    mockLibrary().notes.delete(path);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="missing"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('Enregistrer comme copie');
  });

  /** The note disappeared and the creation of the copy waits for `open()`. */
  async function copyWaiting(title: string) {
    const rendered = await render(title);
    mockLibrary().notes.delete(rendered.path);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(rendered.target.querySelector('[data-disk="missing"]')).toBeTruthy());
    let open!: () => void;
    const gate = new Promise<void>((resolve) => (open = resolve));
    const createNote = api.createNote;
    vi.spyOn(api, 'createNote').mockImplementation(async (binder, name) => {
      await gate;
      return createNote(binder, name);
    });
    return { ...rendered, open };
  }

  it('goes back to the library only once a copy being saved is written', async () => {
    const { button, onclose, onsaved, open } = await copyWaiting('Liste de courses');
    try {
      button('Enregistrer comme copie').click();
      button('Bibliothèque').click();
      button('Bibliothèque').click();
      await flush();
      expect(onclose).not.toHaveBeenCalled();
      open();
      await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
      // The copy was written first, and announced.
      expect(onsaved).toHaveBeenCalledTimes(1);
      expect(mockLibrary().notes.get(onsaved.mock.calls[0][0] as string)?.note.strokes).toHaveLength(1);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it('releases the background images only after the writes still queued when it goes away', async () => {
    const { button, open } = await copyWaiting('Liste de courses');
    try {
      const save = vi.spyOn(api, 'saveNote');
      const release = vi.spyOn(api, 'releaseBackgroundImages');
      button('Enregistrer comme copie').click();
      cleanup?.();
      cleanup = null;
      await flush();
      expect(release).not.toHaveBeenCalled();
      open();
      await vi.waitFor(() => expect(release).toHaveBeenCalledTimes(1));
      expect(save).toHaveBeenCalledTimes(1);
      expect(save.mock.invocationCallOrder[0]).toBeLessThan(release.mock.invocationCallOrder[0]);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it('leaves no blank copy behind when the copy cannot be written', async () => {
    const { target, button } = await render('Cours de physique');
    const before = new Set(mockLibrary().notes.keys());
    const path = [...before].find((p) => p.includes('Cours de physique'))!;
    mockLibrary().notes.delete(path);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="missing"]')).toBeTruthy());
    vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'backgroundMissing', message: 'mock-1' });
    try {
      button('Enregistrer comme copie').click();
      await vi.waitFor(() => expect(target.querySelector('[role="alert"]')?.textContent).toContain("l'image de fond n'est plus en mémoire"));
      // Nothing new in the library: the blank note made for the copy went to the trash.
      const added = [...mockLibrary().notes.keys()].filter((p) => !before.has(p));
      expect(added).toEqual([]);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it('saves after a pending rename, with the new path and stamp', async () => {
    const { path, target, drag, button, typeTitle, blurTitle, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    typeTitle('Courses du samedi');
    blurTitle();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    const renamed = onsaved.mock.calls[0][0] as string;
    expect(renamed).not.toBe(path);
    expect(onsaved.mock.calls[1][0]).toBe(renamed);
    expect(target.querySelector('[role="alert"]')).toBeNull();
    const stored = mockLibrary().notes.get(renamed)!.note;
    expect(stored.title).toBe('Courses du samedi');
    expect(stored.strokes).toHaveLength(2);
  });

  it('commits a typed title before saving with Ctrl+S from the title field', async () => {
    const { drag, typeTitle, titleInput, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    typeTitle('Courses du dimanche');
    titleInput().dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    const stored = mockLibrary().notes.get(onsaved.mock.calls[1][0] as string)!.note;
    expect(stored.title).toBe('Courses du dimanche');
    expect(stored.strokes).toHaveLength(2);
  });

  it('goes back once a rename under way is done, after announcing the new path', async () => {
    const { button, typeTitle, blurTitle, onclose, onsaved } = await render('Liste de courses');
    typeTitle('Courses du samedi');
    blurTitle();
    button('Bibliothèque').click();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
    // The rename itself went through, and the library heard of it first.
    expect([...mockLibrary().notes.keys()].some((p) => p.includes('Courses du samedi'))).toBe(true);
    expect(onsaved).toHaveBeenCalledTimes(1);
    expect(onsaved.mock.calls[0][0]).toContain('Courses du samedi');
    expect(onsaved.mock.invocationCallOrder[0]).toBeLessThan(onclose.mock.invocationCallOrder[0]);
    await flush();
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('calls no one back once destroyed: a save finishing after unmount', async () => {
    const { path, drag, button, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Enregistrer').click();
    cleanup?.();
    cleanup = null;
    await flush();
    await flush();
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    expect(onsaved).not.toHaveBeenCalled();
  });

  it('keeps crops from deleting strokes on hidden layers', async () => {
    const { path, button, drag, onsaved } = await render('Liste de courses');
    button('+ Ajouter un calque').click();
    await tick();
    await drag(line(20000, 25000, 18000));
    button('Masquer « Calque 2 »').click();
    await tick();
    button('Recadrer autour du contenu').click();
    await tick();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.page.width).toBeLessThan(29700);
    expect(stored.strokes.map((s) => s.layer)).toEqual([0, 1]);
  });

  it('never draws on a hidden layer: it stays chosen, the zone warns and the pen says why', async () => {
    const { target, paths, button, drag } = await render('Cours de physique');
    const chosen = () => target.querySelector<HTMLElement>('li.layer[data-chosen="true"]')?.dataset.layer;
    const warning = () => target.querySelector<HTMLElement>('[data-slot="layer-warning"]');
    button('+ Ajouter un calque').click();
    await tick();
    expect(chosen()).toBe('1');
    // Hiding the chosen layer does not move the choice.
    button('Masquer « Calque 2 »').click();
    await tick();
    expect(chosen()).toBe('1');
    expect(warning()?.dataset.kind).toBe('chosenHidden');
    expect(warning()?.textContent).toContain('Le calque choisi, « Calque 2 », est masqué');
    // The pen draws nothing and says why.
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(2);
    expect(target.textContent).toContain('affichez-le ou choisissez-en un autre pour dessiner');
    // Choosing the visible layer below gives the pen back, and the warning goes.
    button('Agir sur « Calque 1 »').click();
    await tick();
    expect(warning()).toBeNull();
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(3);
    // Showing the hidden layer gives the same state back.
    button('Afficher « Calque 2 »').click();
    await tick();
    expect(paths()).toHaveLength(3);
    expect(chosen()).toBe('0');
  });

  it('shows the layers zone in place of the layers panel', async () => {
    const { target } = await render('Cours de physique');
    const zone = target.querySelector<HTMLElement>('.editor-side section.layers-zone')!;
    expect(zone.dataset.where).toBe('editor');
    expect(zone.querySelector('input[type="radio"], input[type="checkbox"]')).toBeNull();
    expect(zone.querySelector('[data-action="tablet-layer"]')).toBeNull();
    expect(zone.querySelector('[data-slot="layers-help"]')?.textContent).toContain('calque choisi (encadré)');
    expect(target.querySelectorAll('#editor-layers-heading')).toHaveLength(0);
  });

  it('chooses a layer with a click on its row, takes the selection off it, and Ctrl+A takes that layer only', async () => {
    const { target, button, drag, key } = await render('Cours de physique');
    const row = (n: number) => target.querySelector<HTMLElement>(`li.layer[data-layer="${n}"]`)!;
    const selected = () => target.textContent?.match(/Traits sélectionnés : (\d+)/)?.[1] ?? null;
    button('+ Ajouter un calque').click();
    await tick();
    await drag(line(5000, 15000, 15000));
    button('Sélection').click();
    await tick();
    // The new layer holds one stroke, the first holds the note's two.
    await key('a', { ctrlKey: true });
    expect(selected()).toBe('1');
    row(0).click();
    await tick();
    expect(row(0).dataset.chosen).toBe('true');
    expect(row(1).dataset.chosen).toBeUndefined();
    expect(selected()).toBeNull();
    expect(target.querySelector('rect.selection')).toBeNull();
    await key('a', { ctrlKey: true });
    expect(selected()).toBe('2');
  });

  it('lets Space press a button, and pans elsewhere', async () => {
    const { svg, button } = await render('Cours de physique');
    const box = button('Masquer « Calque 1 »');
    box.focus();
    const onBox = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    box.dispatchEvent(onBox);
    await tick();
    expect(onBox.defaultPrevented).toBe(false);
    expect(svg.classList.contains('space')).toBe(false);
    box.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
    const elsewhere = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    window.dispatchEvent(elsewhere);
    await tick();
    expect(elsewhere.defaultPrevented).toBe(true);
    expect(svg.classList.contains('space')).toBe(true);
    window.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
  });

  it('recovers when the canvas loses the pointer capture mid-stroke', async () => {
    const { path, target, pointer, drag, button, onsaved } = await render('Liste de courses');
    pointer('pointerdown', 5000, 15000);
    pointer('pointermove', 8000, 15000);
    pointer('lostpointercapture', 8000, 15000);
    await tick();
    expect(target.querySelector('path.live')).toBeNull();
    await drag(line(5000, 15000, 18000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const strokes = mockLibrary().notes.get(path)!.note.strokes;
    expect(strokes).toHaveLength(2);
    expect(strokes[1].points[0].y).toBeCloseTo(18000, -1);
  });

  it('ignores undo while erasing, so that the undo history stays consistent', async () => {
    const { paths, pointer, drag, key, button } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(3);
    button('Gomme').click();
    await tick();
    pointer('pointerdown', 4000, 15000);
    pointer('pointermove', 10000, 15000);
    await key('z', { ctrlKey: true });
    pointer('pointermove', 16000, 15000);
    pointer('pointerup', 16000, 15000);
    await tick();
    expect(paths()).toHaveLength(2);
    // Undo restores the erased stroke, then removes the drawn strokes in reverse order.
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(3);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(2);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(1);
  });

  it('reports a note that cannot be opened', async () => {
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(Editor, { target, props: { path: 'C:\\demo\\Ardoise\\nope.ardoise', app, onclose: () => {} } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    expect(target.textContent).toContain("Impossible d'ouvrir cette note");
  });
});

describe('note editor: split, copy image and export', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    mockClipboard.available = true;
  });

  const panelOf = (target: HTMLElement) => target.querySelector<HTMLElement>('.split-panel');
  const slider = (target: HTMLElement) => target.querySelector<HTMLInputElement>('.split-panel input[type="range"]')!;
  const titleField = (target: HTMLElement) => target.querySelector<HTMLInputElement>('.split-panel input[type="text"]')!;
  const moved = (target: HTMLElement) => target.querySelectorAll('g.ink path.split-moved');
  const kept = (target: HTMLElement) => target.querySelectorAll('g.ink path.split-kept');
  const notice = (target: HTMLElement) => target.querySelector('.editor-notice');
  const panelButton = (target: HTMLElement, label: string) => {
    const found = [...target.querySelectorAll<HTMLButtonElement>('.split-panel button')].find((b) => b.textContent?.trim() === label);
    if (!found) throw new Error(`no panel button "${label}"`);
    return found;
  };
  const setAt = async (target: HTMLElement, value: number) => {
    const input = slider(target);
    input.value = String(value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const chooseZone = async (target: HTMLElement) => {
    [...target.querySelectorAll<HTMLLabelElement>('.split-panel label.choice')]
      .find((l) => /Selon une zone|By zone/.test(l.textContent ?? ''))!
      .querySelector('input')!
      .click();
    await tick();
  };
  /** The mock library's notes other than the three demo ones. */
  const created = () => [...mockLibrary().notes.values()].filter((s) => !s.note.id.startsWith('b1f0c6c2'));

  it('splits at an instant: live highlight, new note, kept part shown saved with a fresh history', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { path, target, paths, button, drag, status, onsaved } = await render('Cours de physique');
    // An unsaved stroke written after the two demo ones (their samples end at 295 ms).
    await drag(line(5000, 15000, 15000));
    expect(status()).toBe('Modifications non enregistrées');
    expect(panelOf(target)).toBeNull();

    button('Découper').click();
    await tick();
    expect(panelOf(target)).toBeTruthy();
    expect(slider(target).min).toBe('0');
    expect(Number(slider(target).max)).toBeGreaterThanOrEqual(795);
    expect(panelOf(target)!.textContent).toContain('enregistrées avec la découpe');

    // At 150 ms the two demo strokes are cut in two and the drawn stroke leaves entirely.
    await setAt(target, 150);
    expect(target.querySelector('.split-time')?.textContent).toContain('0:00.1');
    expect(moved(target)).toHaveLength(3);
    expect(kept(target)).toHaveLength(2);
    expect(target.querySelector('.split-counts')?.textContent).toBe('Traits qui restent ici : 2 · traits qui partent : 3');
    expect(titleField(target).value).toBe('Cours de physique (suite)');

    // Moving the slider moves the highlight: past the demo strokes only the drawn one leaves.
    await setAt(target, 400);
    expect(moved(target)).toHaveLength(1);
    expect(kept(target)).toHaveLength(2);
    await setAt(target, 150);

    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)).toBeNull());
    expect(splitSpy).toHaveBeenCalledTimes(1);
    expect(splitSpy).toHaveBeenCalledWith(
      path,
      expect.objectContaining({ title: 'Cours de physique' }),
      expect.anything(),
      { kind: 'time', at: 150 },
      'Cours de physique (suite)',
      // No background image: no staged pixels.
      undefined,
    );
    // The editor shows what is kept, saved, with nothing left to undo.
    expect(paths()).toHaveLength(2);
    expect(moved(target)).toHaveLength(0);
    expect(status()).toBe('Enregistrée');
    expect(button('Annuler (Ctrl+Z)').disabled).toBe(true);
    expect(notice(target)?.textContent).toBe('Nouvelle note créée : Cours de physique (suite)');
    expect(onsaved).toHaveBeenCalledWith(path);

    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes.map((s) => s.points.length)).toEqual([30, 30]);
    const [other] = created();
    expect(other.note.title).toBe('Cours de physique (suite)');
    expect(other.note.strokes).toHaveLength(3);
    expect(other.binder).toBe('Classe/3e B');
  });

  it('proposes English titles in English', async () => {
    setLocale('en');
    const { target, button } = await render('Cours de physique');
    button('Split').click();
    await tick();
    expect(titleField(target).value).toBe('Cours de physique (continued)');
    await chooseZone(target);
    expect(titleField(target).value).toBe('Cours de physique (excerpt)');
    expect(panelOf(target)!.textContent).toContain('Trace the zone to cut out');
  });

  it('keeps a title the user typed when the mode changes', async () => {
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    const field = titleField(target);
    field.value = 'Ma suite';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await chooseZone(target);
    expect(titleField(target).value).toBe('Ma suite');
  });

  it('splits by zone with the lasso rule', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { path, target, paths, button, drag } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await chooseZone(target);
    expect(slider(target)).toBeNull();
    expect(titleField(target).value).toBe('Cours de physique (extrait)');
    // Nothing traced yet: nothing highlighted, the confirm button waits.
    expect(moved(target)).toHaveLength(0);
    expect(kept(target)).toHaveLength(0);
    expect(panelButton(target, 'Découper').disabled).toBe(true);

    // The zone encloses the first demo stroke (a wave around y = 3000) only.
    await drag([
      [1500, 2000],
      [23000, 2000],
      [23000, 4500],
      [1500, 4500],
      [1500, 2000],
    ]);
    expect(target.querySelector('polygon.split-zone')).toBeTruthy();
    expect(moved(target)).toHaveLength(1);
    expect(moved(target)[0].getAttribute('data-stroke')).toBe('0');
    expect(kept(target)).toHaveLength(1);

    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)).toBeNull());
    const spec = splitSpy.mock.calls[0][3];
    expect(spec.kind).toBe('zone');
    expect(spec.kind === 'zone' && spec.polygon).toHaveLength(5);
    expect(splitSpy.mock.calls[0][4]).toBe('Cours de physique (extrait)');
    expect(paths()).toHaveLength(1);
    expect(mockLibrary().notes.get(path)!.note.strokes.map((s) => s.tool)).toEqual(['fountainPen']);
    expect(created()[0].note.strokes.map((s) => s.tool)).toEqual(['ballpoint']);
  });

  it('keeps the page read-only while the panel is open', async () => {
    const { target, paths, button, drag } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    const before = paths().length;
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(before);
    expect(target.querySelector('g.ink path.live')).toBeNull();
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(button('Enregistrer').disabled).toBe(true);
  });

  it('refuses a cut that leaves one side empty and explains why', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await setAt(target, 0);
    expect(panelButton(target, 'Découper').disabled).toBe(true);
    expect(panelOf(target)!.textContent).toContain('Rien à découper ainsi');
    await setAt(target, 100);
    expect(panelButton(target, 'Découper').disabled).toBe(false);
    expect(panelOf(target)!.textContent).not.toContain('Rien à découper ainsi');
    expect(splitSpy).not.toHaveBeenCalled();
  });

  it('cancels with Escape or the cancel button, leaving everything untouched', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { target, paths, button, key, status } = await render('Cours de physique');
    const before = mockLibrary().notes.size;
    button('Découper').click();
    await tick();
    await setAt(target, 150);
    expect(moved(target).length).toBeGreaterThan(0);
    await key('Escape');
    expect(panelOf(target)).toBeNull();
    expect(moved(target)).toHaveLength(0);
    expect(kept(target)).toHaveLength(0);

    // Ctrl + W closes the split, not the editor.
    button('Découper').click();
    await tick();
    expect(panelOf(target)).not.toBeNull();
    await key('w', { ctrlKey: true });
    expect(panelOf(target)).toBeNull();
    expect(target.querySelector('.editor')).not.toBeNull();

    button('Découper').click();
    await tick();
    panelButton(target, 'Annuler').click();
    await tick();
    expect(panelOf(target)).toBeNull();
    expect(paths()).toHaveLength(2);
    expect(status()).toBe('Enregistrée');
    expect(splitSpy).not.toHaveBeenCalled();
    expect(mockLibrary().notes.size).toBe(before);
  });

  it('closes the panel from the toolbar button too', async () => {
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    expect(button('Découper').getAttribute('aria-pressed')).toBe('true');
    button('Découper').click();
    await tick();
    expect(panelOf(target)).toBeNull();
  });

  it('ignores the editing keys while the panel is open', async () => {
    const { target, paths, button, drag, key } = await render('Cours de physique');
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    button('Découper').click();
    await tick();
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(3);
    expect(panelOf(target)).toBeTruthy();
  });

  it('keeps the note and says why when the file changed meanwhile', async () => {
    const { path, target, paths, button, status } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await setAt(target, 150);
    // Someone else saved the note: its stamp is no longer the one the editor knows.
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    const before = mockLibrary().notes.size;
    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)!.querySelector('[role="alert"]')).toBeTruthy());
    expect(panelOf(target)!.querySelector('[role="alert"]')!.textContent).toContain(
      'Impossible de découper la note : la note a été modifiée entre-temps',
    );
    expect(panelButton(target, 'Découper').disabled).toBe(false);
    // Still showing the preview of the same note, which was not touched.
    expect(moved(target)).toHaveLength(2);
    expect(paths()).toHaveLength(4);
    expect(status()).toBe('Enregistrée');
    expect(mockLibrary().notes.size).toBe(before);
    expect(mockLibrary().notes.get(path)!.note.strokes.map((s) => s.points.length)).toEqual([60, 60]);
  });

  it('shows the refusal of the desktop app when nothing can be split', async () => {
    vi.spyOn(api, 'splitNote').mockRejectedValueOnce({ code: 'nothingToSplit', message: 'x' });
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)!.querySelector('[role="alert"]')).toBeTruthy());
    expect(panelOf(target)!.querySelector('[role="alert"]')!.textContent).toContain('cette découpe ne laisserait rien');
  });

  it('says so when the note is empty', async () => {
    const { target, button, key } = await render('Cours de physique');
    button('Sélection').click();
    await tick();
    await key('a', { ctrlKey: true });
    await key('Delete');
    button('Découper').click();
    await tick();
    expect(panelOf(target)!.textContent).toContain('Cette note est vide');
    expect(slider(target)).toBeNull();
  });

  it('locks the page panel, the layers panel and the style controls while the split panel is open', async () => {
    const { target, button } = await render('Cours de physique');
    const section = (selector: string) => target.querySelector<HTMLElement>(selector)!;
    const controls = (root: ParentNode) => [...root.querySelectorAll<HTMLElement>('button, input, select')];
    // The page's blocks (paper, format, turn and crop) and the background image, which is in the Fond tab.
    const pageControls = () =>
      ['[data-section="paper"]', '[data-section="page-format"]', '[data-section="turn-crop"]', '[data-section="background-image"]'].flatMap(
        (selector) => controls(section(selector)),
      );
    const toolbarControls = () =>
      controls(target.querySelector('.toolbar')!).filter((c) => !c.closest('.share'));
    const locked = (items: HTMLElement[]) =>
      items.every((c) => (c as HTMLButtonElement | HTMLInputElement | HTMLSelectElement).disabled);
    const free = (items: HTMLElement[]) =>
      items.every((c) => !(c as HTMLButtonElement | HTMLInputElement | HTMLSelectElement).disabled);

    // Before: everything works (the layer buttons that cannot apply to the top layer excepted).
    expect(toolbarControls().length).toBeGreaterThan(10);
    expect(free(toolbarControls())).toBe(true);
    expect(free(pageControls())).toBe(true);
    expect(controls(section('.layers-zone')).some((c) => (c as HTMLInputElement).disabled)).toBe(true);
    expect(button('+ Ajouter un calque').disabled).toBe(false);

    button('Découper').click();
    await tick();
    expect(panelOf(target)).toBeTruthy();
    // Mode, tool, colours, width, "apply to all": all locked.
    expect(toolbarControls().length).toBeGreaterThan(10);
    expect(locked(toolbarControls())).toBe(true);
    expect(locked(pageControls())).toBe(true);
    // Four paper swatches, the template, the texture, two orientations, paper size, two sides and Apply, the
    // scale option, two turns, two crops, and the background image's button.
    expect(pageControls().length).toBe(18);
    expect(locked(controls(section('.layers-zone')))).toBe(true);
    // The split's own buttons, copy and export stay available.
    expect(button('Découper').disabled).toBe(false);
    expect(button("Copier l'image").disabled).toBe(false);
    expect(button('Exporter…').disabled).toBe(false);

    // Closing the panel gives everything back.
    panelButton(target, 'Annuler').click();
    await tick();
    expect(panelOf(target)).toBeNull();
    expect(free(toolbarControls())).toBe(true);
    expect(free(pageControls())).toBe(true);
    expect(button('+ Ajouter un calque').disabled).toBe(false);
  });

  it('does not let a click on a locked control change the note while splitting', async () => {
    const { target, button, status } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    button("Tourner la page d'un quart de tour vers la droite").click();
    button('Recadrer autour du contenu').click();
    button('+ Ajouter un calque').click();
    await tick();
    expect(status()).toBe('Enregistrée');
    expect(target.querySelectorAll('[data-layer]')).toHaveLength(1);
  });

  it('copies the note as an image at 150 dpi, lower for a page that would exceed 16 384 px', async () => {
    // 3 m wide: 17 717 px at 150 dpi, 11 339 px at 96 dpi; 5 m: only 72 dpi fits.
    const wide = (width: number) => {
      mockLibrary().notes.get(demoPath('Liste de courses'))!.note.page = { width, height: 21600 };
    };
    const copySpy = vi.spyOn(api, 'copyNoteImage');
    wide(300_000);
    const first = await render('Liste de courses');
    first.button("Copier l'image").click();
    await vi.waitFor(() => expect(copySpy).toHaveBeenCalledTimes(1));
    expect(copySpy.mock.calls[0][1]).toBe(96);
    cleanup?.();
    cleanup = null;

    wide(500_000);
    const second = await render('Liste de courses');
    second.button("Copier l'image").click();
    await vi.waitFor(() => expect(copySpy).toHaveBeenCalledTimes(2));
    expect(copySpy.mock.calls[1][1]).toBe(72);
  });

  it('copies the note as an image at 150 dpi', async () => {
    const copySpy = vi.spyOn(api, 'copyNoteImage');
    const { path, target, button } = await render('Liste de courses');
    button("Copier l'image").click();
    await vi.waitFor(() => expect(notice(target)?.textContent).toContain('Image copiée'));
    expect(copySpy).toHaveBeenCalledWith({ path, note: expect.objectContaining({ title: 'Liste de courses' }) }, 150);
    expect(notice(target)?.getAttribute('role')).toBe('status');
  });

  it('says in plain words when the clipboard is unavailable', async () => {
    mockClipboard.available = false;
    const { target, button } = await render('Liste de courses');
    button("Copier l'image").click();
    await vi.waitFor(() => expect(notice(target)).toBeTruthy());
    expect(notice(target)?.getAttribute('role')).toBe('alert');
    expect(notice(target)?.textContent).toContain("Impossible de copier l'image : le presse-papiers n'est pas disponible");
  });

  it('exports the note as it is in the editor, unsaved changes included', async () => {
    const exportSpy = vi.spyOn(api, 'exportNotes');
    const { path, target, drag, button, key, paths } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Exporter…').click();
    await tick();
    const dialog = target.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog).toBeTruthy();
    expect(dialog.textContent).toContain('Note : Liste de courses');

    // The editing keys wait while the dialog is open.
    await key('a', { ctrlKey: true });
    await key('Delete');
    expect(paths()).toHaveLength(2);

    [...dialog.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Exporter…')!.click();
    await vi.waitFor(() => expect(exportSpy).toHaveBeenCalled());
    const [sources, format, options, destination] = exportSpy.mock.calls[0];
    expect(sources[0].path).toBe(path);
    expect(sources[0].note?.strokes).toHaveLength(2);
    expect(format).toBe('png');
    expect(options).toEqual({ dpi: 150, background: true, quality: 90, pdfPage: 'actual' });
    expect(destination).toMatch(/Liste de courses\.png$/);
    await vi.waitFor(() => expect(dialog.textContent).toContain('Fichiers créés : 1'));

    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(paths()).toHaveLength(2);
  });
});

// --- Replay (SPEC §4.5) and narration (docs/design/media.md) --------------------------------------

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

/** The demo note `title` gets strokes over [0, 1000], [3000, 4000] and [4200, 5000] ms: a 5 s replay. */
function makeSlow(title: string) {
  const entry = mockLibrary().notes.get(demoPath(title))!;
  entry.note = {
    ...entry.note,
    strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, 5000)],
  };
}

const action = <T extends HTMLElement = HTMLElement>(target: HTMLElement, name: string) =>
  target.querySelector<T>(`[data-action="${name}"]`)!;
const roleText = (target: HTMLElement, role: string) => target.querySelector(`[data-role="${role}"]`)?.textContent;

describe('note editor: replay', () => {
  beforeEach(() => {
    resetMockNarration();
    makeSlow('Liste de courses');
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  /** Opens the replay of the slow note, with the clock under the test's control. */
  async function replay() {
    const view = await render('Liste de courses');
    vi.useFakeTimers();
    view.button('Relecture').click();
    await tick();
    return view;
  }

  it('is switched on and off by the toolbar button and by R, and never changes the note', async () => {
    const { target, button, key, paths, drag, status } = await render('Liste de courses');
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(paths()).toHaveLength(3);
    expect(button('Relecture').getAttribute('aria-pressed')).toBe('false');

    button('Relecture').click();
    await tick();
    expect(target.querySelector('.replay-bar')).toBeTruthy();
    expect(button('Relecture').getAttribute('aria-pressed')).toBe('true');
    // The replay starts with the first sample.
    expect(paths()).toHaveLength(1);

    // Read-only: the tools wait, a drag moves the page and draws nothing, the editing keys do nothing.
    expect(button('Stylo').disabled).toBe(true);
    expect(button('Gomme').disabled).toBe(true);
    expect(button('Découper').disabled).toBe(true);
    expect(button('Annuler (Ctrl+Z)').disabled).toBe(true);
    await drag(line(5000, 15000, 15000));
    await key('a', { ctrlKey: true });
    await key('Delete');
    expect(paths()).toHaveLength(1);
    expect(status()).toBe('Enregistrée');

    await key('r');
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(paths()).toHaveLength(3);
    expect(button('Stylo').disabled).toBe(false);
    expect(status()).toBe('Enregistrée');
    // Editing works again.
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(4);
    expect(status()).toBe('Modifications non enregistrées');

    await key('r');
    expect(target.querySelector('.replay-bar')).toBeTruthy();
    // The unsaved stroke is part of the replay, which ends on the whole page.
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    seek.value = seek.max;
    seek.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(paths()).toHaveLength(4);
    await key('Escape');
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(status()).toBe('Modifications non enregistrées');
  });

  it('plays with the clock, stops at the end, and plays again from the start', async () => {
    const { target, paths } = await replay();
    const play = () => action<HTMLButtonElement>(target, 'replay-play');
    expect(play().getAttribute('aria-label')).toBe('Lecture');
    expect(roleText(target, 'replay-total')).toBe('0:05');
    expect(roleText(target, 'replay-current')).toBe('0:00');

    play().click();
    await tick();
    expect(play().getAttribute('aria-label')).toBe('Pause');
    expect(target.querySelector('.replay-bar')?.getAttribute('data-playing')).toBe('yes');
    await vi.advanceTimersByTimeAsync(2100);
    expect(roleText(target, 'replay-current')).toBe('0:02');
    expect(Number(action<HTMLInputElement>(target, 'replay-seek').value)).toBeGreaterThan(2000);
    // The first stroke is complete, the others have not begun.
    expect(paths()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1500);
    expect(roleText(target, 'replay-current')).toBe('0:03');
    expect(paths()).toHaveLength(2);

    await vi.advanceTimersByTimeAsync(3000);
    expect(roleText(target, 'replay-current')).toBe('0:05');
    expect(target.querySelector('.replay-bar')?.getAttribute('data-playing')).toBe('no');
    expect(play().getAttribute('aria-label')).toBe('Lecture');
    expect(paths()).toHaveLength(3);

    // Looping is off: playing again starts over.
    play().click();
    await tick();
    await vi.advanceTimersByTimeAsync(200);
    expect(roleText(target, 'replay-current')).toBe('0:00');
    expect(paths()).toHaveLength(1);
    // Pause holds the page where it is.
    play().click();
    await tick();
    const held = action<HTMLInputElement>(target, 'replay-seek').value;
    await vi.advanceTimersByTimeAsync(1000);
    expect(action<HTMLInputElement>(target, 'replay-seek').value).toBe(held);
  });

  it('plays and pauses with Space, also from the timeline', async () => {
    const { target, key } = await replay();
    const bar = () => target.querySelector('.replay-bar')?.getAttribute('data-playing');
    await key(' ');
    expect(bar()).toBe('yes');
    await key(' ');
    expect(bar()).toBe('no');
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    seek.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true }));
    await tick();
    expect(bar()).toBe('yes');
    // On a button, Space presses the button (and the editor leaves it alone).
    const play = action<HTMLButtonElement>(target, 'replay-play');
    play.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true }));
    await tick();
    expect(bar()).toBe('yes');
  });

  it('seeks by the timeline and by its keys', async () => {
    const { target, paths } = await replay();
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    expect(seek.max).toBe('5000');
    seek.value = '3500';
    seek.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(roleText(target, 'replay-current')).toBe('0:03');
    expect(seek.getAttribute('aria-valuetext')).toBe('0:03 sur 0:05');
    expect(seek.getAttribute('aria-label')).toBe('Position dans la relecture');
    expect(paths()).toHaveLength(2);

    const press = async (k: string) => {
      seek.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
      await tick();
    };
    await press('ArrowRight');
    expect(seek.value).toBe('4500');
    await press('ArrowLeft');
    await press('ArrowLeft');
    expect(seek.value).toBe('2500');
    expect(paths()).toHaveLength(1);
    await press('PageUp');
    expect(seek.value).toBe('5000');
    expect(paths()).toHaveLength(3);
    await press('PageDown');
    expect(seek.value).toBe('0');

    // Dragging the thumb pauses the replay and resumes it on release.
    const bar = () => target.querySelector('.replay-bar')?.getAttribute('data-playing');
    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    expect(bar()).toBe('yes');
    seek.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await tick();
    expect(bar()).toBe('no');
    seek.dispatchEvent(new Event('pointerup', { bubbles: true }));
    await tick();
    expect(bar()).toBe('yes');
  });

  it('changes the speed and trims the dead time, which change the length but not the page', async () => {
    const { target, paths } = await replay();
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    const speed = action<HTMLSelectElement>(target, 'replay-speed');
    const trim = action<HTMLInputElement>(target, 'replay-trim');
    expect([...speed.options].map((o) => o.value)).toEqual(['0.25', '0.5', '1', '2', '4', '8', '16']);
    expect([...speed.options].map((o) => o.textContent?.trim())).toEqual([
      '0,25×',
      '0,5×',
      '1×',
      '2×',
      '4×',
      '8×',
      '16×',
    ]);
    expect(speed.value).toBe('1');
    expect(trim.checked).toBe(false);

    const choose = async (value: string) => {
      speed.value = value;
      speed.dispatchEvent(new Event('change', { bubbles: true }));
      await tick();
    };
    await choose('4');
    expect(roleText(target, 'replay-total')).toBe('0:01');
    await choose('0.25');
    expect(roleText(target, 'replay-total')).toBe('0:20');
    await choose('1');

    // At 3.5 s the second stroke is half drawn; trimming moves the playhead, not the drawing.
    seek.value = '3500';
    seek.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    const before = paths().map((p) => p.getAttribute('d'));
    trim.click();
    await tick();
    expect(trim.checked).toBe(true);
    expect(roleText(target, 'replay-total')).toBe('0:03');
    expect(roleText(target, 'replay-current')).toBe('0:02');
    expect(paths().map((p) => p.getAttribute('d'))).toEqual(before);
    trim.click();
    await tick();
    expect(roleText(target, 'replay-total')).toBe('0:05');
    expect(roleText(target, 'replay-current')).toBe('0:03');

    // At 16x the whole replay takes a third of a second.
    await choose('16');
    expect(roleText(target, 'replay-total')).toBe('0:00');
    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    await vi.advanceTimersByTimeAsync(400);
    expect(target.querySelector('.replay-bar')?.getAttribute('data-playing')).toBe('no');
    expect(paths()).toHaveLength(3);
  });

  it('draws the stroke in progress sample by sample and keeps the finished strokes as they are', async () => {
    const { target, paths } = await replay();
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    const seekTo = async (ms: number) => {
      seek.value = String(ms);
      seek.dispatchEvent(new Event('input', { bubbles: true }));
      await tick();
    };
    await seekTo(5000);
    const whole = paths().map((p) => p.getAttribute('d'));
    expect(whole).toHaveLength(3);
    await seekTo(500);
    expect(paths()).toHaveLength(1);
    const half = paths()[0].getAttribute('d');
    expect(half).not.toBe(whole[0]);
    // A little later the stroke has grown.
    await seekTo(700);
    expect(paths()[0].getAttribute('d')).not.toBe(half);
    await seekTo(3500);
    expect(paths()).toHaveLength(2);
    // The first stroke is whole, the second still short of its final shape.
    expect(paths()[0].getAttribute('d')).toBe(whole[0]);
    expect(paths()[1].getAttribute('d')).not.toBe(whole[1]);
    await seekTo(4100);
    expect(paths()[1].getAttribute('d')).toBe(whole[1]);
    expect(target.textContent).not.toContain('Cette note ne contient aucun tracé');
  });

  it('opens the video export for the note as it is in the editor', async () => {
    const { path, target, button, key, drag, paths } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(4);
    expect(target.querySelector('.video-dialog-double')).toBeNull();
    button('Vidéo').click();
    await tick();
    const dialog = target.querySelector<HTMLElement>('.video-dialog-double')!;
    expect(dialog).toBeTruthy();
    expect(dialog.dataset.path).toBe(path);
    expect(dialog.dataset.subject).toBe('Liste de courses');
    expect(dialog.dataset.strokes).toBe('4');
    // The editing keys wait while it is open.
    await key('a', { ctrlKey: true });
    await key('Delete');
    expect(paths()).toHaveLength(4);
    [...dialog.querySelectorAll('button')].find((b) => b.textContent === 'close-video')!.click();
    await tick();
    expect(target.querySelector('.video-dialog-double')).toBeNull();
  });

  it('says so when there is nothing to replay', async () => {
    const entry = mockLibrary().notes.get(demoPath('Liste de courses'))!;
    entry.note = { ...entry.note, strokes: [] };
    const { target, button } = await render('Liste de courses');
    button('Relecture').click();
    await tick();
    expect(target.textContent).toContain('Cette note ne contient aucun tracé à rejouer.');
    expect(action<HTMLButtonElement>(target, 'replay-play').disabled).toBe(true);
    expect(action<HTMLInputElement>(target, 'replay-seek').disabled).toBe(true);
  });

  it('speaks English too', async () => {
    setLocale('en');
    const { target, button } = await render('Liste de courses');
    button('Replay').click();
    await tick();
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    expect(seek.getAttribute('aria-label')).toBe('Position in the replay');
    expect(seek.getAttribute('aria-valuetext')).toBe('0:00 of 0:05');
    expect(target.textContent).toContain('Remove dead time');
    expect(action<HTMLSelectElement>(target, 'replay-speed').options[0].textContent?.trim()).toBe('0.25×');
    expect(button('Video')).toBeTruthy();
  });
});

/** Stands in for the audio element's playback, which jsdom does not implement. */
function fakeMedia() {
  const paused = new WeakMap<object, boolean>();
  const plays = vi.fn();
  const pauses = vi.fn();
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(function (this: HTMLMediaElement) {
    paused.set(this, false);
    plays();
    return Promise.resolve();
  });
  vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(function (this: HTMLMediaElement) {
    paused.set(this, true);
    pauses();
  });
  vi.spyOn(HTMLMediaElement.prototype, 'paused', 'get').mockImplementation(function (this: HTMLMediaElement) {
    return paused.get(this) ?? true;
  });
  vi.spyOn(HTMLMediaElement.prototype, 'duration', 'get').mockReturnValue(3);
  const create = vi.fn((blob: Blob) => (blob ? 'blob:narration' : ''));
  const revoke = vi.fn();
  Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke });
  return { plays, pauses, create, revoke };
}

describe('note editor: narration', () => {
  beforeEach(() => {
    resetMockNarration();
    makeSlow('Liste de courses');
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    Reflect.deleteProperty(URL, 'createObjectURL');
    Reflect.deleteProperty(URL, 'revokeObjectURL');
  });

  async function replay(extra: { windowClose?: WindowClose; onchange?: (next: AppState) => void } = {}) {
    const view = await render('Liste de courses', extra);
    vi.useFakeTimers();
    view.button('Relecture').click();
    await tick();
    return view;
  }

  /** Opens the record panel and starts recording; the replay then plays. */
  async function startRecording(target: HTMLElement) {
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action(target, 'narration-start')).toBeTruthy());
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(300);
  }

  const pendingBlock = (target: HTMLElement) => target.querySelector<HTMLElement>('[data-slot="narration-pending"]');

  const barState = (target: HTMLElement) => target.querySelector('.replay-bar')?.getAttribute('data-playing');
  const storedText = (target: HTMLElement) => target.querySelector('[data-slot="narration-stored"]')?.textContent?.trim();

  it('plays the stored narration along with the replay, with the options it was recorded with', async () => {
    const media = fakeMedia();
    setMockNarration(demoPath('Liste de courses'), {
      durationMs: 3000,
      base: 'replay',
      speed: 2,
      maxPauseMs: 500,
      startMs: 0,
    });
    const { target } = await replay();
    await vi.waitFor(() => expect(action(target, 'replay-narration')).toBeTruthy());
    const bar = target.querySelector('.replay-bar')!;
    expect(bar.textContent).toContain('Lire avec la narration');
    expect(bar.textContent).toContain('(0:03)');
    expect(storedText(target)).toBe('Narration enregistrée dans la note (0:03).');
    const along = action<HTMLInputElement>(target, 'replay-narration');
    expect(along.checked).toBe(false);
    expect(target.querySelector('audio')).toBeNull();

    along.click();
    await vi.waitFor(() => expect(target.querySelector('audio')).toBeTruthy());
    const audio = target.querySelector('audio')!;
    expect(audio.getAttribute('src')).toBe('blob:narration');
    expect(media.create.mock.calls[0][0]).toBeInstanceOf(Blob);
    expect(media.create.mock.calls[0][0].type).toBe('audio/wav');
    // The replay now has the speed and the pauses of the recording, which the user cannot change.
    const speed = action<HTMLSelectElement>(target, 'replay-speed');
    const trim = action<HTMLInputElement>(target, 'replay-trim');
    expect(speed.value).toBe('2');
    expect(speed.disabled).toBe(true);
    expect(trim.checked).toBe(true);
    expect(trim.disabled).toBe(true);
    // 3500 ms of drawing at 2x is shorter than the 3 s of sound.
    expect(roleText(target, 'replay-total')).toBe('0:03');

    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    await vi.advanceTimersByTimeAsync(1000);
    expect(media.plays).toHaveBeenCalled();
    expect(audio.currentTime).toBeGreaterThan(0.7);
    expect(audio.currentTime).toBeLessThan(1.1);
    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    expect(media.pauses).toHaveBeenCalled();

    // Seeking moves the sound too.
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    seek.value = '1500';
    seek.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(audio.currentTime).toBe(1.5);

    along.click();
    await tick();
    expect(target.querySelector('audio')).toBeNull();
    expect(media.revoke).toHaveBeenCalledWith('blob:narration');
    expect(speed.disabled).toBe(false);
    expect(trim.disabled).toBe(false);
    expect(roleText(target, 'replay-total')).toBe('0:01');
  });

  it('records a narration while the replay plays, and the next save uses the new stamp', async () => {
    const setMicrophone = vi.spyOn(api, 'setMicrophone');
    const monitor = vi.spyOn(api, 'microphoneMonitor');
    const start = vi.spyOn(api, 'narrationStart');
    const stop = vi.spyOn(api, 'narrationStop');
    const saveNote = vi.spyOn(api, 'saveNote');
    const { path, target, button, drag, key, status, onclose } = await replay();
    const before = mockLibrary().notes.get(path)!.stamp;
    expect(target.textContent).toContain("Cette note n'a pas de narration.");

    // The panel listens to the microphone before recording.
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(monitor).toHaveBeenCalledWith(null));
    const select = target.querySelector<HTMLSelectElement>('select[data-field="microphone"]')!;
    await vi.waitFor(() => expect(select.options.length).toBe(3));
    select.value = 'mock-mic-2';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(setMicrophone).toHaveBeenCalledWith('mock-mic-2'));
    await vi.waitFor(() => expect(monitor).toHaveBeenLastCalledWith('mock-mic-2'));
    await vi.advanceTimersByTimeAsync(400);
    expect(target.querySelector('[role="meter"]')).toBeTruthy();

    // The replay options at that moment are the ones the narration is recorded with.
    const speed = action<HTMLSelectElement>(target, 'replay-speed');
    speed.value = '2';
    speed.dispatchEvent(new Event('change', { bubbles: true }));
    action<HTMLInputElement>(target, 'replay-trim').click();
    await tick();
    expect(target.querySelector('[data-section="narration"]')?.textContent).toContain(
      'vitesse 2×, temps morts supprimés',
    );

    const clickedAt = Date.now();
    action(target, 'narration-start').click();
    await vi.waitFor(() =>
      expect(start).toHaveBeenCalledWith('mock-mic-2', { kind: 'replay', path, speed: 2, maxPauseMs: 500, startMs: 0 }),
    );
    // The level capture was stopped before the recording took the microphone over.
    expect((await api.narrationStatus()).state).toBe('recording');
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(1200);
    expect(target.querySelector('.elapsed')?.textContent).toContain('Enregistrement : 0:01');
    await vi.waitFor(() =>
      expect(Number(target.querySelector('[role="meter"]')?.getAttribute('aria-valuenow'))).toBeGreaterThan(0),
    );

    // Nothing that would put the sound out of step with the replay works meanwhile.
    expect(action<HTMLButtonElement>(target, 'replay-play').disabled).toBe(true);
    expect(action<HTMLInputElement>(target, 'replay-seek').disabled).toBe(true);
    expect(speed.disabled).toBe(true);
    expect(action<HTMLInputElement>(target, 'replay-trim').disabled).toBe(true);
    expect(button('Relecture').disabled).toBe(true);
    await key(' ');
    expect(barState(target)).toBe('yes');
    // Leaving the editor would lose it.
    button('Bibliothèque').click();
    await tick();
    expect(onclose).not.toHaveBeenCalled();
    expect(target.querySelector('.editor-notice')?.getAttribute('role')).toBe('alert');
    expect(target.querySelector('select[data-field="microphone"]')).toBeTruthy();
    expect(target.querySelector<HTMLSelectElement>('select[data-field="microphone"]')!.disabled).toBe(true);

    action(target, 'narration-stop').click();
    // The note's current path and stamp, and when the replay was at instant 0 (Date.now() at its start).
    await vi.waitFor(() => expect(stop).toHaveBeenCalledWith(true, before, path, expect.any(Number)));
    const startedAt = stop.mock.calls.find(([save]) => save)![3] as number;
    expect(startedAt).toBeGreaterThanOrEqual(clickedAt);
    expect(startedAt).toBeLessThan(clickedAt + 1000);
    await vi.waitFor(() => expect(storedText(target)).toBe('Narration enregistrée dans la note (0:01).'));
    // The sound began just before the replay did: a tiny negative start in replay time.
    const startMs = (await api.narrationInfo(path))!.startMs;
    expect(startMs).toBeLessThanOrEqual(0);
    expect(startMs).toBeGreaterThan(-1000);
    const recorded = mockLibrary().notes.get(path)!.stamp;
    expect(recorded).not.toEqual(before);
    expect(target.querySelector('.editor-notice')?.textContent).toContain('Narration enregistrée (0:01)');
    // The replay stopped and the panel went back to its start.
    expect(barState(target)).toBe('no');
    expect(action(target, 'narration-open')).toBeTruthy();
    expect(action<HTMLButtonElement>(target, 'replay-play').disabled).toBe(false);
    expect(target.querySelector('.replay-bar')?.textContent).toContain('Lire avec la narration');

    // The editor kept the stamp of the file: the next save is not a conflict.
    await key('r');
    await drag(line(5000, 15000, 15000));
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(saveNote).toHaveBeenCalled());
    expect(saveNote.mock.calls.at(-1)![2]).toEqual(recorded);
    await vi.waitFor(() => expect(status()).toBe('Enregistrée'));
    expect(target.querySelector('.banner.error')).toBeNull();
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(4);
  });

  it('cancels a recording without saving anything', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const { path, target, button, onclose } = await replay();
    const before = mockLibrary().notes.get(path)!.stamp;
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action(target, 'narration-start')).toBeTruthy());
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(500);
    action(target, 'narration-cancel').click();
    await vi.waitFor(() => expect(action(target, 'narration-start')).toBeTruthy());
    expect(stop).toHaveBeenCalledWith(false, null, null, null);
    expect(mockLibrary().notes.get(path)!.stamp).toEqual(before);
    expect(await api.narrationInfo(path)).toBeNull();
    expect(target.textContent).toContain("Cette note n'a pas de narration.");
    expect((await api.narrationStatus()).state).not.toBe('recording');
    // Free to leave again.
    expect(button('Relecture').disabled).toBe(false);
    button('Bibliothèque').click();
    await tick();
    expect(onclose).toHaveBeenCalled();
  });

  it('lets the recording go on after the end of the replay, and stops the replay with the recording', async () => {
    const { target } = await replay();
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action(target, 'narration-start')).toBeTruthy());
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(5600);
    // The replay is over, the last image stays and the recording runs on.
    expect(barState(target)).toBe('no');
    expect(roleText(target, 'replay-current')).toBe('0:05');
    expect(target.querySelector('.elapsed')?.textContent).toContain('Enregistrement : 0:05');
    expect(target.querySelector('[data-section="narration"]')?.textContent).toContain(
      "La relecture est terminée ; l'enregistrement continue",
    );
    await vi.advanceTimersByTimeAsync(1000);
    expect(target.querySelector('.elapsed')?.textContent).toContain('Enregistrement : 0:06');
    action(target, 'narration-stop').click();
    await vi.waitFor(() => expect(storedText(target)).toBe('Narration enregistrée dans la note (0:06).'));
  });

  it('explains when Windows blocks the microphone, in the panel and during the recording', async () => {
    mockNarration.silent = true;
    const { target } = await replay();
    action(target, 'narration-open').click();
    await tick();
    await vi.advanceTimersByTimeAsync(1500);
    const silent = () => target.querySelector<HTMLElement>('[data-capture="silent"]');
    expect(silent()?.textContent).toContain("Windows bloque peut-être l'accès au micro");
    expect(silent()?.textContent).toContain('Paramètres > Confidentialité et sécurité > Microphone');
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(1500);
    expect(silent()?.textContent).toContain("Windows bloque peut-être l'accès au micro");
  });

  it('replaces a narration after saying so, and deletes it after asking', async () => {
    const deleteNarration = vi.spyOn(api, 'deleteNarration');
    const saveNote = vi.spyOn(api, 'saveNote');
    const path = demoPath('Liste de courses');
    setMockNarration(path, { durationMs: 2000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const { target, drag, key, status } = await replay();
    await vi.waitFor(() => expect(storedText(target)).toBe('Narration enregistrée dans la note (0:02).'));
    action(target, 'narration-open').click();
    await tick();
    expect(target.querySelector('[data-slot="replaces"]')?.textContent).toBe(
      'Un nouvel enregistrement remplace la narration actuelle.',
    );
    action(target, 'narration-close').click();
    await tick();

    action(target, 'narration-delete').click();
    await tick();
    expect(target.textContent).toContain('Supprimer définitivement la narration de cette note ?');
    expect(deleteNarration).not.toHaveBeenCalled();
    action(target, 'narration-delete-yes').click();
    await vi.waitFor(() => expect(deleteNarration).toHaveBeenCalled());
    const stamp = mockLibrary().notes.get(path)!.stamp;
    await vi.waitFor(() => expect(target.textContent).toContain("Cette note n'a pas de narration."));
    expect(target.querySelector('[data-action="replay-narration"]')).toBeNull();
    expect(await api.narrationInfo(path)).toBeNull();

    // Saving still works with the stamp the deletion left.
    await key('r');
    await drag(line(5000, 15000, 15000));
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(saveNote).toHaveBeenCalled());
    expect(saveNote.mock.calls.at(-1)![2]).toEqual(stamp);
    await vi.waitFor(() => expect(status()).toBe('Enregistrée'));
  });

  it('reports a recording that cannot start and stays ready for another try', async () => {
    mockNarration.microphones = [];
    const { target } = await replay();
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(target.textContent).toContain('Aucun micro détecté'));
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(target.querySelector('[data-slot="record-error"]')).toBeTruthy());
    expect(barState(target)).toBe('no');
    expect(action<HTMLButtonElement>(target, 'replay-play').disabled).toBe(false);
    expect(action(target, 'narration-start')).toBeTruthy();
  });

  it('does not let the window close while a narration is being recorded, and saves it when the editor goes away', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const win = fakeWindow();
    const { path, target } = await replay({ windowClose: win.closer });
    const before = mockLibrary().notes.get(path)!.stamp;
    action(target, 'narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action(target, 'narration-start')).toBeTruthy());
    action(target, 'narration-start').click();
    await vi.waitFor(() => expect(barState(target)).toBe('yes'));
    await vi.advanceTimersByTimeAsync(300);
    expect(win.request()).toBe(true);
    await tick();
    expect(win.destroy).not.toHaveBeenCalled();
    expect(target.querySelector('.editor-notice')?.textContent).toContain('Arrêtez ou annulez');

    // Torn down anyway: the recording is not thrown away silently, it goes into its note.
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(stop).toHaveBeenCalledWith(true, before, path, expect.any(Number)));
    expect(stop).not.toHaveBeenCalledWith(false, expect.anything(), expect.anything(), expect.anything());
    await vi.waitFor(async () => expect(await api.narrationInfo(path)).not.toBeNull());
    expect((await api.narrationStatus()).state).toBe('idle');
  });

  it('freezes the title and the copy buttons while recording, and sends the note to the library as usual afterwards', async () => {
    const { path, target, button, drag, titleInput, key } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    vi.useFakeTimers();
    button('Relecture').click();
    await tick();
    expect(titleInput().disabled).toBe(false);
    await startRecording(target);
    expect(titleInput().disabled).toBe(true);

    // The note changes on disk meanwhile: the banner offers a copy, which would move the note.
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    expect(button('Enregistrer comme copie').disabled).toBe(true);
    // Ctrl+S (the note is dirty) saves nothing under the recording's feet.
    await key('s', { ctrlKey: true });
    expect(button('Enregistrer comme copie').disabled).toBe(true);

    action(target, 'narration-cancel').click();
    await vi.waitFor(() => expect(titleInput().disabled).toBe(false));
    expect(button('Enregistrer comme copie').disabled).toBe(false);
  });

  it('stops into the note under the name it has now', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const { path, target, typeTitle, blurTitle, onsaved } = await replay();
    // Renamed in the replay, before the recording: the file moved.
    typeTitle('Courses du samedi');
    blurTitle();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const renamed = onsaved.mock.calls.at(-1)![0] as string;
    expect(renamed).not.toBe(path);
    const stamp = mockLibrary().notes.get(renamed)!.stamp;
    await startRecording(target);
    action(target, 'narration-stop').click();
    await vi.waitFor(() => expect(stop).toHaveBeenCalledWith(true, stamp, renamed, expect.any(Number)));
    await vi.waitFor(() => expect(storedText(target)).toContain('Narration enregistrée dans la note'));
    expect(await api.narrationInfo(renamed)).not.toBeNull();
  });

  it('keeps a recording that cannot be saved: the panel explains, then retries into a copy of the note', async () => {
    const retry = vi.spyOn(api, 'narrationRetry');
    const { path, target, button, drag, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    vi.useFakeTimers();
    button('Relecture').click();
    await tick();
    await startRecording(target);
    // Another PC saved the note meanwhile.
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    action(target, 'narration-stop').click();
    await vi.waitFor(() => expect(pendingBlock(target)).toBeTruthy());
    expect(target.querySelector('[data-slot="record-error"]')?.textContent).toContain('la note a été modifiée entre-temps');
    expect(pendingBlock(target)?.textContent).toContain('est conservée');
    expect((await api.narrationStatus()).pending).toBe(true);
    // Nothing else can start meanwhile; the choices are the retry and the discard.
    expect(target.querySelector('[data-action="narration-open"]')).toBeNull();
    expect(action(target, 'narration-retry').textContent?.trim()).toBe('Réessayer');
    expect(action(target, 'narration-discard').textContent?.trim()).toBe("Abandonner l'enregistrement");
    expect(await api.narrationInfo(path)).toBeNull();

    // The note on disk is still not the one the editor holds: it stays pending.
    action(target, 'narration-retry').click();
    await vi.waitFor(() => expect(retry).toHaveBeenCalledTimes(1));
    expect(retry.mock.calls[0][0]).toBe(path);
    await vi.waitFor(() => expect(target.querySelector('[data-slot="record-error"]')).toBeTruthy());
    expect(pendingBlock(target)).toBeTruthy();

    // The user saves a copy, which the editor then edits: the retry goes into that one.
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved.mock.calls.at(-1)![0]).not.toBe(path));
    const copy = onsaved.mock.calls.at(-1)![0] as string;
    await vi.waitFor(() => expect(target.querySelector('[data-disk]')).toBeNull());
    const copyStamp = mockLibrary().notes.get(copy)!.stamp;
    action(target, 'narration-retry').click();
    await vi.waitFor(() => expect(retry).toHaveBeenCalledTimes(2));
    expect(retry.mock.calls[1]).toEqual([copy, copyStamp]);
    await vi.waitFor(() => expect(pendingBlock(target)).toBeNull());
    await vi.waitFor(() => expect(storedText(target)).toContain('Narration enregistrée dans la note'));
    expect(target.querySelector('.editor-notice')?.textContent).toContain('Narration enregistrée');
    expect(target.querySelector('[data-slot="record-error"]')).toBeNull();
    expect(await api.narrationInfo(copy)).not.toBeNull();
    expect((await api.narrationStatus()).pending).toBe(false);
    // The editor holds the stamp the retry left: the next save is not a conflict.
    expect(mockLibrary().notes.get(copy)!.stamp).not.toEqual(copyStamp);
    expect(action(target, 'narration-open')).toBeTruthy();
  });

  it('abandons a kept recording only after asking', async () => {
    mockNarration.saveConflict = true;
    const discard = vi.spyOn(api, 'narrationDiscard');
    const { target } = await replay();
    await startRecording(target);
    action(target, 'narration-stop').click();
    await vi.waitFor(() => expect(pendingBlock(target)).toBeTruthy());
    expect(target.querySelector('[data-slot="record-error"]')?.textContent).toContain('la note a été modifiée entre-temps');

    action(target, 'narration-discard').click();
    await tick();
    expect(pendingBlock(target)?.textContent).toContain('Abandonner définitivement cet enregistrement ?');
    expect(discard).not.toHaveBeenCalled();
    action(target, 'narration-discard-no').click();
    await tick();
    expect(action(target, 'narration-discard')).toBeTruthy();
    expect(pendingBlock(target)?.textContent).not.toContain('Abandonner définitivement');

    action(target, 'narration-discard').click();
    await tick();
    action(target, 'narration-discard-yes').click();
    await vi.waitFor(() => expect(discard).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(pendingBlock(target)).toBeNull());
    expect((await api.narrationStatus()).pending).toBe(false);
    expect(target.querySelector('[data-slot="record-error"]')).toBeNull();
    expect(action(target, 'narration-open')).toBeTruthy();
  });

  it('offers the kept recording again when the panel appears with one waiting', async () => {
    const path = demoPath('Liste de courses');
    mockNarration.saveConflict = true;
    await api.narrationStart(null, { kind: 'replay', path, speed: 1, maxPauseMs: null, startMs: 0 });
    await expect(api.narrationStop(true, null, path, null)).rejects.toMatchObject({ code: 'noteConflict' });
    mockNarration.saveConflict = false;
    const { target } = await replay();
    await vi.waitFor(() => expect(pendingBlock(target)).toBeTruthy());
    expect(target.querySelector('[data-action="narration-open"]')).toBeNull();
    expect(action(target, 'narration-retry')).toBeTruthy();
    action(target, 'narration-retry').click();
    await vi.waitFor(() => expect(pendingBlock(target)).toBeNull());
    await vi.waitFor(() => expect(storedText(target)).toContain('Narration enregistrée dans la note'));
  });

  const SAVE_NOTE =
    "Le son est ajouté à la note telle qu'elle est enregistrée sur le disque : les modifications non enregistrées n'en font pas partie.";

  it('says that the narration goes into the note as it is on disk', async () => {
    const { target } = await replay();
    action(target, 'narration-open').click();
    await tick();
    const section = target.querySelector('[data-section="narration"]')!;
    expect(section.querySelector('[data-slot="save-note"]')?.textContent).toBe(SAVE_NOTE);
    expect(section.querySelector('[data-slot="unsaved-warning"]')).toBeNull();
  });

  it('suggests saving first when the note has unsaved changes', async () => {
    const { target, button, drag } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    vi.useFakeTimers();
    button('Relecture').click();
    await tick();
    action(target, 'narration-open').click();
    await tick();
    expect(target.querySelector('[data-slot="save-note"]')?.textContent).toBe(SAVE_NOTE);
    expect(target.querySelector('[data-slot="unsaved-warning"]')?.textContent).toBe(
      "Cette note a des modifications non enregistrées. Enregistrez-la d'abord (Ctrl+S), sinon elles ne seront pas conservées avec la narration.",
    );
  });

  it('gives the microphone chosen here to the app state', async () => {
    const onchange = vi.fn();
    const { target } = await replay({ onchange });
    action(target, 'narration-open').click();
    await tick();
    const select = target.querySelector<HTMLSelectElement>('select[data-field="microphone"]')!;
    await vi.waitFor(() => expect(select.options.length).toBe(3));
    select.value = 'mock-mic-2';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ microphone: 'mock-mic-2' })));
    expect(onchange).toHaveBeenCalledTimes(1);
    expect(select.value).toBe('mock-mic-2');
  });

  it('hears a narration that began before the first stroke: the timeline opens with a lead-in', async () => {
    const media = fakeMedia();
    // A live recording: 3 s of sound, the first of which comes before the drawing starts.
    setMockNarration(demoPath('Liste de courses'), { durationMs: 3000, base: 'note', speed: 1, maxPauseMs: null, startMs: -1000 });
    const { target, paths } = await replay();
    await vi.waitFor(() => expect(action(target, 'replay-narration')).toBeTruthy());
    // Without the sound the replay is the drawing's (5 s).
    expect(roleText(target, 'replay-total')).toBe('0:05');
    action<HTMLInputElement>(target, 'replay-narration').click();
    await vi.waitFor(() => expect(target.querySelector('audio')).toBeTruthy());
    const audio = target.querySelector('audio')!;
    // 1 s of lead-in, then the 5 s of drawing (longer than the sound's 2 s after instant 0).
    expect(roleText(target, 'replay-total')).toBe('0:06');
    // At the start of the replay, the playhead stays at the start: the sound is heard from its beginning.
    expect(roleText(target, 'replay-current')).toBe('0:00');
    const seek = action<HTMLInputElement>(target, 'replay-seek');
    expect(seek.max).toBe('6000');

    // Inside the lead-in the sound is already at its place, and the page is the first image.
    const shown = (position: string) => {
      seek.value = position;
      seek.dispatchEvent(new Event('input', { bubbles: true }));
      return tick().then(() => paths().map((p) => p.getAttribute('d')));
    };
    const atStart = await shown('0');
    expect(audio.currentTime).toBe(0);
    expect(await shown('500')).toEqual(atStart);
    expect(audio.currentTime).toBe(0.5);
    expect(await shown('1000')).toEqual(atStart);
    expect(audio.currentTime).toBe(1);
    // After the lead-in the drawing moves on, with the sound still in step.
    expect(await shown('2500')).not.toEqual(atStart);
    expect(audio.currentTime).toBe(2.5);
    // The sound ended (3 s); the drawing goes on.
    expect(await shown('4500')).not.toEqual(atStart);
    expect(audio.currentTime).toBe(3);

    // Played from the start: the sound begins at once and the drawing waits for its turn.
    await shown('0');
    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    await vi.advanceTimersByTimeAsync(400);
    expect(media.plays).toHaveBeenCalled();
    expect(audio.currentTime).toBeGreaterThan(0.1);
    expect(audio.currentTime).toBeLessThan(0.6);
    expect(roleText(target, 'replay-current')).toBe('0:00');
    expect(paths().map((p) => p.getAttribute('d'))).toEqual(atStart);
    await vi.advanceTimersByTimeAsync(1700);
    expect(audio.currentTime).toBeGreaterThan(1.6);
    expect(roleText(target, 'replay-current')).toBe('0:02');
    expect(paths().map((p) => p.getAttribute('d'))).not.toEqual(atStart);

    // Switching the sound off gives the drawing's own timeline back, on the same moment of the page.
    action<HTMLButtonElement>(target, 'replay-play').click();
    await tick();
    action<HTMLInputElement>(target, 'replay-narration').click();
    await tick();
    expect(roleText(target, 'replay-total')).toBe('0:05');
  });

  it('holds the window close while a video is being exported', async () => {
    const win = fakeWindow();
    const { target, button, onclose } = await render('Liste de courses', { windowClose: win.closer });
    button('Vidéo').click();
    await tick();
    const double = (name: string) =>
      [...target.querySelectorAll('.video-dialog-double button')].find((b) => b.textContent === name) as HTMLButtonElement;
    expect(win.request()).toBe(false);
    double('run-video').click();
    await tick();
    expect(win.request()).toBe(true);
    await tick();
    expect(win.destroy).not.toHaveBeenCalled();
    expect(target.querySelector('.editor-notice')?.textContent).toContain('Un export vidéo est en cours');
    expect(target.querySelector('.editor-notice')?.getAttribute('role')).toBe('alert');
    // The way back to the library waits too.
    button('Bibliothèque').click();
    await tick();
    expect(onclose).not.toHaveBeenCalled();
    double('end-video').click();
    await tick();
    expect(win.request()).toBe(false);
    // A dialog that goes away releases the window too.
    double('run-video').click();
    await tick();
    expect(win.request()).toBe(true);
    double('close-video').click();
    await tick();
    expect(win.request()).toBe(false);
  });

  it('has its texts in English', async () => {
    setLocale('en');
    mockNarration.silent = true;
    setMockNarration(demoPath('Liste de courses'), { durationMs: 2000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const { target, button } = await render('Liste de courses');
    vi.useFakeTimers();
    button('Replay').click();
    await tick();
    await vi.waitFor(() => expect(storedText(target)).toBe('Narration saved in the note (0:02).'));
    expect(target.querySelector('.replay-bar')?.textContent).toContain('Play with narration');
    action(target, 'narration-open').click();
    await tick();
    expect(target.textContent).toContain('A new recording replaces the current narration.');
    await vi.advanceTimersByTimeAsync(1500);
    expect(target.querySelector('[data-capture="silent"]')?.textContent).toContain('Let desktop apps access your microphone');
    expect(button('Delete the narration')).toBeTruthy();
  });
});

describe('note editor: narration sound', () => {
  beforeEach(() => {
    resetMockNarration();
    makeSlow('Liste de courses');
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    Reflect.deleteProperty(URL, 'createObjectURL');
    Reflect.deleteProperty(URL, 'revokeObjectURL');
  });

  it('says so when the browser cannot play the sound, and when the sound cannot be fetched', async () => {
    fakeMedia();
    setMockNarration(demoPath('Liste de courses'), { durationMs: 3000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const { target, button } = await render('Liste de courses');
    button('Relecture').click();
    await vi.waitFor(() => expect(action(target, 'replay-narration')).toBeTruthy());
    action<HTMLInputElement>(target, 'replay-narration').click();
    await vi.waitFor(() => expect(target.querySelector('audio')).toBeTruthy());
    expect(target.querySelector('[data-slot="replay-error"]')).toBeNull();
    target.querySelector('audio')!.dispatchEvent(new Event('error'));
    await tick();
    expect(target.querySelector('[data-slot="replay-error"]')?.textContent).toBe('Le son de la narration ne peut pas être lu.');
    action<HTMLInputElement>(target, 'replay-narration').click();
    await tick();
    expect(target.querySelector('[data-slot="replay-error"]')).toBeNull();

    vi.spyOn(api, 'narrationAudio').mockRejectedValueOnce({ code: 'noRecording', message: 'gone' });
    action<HTMLInputElement>(target, 'replay-narration').click();
    await vi.waitFor(() => expect(target.querySelector('[data-slot="replay-error"]')).toBeTruthy());
    expect(target.querySelector('audio')).toBeNull();
    expect(action<HTMLInputElement>(target, 'replay-narration').checked).toBe(false);
  });
});
