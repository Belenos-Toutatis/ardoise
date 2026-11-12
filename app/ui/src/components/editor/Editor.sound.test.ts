// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The editor's sound while writing (milestone M17; docs/design/paper-projection-narration.md §3.4, §3.5, §3.7, §3.8):
 * the Son tab's mode and its storage, the stored narration's line outside a replay, the replace question with a stored
 * or a kept sound, the writing recording (its target, the strokes on the sound's clock, a paste at the sound's time),
 * « Arrêter » saving note and sound at once, a failed save keeping the sound (a copy takes it, « Ne pas enregistrer »
 * lets it go), « Annuler » keeping the strokes, what waits while it records, and the strokes after it. The PC's
 * microphone is never used: `mock-narration.ts` answers.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { encodeStrokes } from '../../lib/editor/clipboard';
import { setLocale } from '../../lib/i18n.svelte';
import { freshStamp, mockLibrary, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { mockNarration, mockNarrationOf, mockStagedNarration, resetMockNarration, setMockNarration } from '../../lib/mock-narration';
import type { Stroke } from '../../lib/note';
import type { NarrationInfo } from '../../lib/video';
import { openSideTab } from '../side/side-test-helpers';
import Editor from './Editor.svelte';
import { NARRATION_MODE_KEY } from './EditorSound.svelte';

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

const BLACK = { r: 0, g: 0, b: 0, a: 255 };

/** A stroke of 21 samples spread over [t0, t1] ms. */
function slowStroke(y: number, t0: number, t1: number): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({ x: 2000 + i * 500, y, pressure: 0.5, t: Math.round(t0 + ((t1 - t0) * i) / 20) })),
  };
}

const TITLE = 'Liste de courses';
/** The demo note's last sample after `beforeEach` (strokes over [0, 1000], [3000, 4000], [4200, 5000] ms). */
const LAST_SAMPLE = 5000;

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  mockNarration.receivingAfterMs = 0;
  setLocale('fr');
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
  const entry = mockLibrary().notes.get(demoPath(TITLE))!;
  entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, LAST_SAMPLE)] };
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** The editor on the demo note, then the clock under the test's control (the pointer events carry its time). */
async function render(title = TITLE) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  vi.useFakeTimers();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`[data-action="${name}"]`);
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (target.querySelector('svg.canvas')!.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  /** A pointer event at page point (x, y), stamped with the page's clock now (as a browser does). */
  const pointer = (type: string, x: number, y: number) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: performance.now() })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  /** A stroke along y from x0 to x1, a move every 10 ms; returns `Date.now()` at its pen down. */
  const draw = async (y: number, x0 = 5000, x1 = 15_000): Promise<number> => {
    const downAt = Date.now();
    pointer('pointerdown', x0, y);
    for (let i = 1; i <= 10; i++) {
      vi.advanceTimersByTime(10);
      pointer('pointermove', x0 + ((x1 - x0) * i) / 10, y);
    }
    pointer('pointerup', x1, y);
    await tick();
    return downAt;
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const strokes = () => target.querySelectorAll('g.ink path:not(.live)').length;
  const status = () => q('.status')?.textContent?.trim();
  const notice = () => q('.editor-notice');
  const sound = () => q('[data-tab-panel="sound"]')!;
  const phase = () => q('[data-tab-panel="sound"] .record')?.dataset.phase ?? null;
  /** Opens Son and the recording panel, and presses « Démarrer » (the question, if any, is the caller's). */
  const pressStart = async () => {
    await openSideTab(target, 'sound');
    if (!action('narration-start')) {
      action('narration-open')!.click();
      await tick();
    }
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    action('narration-start')!.click();
    await tick();
  };
  /** Starts a writing recording (no narration to replace) and waits until the strokes follow its clock. */
  const startWriting = async () => {
    await pressStart();
    await vi.waitFor(() => expect(phase()).toBe('recording'));
    // The clock is made once the start has answered.
    await vi.advanceTimersByTimeAsync(0);
  };
  return { path, target, svg, q, action, pointer, draw, key, strokes, status, notice, sound, phase, pressStart, startWriting, onclose, onsaved };
}

const fileOf = (path: string) => mockLibrary().notes.get(path)!;

describe('the Son tab outside a replay', () => {
  it('opens on « Pendant que j’écris », remembers the mode on this PC, and shows each mode’s content', async () => {
    const { target, q, sound } = await render();
    await openSideTab(target, 'sound');
    const group = q('[data-field="narration-mode"]')!;
    expect(group.getAttribute('role')).toBe('radiogroup');
    expect(group.getAttribute('aria-label')).toBe('Enregistrer le son');
    expect(group.classList.contains('pane-seg')).toBe(true);
    const radio = (value: string) => group.querySelector<HTMLInputElement>(`input[type="radio"][value="${value}"]`)!;
    expect(radio('writing').checked).toBe(true);
    expect(radio('writing').closest('label')!.textContent?.trim()).toBe("Pendant que j'écris");
    expect(radio('replay').closest('label')!.textContent?.trim()).toBe('Pendant une relecture');
    // Writing: its help, its recording, nothing of the replay's.
    expect(q('[data-slot="writing-help"]')?.textContent).toContain('Le son et ce que vous écrivez sur l\'ordinateur');
    expect(sound().querySelector('[data-action="narration-open"]')?.textContent).toContain('Enregistrer en écrivant');
    expect(sound().querySelector<HTMLButtonElement>('[data-action="narration-open"]')!.title).toBe(
      "Parler en écrivant : les traits prennent l'heure du son",
    );
    expect(sound().querySelector('[data-action="sound-replay"]')).toBeNull();

    radio('replay').click();
    await tick();
    expect(localStorage.getItem(NARRATION_MODE_KEY)).toBe('replay');
    expect(sound().textContent).toContain("La narration s'écoute et s'enregistre pendant une relecture.");
    expect(sound().querySelector('[data-action="sound-replay"]')).toBeTruthy();
    expect(sound().querySelector('[data-action="narration-open"]')).toBeNull();

    // The next editor opens on the mode chosen.
    cleanup?.();
    vi.useRealTimers();
    const again = await render();
    expect(again.q<HTMLInputElement>('[data-field="narration-mode"] input[value="replay"]')!.checked).toBe(true);
    again.q<HTMLInputElement>('[data-field="narration-mode"] input[value="writing"]')!.click();
    await tick();
    expect(localStorage.getItem(NARRATION_MODE_KEY)).toBe('writing');
  });

  it('says what the recording does before it starts', async () => {
    const { action, sound, target } = await render();
    await openSideTab(target, 'sound');
    action('narration-open')!.click();
    await tick();
    const text = sound().textContent ?? '';
    expect(text).toContain("Les traits écrits pendant l'enregistrement prennent l'heure du son.");
    expect(sound().querySelector('[data-slot="save-note"]')?.textContent).toBe(
      "À l'arrêt, la note est enregistrée avec le son et ce que vous avez écrit.",
    );
    expect(sound().querySelector('[data-slot="replaces"]')).toBeNull();
    expect(sound().querySelector('select[data-field="microphone"]')).toBeTruthy();
  });

  it('shows the narration the note stores outside a replay, from its opening, and deletes it after asking', async () => {
    const info: NarrationInfo = { durationMs: 2000, base: 'note', speed: 1, maxPauseMs: null, startMs: 5800 };
    setMockNarration(demoPath(TITLE), info);
    const narrationInfo = vi.spyOn(api, 'narrationInfo');
    const { sound, action, path } = await render();
    expect(sound().querySelector('[data-slot="narration-stored"]')?.textContent?.trim()).toBe('Narration enregistrée dans la note (0:02).');
    // Told by `open_note`: not read again.
    expect(narrationInfo).not.toHaveBeenCalled();
    action('narration-delete')!.click();
    await tick();
    action('narration-delete-yes')!.click();
    await vi.waitFor(() => expect(sound().querySelector('[data-slot="narration-none"]')?.textContent).toBe("Cette note n'a pas de narration."));
    expect(mockNarrationOf(path)).toBeNull();
  });
});

describe('a recording while writing', () => {
  it('starts with the writing target, times the strokes on the sound, and « Arrêter » saves note and sound at once', async () => {
    const realStart = api.narrationStart;
    const start = vi.spyOn(api, 'narrationStart');
    const stop = vi.spyOn(api, 'narrationStop');
    const saveNote = vi.spyOn(api, 'saveNote');
    // When the stand-in's capture begins (its first sample), on the wall clock.
    let soundStart = 0;
    start.mockImplementation((device, target) => {
      soundStart = Date.now();
      return realStart(device, target);
    });
    const { path, draw, action, strokes, status, notice, sound, startWriting, target } = await render();
    expect(strokes()).toBe(3);
    await startWriting();
    expect(start).toHaveBeenCalledWith(null, { kind: 'writing', path });
    expect(sound().textContent).toContain('Écrivez : vos traits suivent le son.');
    // The red dot of the tab.
    expect(target.querySelector('[role="tab"][data-tab="sound"]')?.getAttribute('data-dot')).toBe('rec');

    await vi.advanceTimersByTimeAsync(1500);
    const firstDown = await draw(12_000);
    await vi.advanceTimersByTimeAsync(1500);
    await draw(14_000);
    await vi.advanceTimersByTimeAsync(1000);
    expect(strokes()).toBe(5);

    action('narration-stop')!.click();
    await vi.waitFor(() => expect(stop).toHaveBeenCalled());
    // Not written by the stop: kept by Rust, with the wall clock at note time 0.
    expect(stop.mock.calls[0].slice(0, 3)).toEqual([true, null, null]);
    expect(typeof stop.mock.calls[0][3]).toBe('number');
    await vi.waitFor(() => expect(saveNote).toHaveBeenCalled());
    const [savedPath, savedNote, , imageKey, narrationKey] = saveNote.mock.calls.at(-1)!;
    expect(savedPath).toBe(path);
    expect(imageKey).toBeUndefined();
    expect(narrationKey).toMatch(/^staged-/);
    expect(savedNote.strokes).toHaveLength(5);
    await vi.waitFor(() => expect(notice()?.textContent).toBe('Son enregistré (0:04) avec vos traits.'));
    expect(status()).toBe('Enregistrée');
    expect(mockStagedNarration()).toBeNull();

    // On disk: the note's narration in note time, and the new strokes on its clock, after the old ink.
    const info = mockNarrationOf(path)!;
    expect(info).toMatchObject({ base: 'note', speed: 1, maxPauseMs: null });
    expect(info.durationMs).toBeGreaterThanOrEqual(4000);
    const origin = LAST_SAMPLE + 1000;
    // The microphone opened just before the clock started: at most a few ms before the origin.
    expect(info.startMs).toBeLessThanOrEqual(origin);
    expect(info.startMs).toBeGreaterThan(origin - 200);
    const written = fileOf(path).note.strokes;
    const first = written[3].points[0].t;
    // The stroke and the sound heard at the same wall instant share their note time.
    expect(first - info.startMs).toBe(firstDown - soundStart);
    expect(first).toBeGreaterThanOrEqual(origin + 1500);
    expect(written[4].points[0].t - first).toBe(100 + 1500);
    expect(written[2].points.at(-1)!.t).toBe(LAST_SAMPLE);
    await vi.waitFor(() =>
      expect(sound().querySelector('[data-slot="narration-stored"]')?.textContent?.trim()).toBe('Narration enregistrée dans la note (0:04).'),
    );
    // The panel is folded again, the dot gone.
    expect(action('narration-open')).toBeTruthy();
    expect(target.querySelector('[role="tab"][data-tab="sound"]')?.hasAttribute('data-dot')).toBe(false);
  });

  it('puts a stroke drawn after the recording after the end of its sound (O14)', async () => {
    const { path, draw, action, startWriting, notice, sound, key } = await render();
    await startWriting();
    await vi.advanceTimersByTimeAsync(3000);
    action('narration-stop')!.click();
    await vi.waitFor(() => expect(notice()?.textContent).toContain('Son enregistré'));
    // The narration read back from the note (its own start, not the editor's estimate).
    await vi.waitFor(() => expect(sound().querySelector('[data-slot="narration-stored"]')).toBeTruthy());
    const info = mockNarrationOf(path)!;
    expect(info.startMs + info.durationMs).toBeGreaterThan(LAST_SAMPLE + 500);
    await draw(15_000);
    const saved = vi.spyOn(api, 'saveNote');
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(saved).toHaveBeenCalled());
    const last = saved.mock.calls.at(-1)![1].strokes.at(-1)!;
    expect(last.points[0].t).toBe(info.startMs + info.durationMs + 500);
  });

  it('pastes at the sound’s current time during the recording (O15)', async () => {
    const { startWriting, strokes, key } = await render();
    await startWriting();
    await vi.advanceTimersByTimeAsync(2000);
    const copied = [slowStroke(15_000, 0, 300)];
    const clipboardData = { getData: () => encodeStrokes(copied), setData: () => {} };
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', { value: clipboardData });
    document.body.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(true);
    expect(strokes()).toBe(4);
    // Saving during the recording is allowed (the strokes go now, the sound at the stop).
    const saveNote = vi.spyOn(api, 'saveNote');
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(saveNote).toHaveBeenCalled());
    expect(saveNote.mock.calls.at(-1)).toHaveLength(4);
    const pasted = saveNote.mock.calls.at(-1)![1].strokes.at(-1)!;
    // The recording started after the note's ink (its origin), the paste two seconds later, give or take the start's waits.
    const origin = LAST_SAMPLE + 1000;
    expect(pasted.points[0].t).toBeGreaterThanOrEqual(origin + 2000);
    expect(pasted.points[0].t).toBeLessThan(origin + 2300);
    // The rhythm of the copy is kept.
    expect(pasted.points.at(-1)!.t - pasted.points[0].t).toBe(300);
  });

  it('« Annuler » drops the sound and keeps the strokes (O16)', async () => {
    const stop = vi.spyOn(api, 'narrationStop');
    const { path, draw, action, startWriting, strokes, status, sound } = await render();
    await startWriting();
    await vi.advanceTimersByTimeAsync(500);
    await draw(12_000);
    action('narration-cancel')!.click();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    expect(stop).toHaveBeenCalledWith(false, null, null, null);
    expect(strokes()).toBe(4);
    expect(status()).toBe('Modifications non enregistrées');
    expect(mockNarrationOf(path)).toBeNull();
    expect(mockStagedNarration()).toBeNull();
    expect(sound().querySelector('[data-slot="narration-none"]')).toBeTruthy();
  });

  it('keeps nothing and says why when Windows kept the microphone silent; the strokes stay', async () => {
    mockNarration.silent = true;
    mockNarration.silentAfterMs = 0;
    const saveNote = vi.spyOn(api, 'saveNote');
    const { draw, action, startWriting, strokes, sound } = await render();
    await startWriting();
    await draw(12_000);
    await vi.advanceTimersByTimeAsync(1200);
    action('narration-stop')!.click();
    await vi.waitFor(() => expect(sound().querySelector('[data-slot="record-error"]')).toBeTruthy());
    expect(sound().querySelector('[data-slot="record-error"]')?.textContent).toContain("Windows bloque peut-être l'accès au micro");
    expect(saveNote).not.toHaveBeenCalled();
    expect(strokes()).toBe(4);
    expect(mockStagedNarration()).toBeNull();
  });

  it('holds back what acts on the timeline it writes: replay, original, split, video, leaving', async () => {
    const path = demoPath(TITLE);
    const entry = fileOf(path);
    entry.original = { source: 'firstVersion', page: entry.note.page, layers: entry.note.layers, strokes: [slowStroke(3000, 0, 1000)] };
    const { action, key, draw, startWriting, notice, q, target, strokes, onclose } = await render();
    await startWriting();
    const busy ="Arrêtez ou annulez d'abord l'enregistrement de la narration.";
    const said = () => {
      const text = notice()?.textContent;
      expect(notice()?.getAttribute('role')).toBe('alert');
      return text;
    };

    await key('r');
    expect(q('.replay-bar')).toBeNull();
    expect(said()).toBe(busy);
    expect(action('replay-toggle')!.disabled).toBe(true);

    const toolbar = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.includes(label))!;
    toolbar('Découper').click();
    await tick();
    expect(q('.split-panel')).toBeNull();
    expect(said()).toBe(busy);

    action('video-export')!.click();
    await tick();
    expect(q('.video-dialog-double')).toBeNull();
    expect(said()).toBe(busy);

    await openSideTab(target, 'page');
    await vi.waitFor(() => expect(action('original')!.disabled).toBe(false));
    action('original')!.click();
    await tick();
    action('original-confirm')!.click();
    await tick();
    expect(strokes()).toBe(3);
    expect(said()).toBe(busy);

    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await tick();
    expect(onclose).not.toHaveBeenCalled();
    expect(said()).toBe(busy);

    // Every other edit goes on: a stroke, undo, redo.
    await draw(15_000);
    expect(strokes()).toBe(4);
    await key('z', { ctrlKey: true });
    expect(strokes()).toBe(3);
    await key('y', { ctrlKey: true });
    expect(strokes()).toBe(4);
    // The mode cannot change under the recording.
    expect(q<HTMLInputElement>('[data-field="narration-mode"] input[value="replay"]')!.disabled).toBe(true);
    action('narration-cancel')!.click();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    expect(action('replay-toggle')!.disabled).toBe(false);
  });
});

describe('the replace question', () => {
  it('asks first when the note has a narration: « Garder l’ancienne » records nothing, « Remplacer » records', async () => {
    setMockNarration(demoPath(TITLE), { durationMs: 83_000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const start = vi.spyOn(api, 'narrationStart');
    const { action, pressStart, q, phase } = await render();
    await pressStart();
    expect(q('[data-slot="replace-question"]')?.textContent).toContain(
      'Cette note a déjà une narration (1:23). La remplacer par ce nouvel enregistrement ?',
    );
    action('narration-replace-no')!.click();
    await tick();
    expect(start).not.toHaveBeenCalled();
    expect(q('[data-slot="replace-question"]')).toBeNull();
    expect(phase()).toBe('idle');

    action('narration-start')!.click();
    await tick();
    action('narration-replace-yes')!.click();
    await vi.waitFor(() => expect(phase()).toBe('recording'));
    expect(start).toHaveBeenCalledTimes(1);
    action('narration-cancel')!.click();
    await vi.waitFor(() => expect(phase()).toBe('idle'));
  });

  it('asks in a replay too (O11)', async () => {
    setMockNarration(demoPath(TITLE), { durationMs: 2000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const start = vi.spyOn(api, 'narrationStart');
    const { action, key, q } = await render();
    await key('r');
    action('narration-open')!.click();
    await tick();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    action('narration-start')!.click();
    await tick();
    expect(q('[data-slot="replace-question"]')).toBeTruthy();
    expect(start).not.toHaveBeenCalled();
    action('narration-replace-yes')!.click();
    await vi.waitFor(() => expect(start).toHaveBeenCalledWith(null, expect.objectContaining({ kind: 'replay' })));
    action('narration-cancel')!.click();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
  });
});

describe('a sound whose save failed', () => {
  /** Records 2 s while writing one stroke, after the note changed on disk: the save at the stop fails. */
  async function failedSave() {
    const view = await render();
    await view.startWriting();
    await vi.advanceTimersByTimeAsync(500);
    await view.draw(12_000);
    await vi.advanceTimersByTimeAsync(1500);
    fileOf(view.path).stamp = freshStamp();
    notifyLibraryChanged();
    view.action('narration-stop')!.click();
    await vi.waitFor(() => expect(view.notice()?.textContent).toBe('Le son est gardé en mémoire : il ira dans la note à son prochain enregistrement.'));
    return view;
  }

  it('stays kept with the strokes: the conflict banner, the unsaved state, the line in Son', async () => {
    const { q, status, notice, sound, path } = await failedSave();
    expect(notice()?.getAttribute('role')).toBe('alert');
    expect(q('.banner.error')?.textContent).toContain('la note a été modifiée entre-temps');
    expect(status()).toBe('Modifications non enregistrées');
    expect(sound().querySelector('[data-slot="narration-staged"]')?.textContent).toBe(
      'Le son est gardé en mémoire : il ira dans la note à son prochain enregistrement.',
    );
    expect(mockStagedNarration()).not.toBeNull();
    expect(mockNarrationOf(path)).toBeNull();
  });

  it('goes into a copy with the strokes timed on it (« Enregistrer comme copie »)', async () => {
    const saveNote = vi.spyOn(api, 'saveNote');
    const { target, onsaved, status, sound } = await failedSave();
    const key = mockStagedNarration()!.key;
    [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Enregistrer comme copie')!.click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copy = onsaved.mock.calls.at(-1)![0] as string;
    expect(saveNote.mock.calls.at(-1)![0]).toBe(copy);
    expect(saveNote.mock.calls.at(-1)![4]).toBe(key);
    expect(mockNarrationOf(copy)).toMatchObject({ base: 'note' });
    expect(fileOf(copy).note.strokes).toHaveLength(4);
    await vi.waitFor(() => expect(status()).toBe('Enregistrée'));
    await vi.waitFor(() => expect(sound().querySelector('[data-slot="narration-stored"]')).toBeTruthy());
    expect(sound().querySelector('[data-slot="narration-staged"]')).toBeNull();
  });

  it('goes with « Ne pas enregistrer »', async () => {
    const drop = vi.spyOn(api, 'narrationDropStaged');
    const { target, onclose } = await failedSave();
    const key = mockStagedNarration()!.key;
    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await tick();
    const dialog = document.querySelector('[role="alertdialog"]')!;
    expect(dialog).toBeTruthy();
    dialog.querySelector<HTMLButtonElement>('button.danger')!.click();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(drop).toHaveBeenCalledWith(key);
    expect(mockStagedNarration()).toBeNull();
  });

  it('counts as the note’s narration for a new recording: the question, then « Remplacer » lets it go', async () => {
    const drop = vi.spyOn(api, 'narrationDropStaged');
    const start = vi.spyOn(api, 'narrationStart');
    const view = await failedSave();
    const key = mockStagedNarration()!.key;
    start.mockClear();
    await view.pressStart();
    expect(view.q('[data-slot="replace-question"]')?.textContent).toContain('(0:02)');
    view.action('narration-replace-yes')!.click();
    await vi.waitFor(() => expect(view.phase()).toBe('recording'));
    expect(drop).toHaveBeenCalledWith(key);
    expect(drop.mock.invocationCallOrder[0]).toBeLessThan(start.mock.invocationCallOrder[0]);
    expect(view.sound().querySelector('[data-slot="narration-staged"]')).toBeNull();
    view.action('narration-cancel')!.click();
    await vi.waitFor(() => expect(view.phase()).toBe('idle'));
  });

  it('goes with the note at its next save (Ctrl+S): note and sound together again', async () => {
    const drop = vi.spyOn(api, 'narrationDropStaged');
    const view = await failedSave();
    const key = mockStagedNarration()!.key;
    const saveNote = vi.spyOn(api, 'saveNote');
    await view.key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(saveNote).toHaveBeenCalled());
    expect(saveNote.mock.calls.at(-1)![4]).toBe(key);
    // Still a conflict (the file changed on disk): the sound stays kept.
    await vi.waitFor(() => expect(view.status()).toBe('Modifications non enregistrées'));
    expect(mockStagedNarration()?.key).toBe(key);
    expect(drop).not.toHaveBeenCalled();
  });

  it('is let go when Rust no longer has it (`noStagedNarration`): the error says where its copy is', async () => {
    const view = await failedSave();
    const key = mockStagedNarration()!.key;
    // Rust lost it (dropped elsewhere): the next save says so and the sound is no longer counted.
    await api.narrationDropStaged(key);
    await view.key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(view.q('.banner.error')?.textContent).toContain('dossier de secours des narrations'));
    expect(view.sound().querySelector('[data-slot="narration-staged"]')).toBeNull();
  });
});

describe('the sound while writing in English', () => {
  it('has its texts', async () => {
    setLocale('en');
    const { target, q, action, sound } = await render();
    await openSideTab(target, 'sound');
    expect(q('[data-field="narration-mode"]')?.getAttribute('aria-label')).toBe('Record the sound');
    expect(sound().textContent).toContain('While I write');
    expect(sound().textContent).toContain('During a replay');
    expect(action('narration-open')!.textContent).toContain('Record while writing');
    action('narration-open')!.click();
    await tick();
    expect(sound().textContent).toContain('The strokes written during the recording take the time of the sound.');
  });
});
