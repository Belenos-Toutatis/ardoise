// @vitest-environment jsdom
/// <reference types="node" />
// The editor's milestone M7 editing features: "Redresser les formes" and "Revenir à l'original".
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { Stroke } from '../../lib/note';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real stroke geometry and shape recognition, from the module's bytes (jsdom cannot fetch it).
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
  setLocale('fr');
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

async function render(title: string) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const paths = () => [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')];
  const action = (name: string) => {
    const found = target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
    if (!found) throw new Error(`no button ${name}`);
    return found;
  };
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number) => {
    const at = client(x, y);
    const options = { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: 1, pointerId: 1, pressure: 0.5 };
    const Ctor = (globalThis as { PointerEvent?: typeof MouseEvent }).PointerEvent ?? MouseEvent;
    const event = new Ctor(type, options);
    for (const key of ['pointerId', 'pressure'] as const) {
      if (!(key in event)) Object.defineProperty(event, key, { value: options[key] });
    }
    svg.dispatchEvent(event);
  };
  const drag = async (points: [number, number][]) => {
    const [first, ...rest] = points;
    pointer('pointerdown', first[0], first[1]);
    for (const [x, y] of rest) pointer('pointermove', x, y);
    const last = points[points.length - 1];
    pointer('pointerup', last[0], last[1]);
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const notice = () => target.querySelector('.editor-notice')?.textContent?.trim();
  const stored = () => mockLibrary().notes.get(path)!;
  const save = async () => {
    onsaved.mockClear();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
  };
  const originalSection = () => target.querySelector<HTMLElement>('[data-section="original"]')!;
  return { path, target, paths, action, drag, key, notice, stored, save, originalSection };
}

/** A wobbly hand-drawn circle (centre, radius), going round once and a little more. */
function circle(cx: number, cy: number, r: number): [number, number][] {
  return Array.from({ length: 140 }, (_, i) => {
    const a = (i / 130) * 2 * Math.PI;
    const wobble = 1 + 0.02 * Math.sin(3 * a) + 0.01 * Math.sin(7 * a + 1);
    return [cx + r * wobble * Math.cos(a), cy + r * wobble * Math.sin(a)];
  });
}

/** A slightly wobbly straight line. */
function wobblyLine(x0: number, y0: number, x1: number, y1: number): [number, number][] {
  return Array.from({ length: 80 }, (_, i) => {
    const k = i / 79;
    return [x0 + (x1 - x0) * k, y0 + (y1 - y0) * k + 40 * Math.sin(k * 9)];
  });
}

/** The demo note becomes a note of live mode, which has no original until its first edit. */
function fromLiveMode(title: string) {
  const entry = mockLibrary().notes.get(demoPath(title))!;
  entry.note = { ...entry.note, origin: { kind: 'live' } };
  delete entry.original;
}

const pathData = (paths: SVGPathElement[]) => paths.map((p) => p.getAttribute('d'));

function isClosed(stroke: Stroke): boolean {
  const first = stroke.points[0];
  const last = stroke.points[stroke.points.length - 1];
  return first.x === last.x && first.y === last.y;
}

describe('straighten shapes', () => {
  it('is disabled with an explanation until strokes are selected', async () => {
    const { action, key, notice } = await render('Liste de courses');
    await key('v');
    const button = action('straighten');
    expect(button.disabled).toBe(true);
    expect(button.title).toBe("Sélectionnez d'abord des traits avec le lasso, puis redressez-les.");
    expect(button.textContent).toContain('Redresser les formes');
    // The help line of the selection tool tells about it.
    expect(document.body.textContent).toContain('F : redresser les formes');
    // The key says the same instead of doing nothing.
    await key('f');
    expect(notice()).toBe("Sélectionnez d'abord des traits avec le lasso, puis redressez-les.");

    await key('a', { ctrlKey: true });
    expect(action('straighten').disabled).toBe(false);
    expect(action('straighten').title).toContain('droite, un cercle, une ellipse, un rectangle ou une flèche');
  });

  it('straightens the selected shapes in one undoable step', async () => {
    const { paths, action, drag, key, notice, stored, save } = await render('Liste de courses');
    await drag(circle(15000, 11000, 3000));
    await drag(wobblyLine(3000, 17000, 22000, 18500));
    expect(paths()).toHaveLength(3);
    const drawn = pathData(paths());

    await key('v');
    await key('a', { ctrlKey: true });
    action('straighten').click();
    await tick();
    expect(notice()).toBe('2 formes redressées (Ctrl+Z pour annuler).');
    expect(paths()).toHaveLength(3);
    const straightened = pathData(paths());
    // The demo's wavy stroke is no shape: it stays exactly as it was.
    expect(straightened[0]).toBe(drawn[0]);
    expect(straightened[1]).not.toBe(drawn[1]);
    expect(straightened[2]).not.toBe(drawn[2]);
    // Still selected, so that it can be moved or recoloured at once.
    expect(document.body.textContent).toContain('Traits sélectionnés : 3');

    // One undo step brings back the hand-drawn strokes, redo the straight ones.
    await key('z', { ctrlKey: true });
    expect(pathData(paths())).toEqual(drawn);
    await key('y', { ctrlKey: true });
    expect(pathData(paths())).toEqual(straightened);

    await save();
    const strokes = stored().note.strokes;
    expect(strokes).toHaveLength(3);
    expect(isClosed(strokes[1])).toBe(true);
    const line = strokes[2];
    // A clean line: every point on the segment between its ends, evenly pressed.
    const [a, b] = [line.points[0], line.points[line.points.length - 1]];
    for (const p of line.points) {
      const cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
      expect(Math.abs(cross) / Math.hypot(b.x - a.x, b.y - a.y)).toBeLessThan(1);
      expect(p.pressure).toBe(line.points[0].pressure);
    }
  });

  it('works with the F key and from the selection panel', async () => {
    const { paths, action, drag, key, notice } = await render('Liste de courses');
    await drag(circle(15000, 11000, 3000));
    await key('v');
    await key('a', { ctrlKey: true });
    await key('f');
    expect(notice()).toBe('1 forme redressée (Ctrl+Z pour annuler).');
    await key('z', { ctrlKey: true });
    action('straighten-selection').click();
    await tick();
    expect(notice()).toBe('1 forme redressée (Ctrl+Z pour annuler).');
    expect(paths()).toHaveLength(2);
  });

  it('says so when nothing in the selection is a shape', async () => {
    const { target, paths, action, key, notice } = await render('Réunion parents');
    const before = pathData(paths());
    await key('v');
    await key('a', { ctrlKey: true });
    action('straighten').click();
    await tick();
    expect(notice()).toContain('Aucune forme reconnue dans la sélection');
    expect(pathData(paths())).toEqual(before);
    // Nothing to undo.
    const undo = [...target.querySelectorAll('button')].find((b) => b.title === 'Annuler (Ctrl+Z)')!;
    expect(undo.disabled).toBe(true);
  });
});

describe('back to the original', () => {
  it('is disabled with an explanation for a note never edited', async () => {
    fromLiveMode('Liste de courses');
    const { originalSection } = await render('Liste de courses');
    await vi.waitFor(() => expect(originalSection().textContent).toContain("Pas encore d'original"));
    const button = originalSection().querySelector<HTMLButtonElement>('[data-action="original"]')!;
    expect(button.disabled).toBe(true);
    expect(button.title).toContain("Pas encore d'original");
  });

  it('goes back to the first version after a confirmation, in one undoable step', async () => {
    fromLiveMode('Liste de courses');
    const { paths, drag, key, notice, stored, save, originalSection } = await render('Liste de courses');
    const first = stored().note;
    const firstPaths = pathData(paths());

    // An edit of the ink and of the title is saved: the version before it is kept.
    await drag(wobblyLine(3000, 15000, 15000, 15000));
    await save();
    await vi.waitFor(() => expect(originalSection().textContent).toContain('première version enregistrée qui en contenait'));
    const button = originalSection().querySelector<HTMLButtonElement>('[data-action="original"]')!;
    expect(button.disabled).toBe(false);
    expect(button.textContent).toContain("Revenir à l'original");

    // The confirmation says what is lost and what stays; cancelling changes nothing.
    button.click();
    await tick();
    const dialog = originalSection().querySelector('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain("Vos modifications de l'encre seront perdues");
    expect(dialog.textContent).toContain('Le titre, les étiquettes, le favori, le papier et la narration restent tels quels.');
    expect(dialog.textContent).toContain("L'image de fond reste aussi");
    // The focus goes to its harmless button, and back to the button when it is cancelled.
    const cancel = originalSection().querySelector<HTMLButtonElement>('[data-action="original-cancel"]')!;
    expect(cancel.textContent?.trim()).toBe('Annuler');
    await vi.waitFor(() => expect(document.activeElement).toBe(cancel));
    cancel.click();
    await tick();
    expect(paths()).toHaveLength(2);
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(originalSection().querySelector('[data-action="original"]')),
    );

    originalSection().querySelector<HTMLButtonElement>('[data-action="original"]')!.click();
    await tick();
    originalSection().querySelector<HTMLButtonElement>('[data-action="original-confirm"]')!.click();
    await tick();
    expect(notice()).toContain("Encre de l'original rétablie");
    expect(pathData(paths())).toEqual(firstPaths);

    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(2);
    await key('y', { ctrlKey: true });
    expect(paths()).toHaveLength(1);

    // Saved like any edit: the first ink, the rest of the note unchanged.
    await save();
    const note = stored().note;
    expect(note.strokes).toEqual(first.strokes);
    expect(note.title).toBe(first.title);
    expect(note.tags).toEqual(first.tags);
    expect(note.favorite).toBe(first.favorite);
  });

  it('is available at once for a tablet note', async () => {
    const { paths, drag, originalSection } = await render('Cours de physique');
    expect(paths()).toHaveLength(2);
    await vi.waitFor(() => expect(originalSection().textContent).toContain("telle qu'elle est arrivée de la tablette"));
    await drag(wobblyLine(3000, 15000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    originalSection().querySelector<HTMLButtonElement>('[data-action="original"]')!.click();
    await tick();
    originalSection().querySelector<HTMLButtonElement>('[data-action="original-confirm"]')!.click();
    await tick();
    expect(paths()).toHaveLength(2);
  });
});
