// @vitest-environment jsdom
/// <reference types="node" />
/**
 * « Chaque raccourci affiché agit vraiment dans sa fenêtre » (milestone M16; docs/design/side-panel.md, §6, "Tests of
 * the help"): a probe per entry of the editor's table (`EDITOR_SHORTCUTS`, which the Help tab shows). Each probe
 * mounts the editor, sets what the shortcut needs (a selection, a replay, a note with a next one…), dispatches the
 * entry's first shown chord on `window`, and checks its effect. A first test fails when an entry has no probe, so a
 * key added to the table must be probed. The table against the key mapping, in both directions, is
 * `lib/editor/shortcuts.test.ts`.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { EDITOR_SHORTCUTS } from '../../lib/editor/shortcuts';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import type { Stroke } from '../../lib/note';
import { share } from '../../lib/share-state.svelte';
import { chordEvents } from '../../lib/shortcuts';
import EditorPagesHarness from './EditorPagesHarness.svelte';

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

const PHYSICS = 'Cours de physique';
const SHOPPING = 'Liste de courses';
const PARENTS = 'Réunion parents';

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
  share.reset();
  resetMockShare();
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** A stroke of 21 samples spread over [t0, t1] ms. */
function slowStroke(y: number, t0: number, t1: number): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({ x: 2000 + i * 500, y, pressure: 0.5, t: Math.round(t0 + ((t1 - t0) * i) / 20) })),
  };
}

/** The editor on « Liste de courses » (three strokes, a five seconds replay), the middle one of three notes. */
async function mountEditor() {
  const entry = mockLibrary().notes.get(demoPath(SHOPPING))!;
  entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, 5000)] };
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const onpage = vi.fn();
  const component = mount(EditorPagesHarness, {
    target,
    props: { initial: demoPath(SHOPPING), pages: [PHYSICS, SHOPPING, PARENTS].map(demoPath), app, onclose, onsaved, onpage },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const paths = () => target.querySelectorAll('g.ink path:not(.live)').length;
  const pressed = () => target.querySelector<HTMLElement>('.toolbar .tool[aria-pressed="true"]')?.title;
  const click = async (selector: string) => {
    q(selector)!.click();
    await tick();
  };
  const tool = async (title: string) => {
    [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === title)!.click();
    await tick();
  };
  /** The entry's first shown chord, as the keyboard sends it, on `window`: returns whether the key was taken. */
  const press = async (id: string) => {
    const entry = EDITOR_SHORTCUTS.find((s) => s.id === id)!;
    const shown = entry.keys.find((k) => k.shown !== false)!;
    const [input] = chordEvents(shown.chord);
    const event = new KeyboardEvent('keydown', { ...input, bubbles: true, cancelable: true });
    window.dispatchEvent(event);
    await tick();
    window.dispatchEvent(new KeyboardEvent('keyup', { ...input, bubbles: true }));
    await tick();
    return event.defaultPrevented;
  };
  const selectAll = async () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true, cancelable: true }));
    await tick();
    expect(q('[data-section="selection"]')).toBeTruthy();
  };
  const scale = () => Number(q('svg.canvas')!.getAttribute('data-view')!.split(' ')[0]);
  const dirty = () => q('.editor')!.getAttribute('data-dirty');
  return { target, q, paths, pressed, click, tool, press, selectAll, scale, dirty, onclose, onsaved, onpage };
}

type View = Awaited<ReturnType<typeof mountEditor>>;

/** What each entry of the table does in the mounted editor (the first shown chord of the entry is pressed). */
const PROBES: Record<string, (v: View) => Promise<void>> = {
  select: async (v) => {
    await v.tool('Stylo');
    await v.press('select');
    expect(v.pressed()).toBe('Sélection');
  },
  pen: async (v) => {
    await v.tool('Sélection');
    await v.press('pen');
    expect(v.pressed()).toBe('Stylo');
  },
  eraser: async (v) => {
    await v.press('eraser');
    expect(v.pressed()).toBe('Gomme');
  },
  text: async (v) => {
    await v.press('text');
    expect(v.pressed()).toBe('Texte');
  },
  equation: async (v) => {
    await v.press('equation');
    expect(v.pressed()).toBe('Équation');
  },
  pan: async (v) => {
    await v.press('pan');
    expect(v.pressed()).toBe('Déplacer la page');
  },
  panHold: async (v) => {
    expect(v.q('.space')).toBeNull();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true }));
    await tick();
    // The page pans while the key is held, and stops when it is let go.
    expect(v.q('.space')).toBeTruthy();
    window.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
    await tick();
    expect(v.q('.space')).toBeNull();
  },
  straighten: async (v) => {
    await v.selectAll();
    await v.press('straighten');
    // Shapes are straightened, or the notice says none was recognised: either way the key acted.
    await vi.waitFor(() => expect(v.q('.editor-notice')?.textContent).toMatch(/forme/));
  },
  replay: async (v) => {
    await v.press('replay');
    expect(v.q('.replay-bar')).toBeTruthy();
  },
  undo: async (v) => {
    v.target.querySelector<HTMLButtonElement>('.toolbar button[title="Rouge"]')!.click();
    await tick();
    [...v.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim() === 'Couleur')!.click();
    await tick();
    expect(v.dirty()).toBe('yes');
    await v.press('undo');
    expect(v.dirty()).toBe('no');
  },
  redo: async (v) => {
    v.target.querySelector<HTMLButtonElement>('.toolbar button[title="Rouge"]')!.click();
    await tick();
    [...v.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim() === 'Couleur')!.click();
    await tick();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true, cancelable: true }));
    await tick();
    expect(v.dirty()).toBe('no');
    await v.press('redo');
    expect(v.dirty()).toBe('yes');
  },
  selectAll: async (v) => {
    expect(v.q('[data-section="selection"]')).toBeNull();
    await v.press('selectAll');
    expect(v.q('[data-section="selection"]')?.textContent).toContain('Traits sélectionnés : 3');
  },
  clipboard: async (v) => {
    await v.selectAll();
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText, readText: async () => '' }, configurable: true });
    await v.press('clipboard');
    // The browser's own copy event does not come under jsdom: the editor does the copy itself after a moment.
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(String((writeText.mock.calls[0] as unknown[])[0])).toContain('ardoise-strokes');
  },
  delete: async (v) => {
    await v.selectAll();
    expect(v.paths()).toBe(3);
    await v.press('delete');
    expect(v.paths()).toBe(0);
  },
  escape: async (v) => {
    await v.selectAll();
    await v.press('escape');
    expect(v.q('[data-section="selection"]')).toBeNull();
  },
  fit: async (v) => {
    const fitted = v.scale();
    v.target.querySelector<HTMLButtonElement>('button[aria-label="Agrandir"]')!.click();
    await tick();
    expect(v.scale()).not.toBe(fitted);
    await v.press('fit');
    expect(v.scale()).toBe(fitted);
  },
  fitWidth: async (v) => {
    const fitted = v.scale();
    await v.press('fitWidth');
    // The page as wide as the stage: not the whole page in the stage, unless both are the same.
    const widthFit = v.scale();
    v.target.querySelector<HTMLButtonElement>('button[aria-label="Agrandir"]')!.click();
    await tick();
    expect(v.scale()).not.toBe(widthFit);
    await v.press('fitWidth');
    expect(v.scale()).toBe(widthFit);
    expect(Number.isFinite(fitted)).toBe(true);
  },
  pages: async (v) => {
    await v.press('pages');
    // The first shown chord is Alt + ←: the note before this one.
    await vi.waitFor(() => expect(v.onpage).toHaveBeenCalledWith(demoPath(PHYSICS)));
  },
  save: async (v) => {
    v.target.querySelector<HTMLButtonElement>('.toolbar button[title="Rouge"]')!.click();
    await tick();
    [...v.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim() === 'Couleur')!.click();
    await tick();
    expect(v.dirty()).toBe('yes');
    await v.press('save');
    await vi.waitFor(() => expect(v.dirty()).toBe('no'));
  },
  close: async (v) => {
    await v.press('close');
    expect(v.onclose).toHaveBeenCalled();
  },
  // Ctrl + Q quits the whole app: `App.svelte` answers it, for every screen. The editor leaves it alone.
  quit: async (v) => {
    const taken = await v.press('quit');
    expect(taken).toBe(false);
    expect(v.onclose).not.toHaveBeenCalled();
    expect(v.dirty()).toBe('no');
  },
  leaveReplay: async (v) => {
    await v.press('replay');
    expect(v.q('.replay-bar')).toBeTruthy();
    await v.press('leaveReplay');
    expect(v.q('.replay-bar')).toBeNull();
  },
  playPause: async (v) => {
    await v.press('replay');
    const bar = () => v.q('.replay-bar')!.getAttribute('data-playing');
    expect(bar()).toBe('no');
    await v.press('playPause');
    expect(bar()).toBe('yes');
    await v.press('playPause');
    expect(bar()).toBe('no');
  },
};

describe('the editor\'s shortcuts act in the editor', () => {
  it('has a probe for every entry of the table, and only for those', () => {
    expect(Object.keys(PROBES).sort()).toEqual(EDITOR_SHORTCUTS.map((s) => s.id).sort());
  });

  for (const entry of EDITOR_SHORTCUTS) {
    it(`${entry.id}`, async () => {
      await PROBES[entry.id](await mountEditor());
    });
  }
});
