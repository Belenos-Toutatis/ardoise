// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The editor's wiring of its projection (milestone M17; docs/design/paper-projection-narration.md §2.6, §2.7, §2.9 E):
 * the Projection tab, the watch at mount, what is fed while the projection is open (the page, a replay, the stroke in
 * progress, what the canvas shows), the frame of the projected area, the blue dot, another note kept projected, and
 * the projection closed when the editor goes. The projection itself (package D) is replaced by a recording stand-in.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import type { MonitorInfo } from '../../lib/live';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import type { Stroke } from '../../lib/note';
import { share } from '../../lib/share-state.svelte';
import { openSideTab, tabPanelOf } from '../side/side-test-helpers';
import { ProjectionDouble } from './EditorProjectionDouble.svelte';
import EditorPagesHarness from './EditorPagesHarness.svelte';

vi.mock('../../lib/editor/projection.svelte', async () => ({
  EditorProjection: (await import('./EditorProjectionDouble.svelte')).ProjectionDouble,
}));

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

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

const PROJECTOR: MonitorInfo = { key: 'Projector@1920,0', name: 'Projector', x: 1920, y: 0, width: 1920, height: 1080, scale: 1, primary: false, chosen: true };
const BLACK = { r: 0, g: 0, b: 0, a: 255 };

function slowStroke(y: number, t0: number, t1: number): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({ x: 2000 + i * 500, y, pressure: 0.5, t: Math.round(t0 + ((t1 - t0) * i) / 20) })),
  };
}

let cleanup: (() => void) | null = null;

beforeEach(() => {
  ProjectionDouble.instances = [];
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

/** The editor on `title`, with the two demo notes as its pages (Alt + → goes to the other). */
async function render(title = 'Liste de courses', other = 'Cours de physique') {
  const path = demoPath(title);
  const pages = [path, demoPath(other)];
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(EditorPagesHarness, { target, props: { initial: path, pages, app, onclose, onsaved: vi.fn(), onpage: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const projection = ProjectionDouble.instances.at(-1)!;
  const svg = () => target.querySelector<SVGSVGElement>('svg.canvas')!;
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg().getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number, t: number) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: t })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg().dispatchEvent(event);
  };
  const draw = async (y: number) => {
    pointer('pointerdown', 5000, y, 0);
    for (let i = 1; i <= 5; i++) pointer('pointermove', 5000 + i * 2000, y, i * 10);
    pointer('pointerup', 15_000, y, 60);
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const tab = (id: string) => target.querySelector<HTMLElement>(`[role="tab"][data-tab="${id}"]`)!;
  const strokes = () => target.querySelectorAll('g.ink path:not(.live)').length;
  return { path, pages, target, projection, draw, key, tab, strokes, onclose };
}

/** The projection opens on the projector (as Rust tells the watcher). */
async function project(projection: ProjectionDouble) {
  projection.presentation = PROJECTOR;
  await tick();
}

describe('the editor’s projection', () => {
  it('has the Projection tab of the projection (screen, area, share), and hears where it is open from the mount', async () => {
    const { target, projection } = await render();
    expect(projection.count('watch')).toBe(1);
    await openSideTab(target, 'projection');
    for (const section of ['projector', 'zone', 'share']) {
      const el = target.querySelector(`[data-section="${section}"]`);
      expect(el, section).toBeTruthy();
      expect(tabPanelOf(el!)).toBe('projection');
    }
    // « Projeter » asks the projection (once a screen is listed).
    projection.monitors = [PROJECTOR];
    projection.monitorKey = PROJECTOR.key;
    projection.loaded = true;
    await tick();
    target.querySelector<HTMLButtonElement>('[data-section="projector"] [data-action="project"]')!.click();
    await tick();
    expect(projection.count('open')).toBe(1);
  });

  it('feeds nothing while it is closed, then the page as soon as it opens, and every change', async () => {
    const { projection, draw, path } = await render();
    await draw(15_000);
    expect(projection.count('show')).toBe(0);
    expect(projection.count('stroke')).toBe(0);
    // What the canvas shows is told all along: the area is right when the projection opens.
    expect(projection.visibles.length).toBeGreaterThan(0);

    await project(projection);
    expect(projection.shows).toHaveLength(1);
    const first = projection.shows[0];
    expect(first.note.id).toBe(mockLibrary().notes.get(path)!.note.id);
    expect(first.note.strokes).toHaveLength(2);
    expect(first.imageKey).toBeNull();

    // A stroke: streamed while drawn, then the page with it.
    await draw(17_000);
    const kinds = projection.strokes.map((e) => e.kind);
    expect(kinds[0]).toBe('begin');
    expect(kinds.filter((k) => k === 'grow').length).toBeGreaterThan(0);
    expect(projection.strokes.at(-1)).toMatchObject({ kind: 'end', committed: true });
    expect(projection.shows.at(-1)!.note.strokes).toHaveLength(3);
  });

  it('draws the projected area on the page as a dashed frame while there is one', async () => {
    const { target, projection } = await render();
    expect(target.querySelector('[data-slot="projected-area"]')).toBeNull();
    await project(projection);
    projection.shownFrame = { x: 1000, y: 2000, width: 15_000, height: 8_437.5 };
    await tick();
    const frame = target.querySelector('[data-slot="projected-area"]')!;
    expect(frame).toBeTruthy();
    expect(frame.getAttribute('width')).toBe('15000');
    projection.shownFrame = null;
    await tick();
    expect(target.querySelector('[data-slot="projected-area"]')).toBeNull();
  });

  it('projects a replay as the editor shows it', async () => {
    const path = demoPath('Liste de courses');
    const entry = mockLibrary().notes.get(path)!;
    entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, 5000)] };
    const { projection, key, target } = await render();
    await project(projection);
    expect(projection.shows.at(-1)!.note.strokes).toHaveLength(3);
    vi.useFakeTimers();
    await key('r');
    expect(target.querySelector('.replay-bar')).toBeTruthy();
    // At the start of the replay the page is (nearly) empty, then it fills in as it plays.
    const atStart = projection.shows.at(-1)!.note.strokes.length;
    expect(atStart).toBeLessThan(3);
    target.querySelector<HTMLButtonElement>('[data-action="replay-play"]')!.click();
    await tick();
    await vi.advanceTimersByTimeAsync(3500);
    expect(projection.shows.at(-1)!.note.strokes.length).toBeGreaterThan(atStart);
    // Back to editing: the whole page again.
    await key('r');
    expect(projection.shows.at(-1)!.note.strokes).toHaveLength(3);
  });

  it('shows the blue dot on Projection while it projects, with or without a share', async () => {
    const { tab, projection } = await render();
    expect(tab('projection').hasAttribute('data-dot')).toBe(false);
    await project(projection);
    expect(tab('projection').dataset.dot).toBe('live');
    expect(tab('projection').textContent).toContain('projection ou partage en cours');
    // Closed from its own window (Esc): the watcher tells, the dot goes.
    projection.presentation = null;
    await tick();
    expect(tab('projection').hasAttribute('data-dot')).toBe(false);
  });

  it('keeps projecting when the editor moves on to another note, and stops when the editor goes', async () => {
    const { projection, key, pages, onclose } = await render();
    await project(projection);
    const other = mockLibrary().notes.get(pages[1])!.note.id;
    await key('ArrowRight', { altKey: true });
    await vi.waitFor(() => expect(projection.shows.at(-1)!.note.id).toBe(other));
    // The same projection: never closed by the move.
    expect(ProjectionDouble.instances).toHaveLength(1);
    expect(projection.count('dispose')).toBe(0);
    expect(projection.count('close')).toBe(0);
    expect(onclose).not.toHaveBeenCalled();
    cleanup?.();
    cleanup = null;
    expect(projection.count('dispose')).toBe(1);
  });
});
