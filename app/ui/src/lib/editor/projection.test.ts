// @vitest-environment jsdom
// The editor's side of its projection (M17, docs/design/paper-projection-narration.md §2.6, §11 O5): the screens, the
// watch, open and close, the projected area that follows the centre of the editor's view at its own zoom, the frame.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setLocale, t } from '../i18n.svelte';
import type { MonitorInfo } from '../live';
import { ZOOM_STEP, fitWidthView, projectedArea } from '../live-view';
import type { Note } from '../note';
import type { ProjectionStatus, ProjectionUpdate } from '../projection';
import { EditorProjection, type ProjectionCommands } from './projection.svelte';

const screen = (n: number, width: number, height: number, primary = false): MonitorInfo => ({
  key: `\\\\.\\DISPLAY${n}@${n * 1920},0`,
  name: `\\\\.\\DISPLAY${n}`,
  x: n * 1920,
  y: 0,
  width,
  height,
  scale: 1,
  primary,
  chosen: false,
});
const LAPTOP = screen(1, 1920, 1080, true);
/** A 16:10 projector. */
const PROJECTOR = screen(2, 1280, 800);
const LANDSCAPE = { width: 29700, height: 21600 };

function note(over: Partial<Note> = {}): Note {
  return {
    id: 'n1',
    title: '',
    created: 0,
    modified: 0,
    page: LANDSCAPE,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: [],
    timing: 'recorded',
    origin: { kind: 'live' },
    ...over,
  };
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** Commands that answer as Rust would, and remember what they were asked. */
function fake({ monitors = [LAPTOP, PROJECTOR], watched = { open: null } as ProjectionStatus } = {}) {
  const calls: string[] = [];
  const updates: ProjectionUpdate[] = [];
  let onStatus: ((status: ProjectionStatus) => void) | null = null;
  let openError: unknown = null;
  let resolveWatch: ((status: ProjectionStatus) => void) | null = null;
  const commands: ProjectionCommands = {
    listMonitors: async () => {
      calls.push('list');
      return monitors.map((m) => ({ ...m }));
    },
    projectionOpen: async (monitor: string | null) => {
      calls.push(`open ${monitor}`);
      if (openError) throw openError;
      const shown = monitors.find((m) => m.key === monitor) ?? PROJECTOR;
      return { ...shown, chosen: true };
    },
    projectionClose: async () => {
      calls.push('close');
    },
    projectionWatch: (callback: (status: ProjectionStatus) => void) => {
      calls.push('watch');
      onStatus = callback;
      return new Promise<ProjectionStatus>((resolve) => {
        resolveWatch = resolve;
        queueMicrotask(() => resolve(watched));
      });
    },
    projectionUpdate: async (update: ProjectionUpdate) => {
      updates.push(update);
    },
  };
  return {
    commands,
    calls,
    updates,
    tell: (status: ProjectionStatus) => onStatus?.(status),
    failOpen: (error: unknown) => (openError = error),
    resolveWatchNow: (status: ProjectionStatus) => resolveWatch?.(status),
  };
}

/** A projection whose view goes at once (no frame to wait for). */
function make(f = fake()) {
  return { f, projection: new EditorProjection(f.commands, { frame: (run) => run() }) };
}

const views = (updates: readonly ProjectionUpdate[]) =>
  updates.flatMap((u) => (u.kind === 'view' || u.kind === 'page' ? [u.view] : []));

beforeEach(() => setLocale('fr'));
afterEach(() => setLocale('fr'));

describe('the screens, the watch, open and close', () => {
  it('lists the screens and offers the first one that is not the primary one', async () => {
    const { projection } = make();
    expect(projection.loaded).toBe(false);
    await projection.refresh();
    expect(projection.loaded).toBe(true);
    expect(projection.monitors.map((m) => m.key)).toEqual([LAPTOP.key, PROJECTOR.key]);
    expect(projection.monitorKey).toBe(PROJECTOR.key);
    // A choice still listed is kept.
    projection.monitorKey = LAPTOP.key;
    await projection.refresh();
    expect(projection.monitorKey).toBe(LAPTOP.key);
  });

  it('hears where the projection is open, also when it is closed from its own window', async () => {
    const { f, projection } = make();
    projection.watch();
    projection.watch();
    await flush();
    expect(f.calls.filter((c) => c === 'watch')).toHaveLength(1);
    expect(projection.presentation).toBeNull();
    f.tell({ open: { ...PROJECTOR, chosen: true } });
    expect(projection.presentation?.key).toBe(PROJECTOR.key);
    // Esc in the projection window.
    f.tell({ open: null });
    expect(projection.presentation).toBeNull();
  });

  it('ignores the answer of the watch when a status was heard meanwhile', async () => {
    const f = fake();
    // The answer comes after a status heard on the channel.
    const slow: ProjectionCommands['projectionWatch'] = (callback) =>
      f.commands.projectionWatch(callback).then(() => new Promise<ProjectionStatus>((resolve) => setTimeout(() => resolve({ open: null }), 5)));
    const projection = new EditorProjection({ ...f.commands, projectionWatch: slow }, { frame: (run) => run() });
    projection.watch();
    f.tell({ open: { ...PROJECTOR, chosen: true } });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(projection.presentation?.key).toBe(PROJECTOR.key);
  });

  it('opens on the chosen screen and sends the page shown, with the whole-page view', async () => {
    const { f, projection } = make();
    await projection.refresh();
    const shown = note();
    projection.show(shown, 'key-1');
    await flush();
    // Nothing goes while closed.
    expect(f.updates).toHaveLength(0);
    const opening = projection.open();
    expect(projection.busy).toBe(true);
    await opening;
    await flush();
    expect(projection.busy).toBe(false);
    expect(f.calls).toContain(`open ${PROJECTOR.key}`);
    expect(projection.presentation?.key).toBe(PROJECTOR.key);
    expect(f.updates).toEqual([{ kind: 'page', note: shown, imageKey: 'key-1', view: { zoom: 1, x: 14850, y: 10800 } }]);
  });

  it('says why it could not open', async () => {
    const { f, projection } = make();
    f.failOpen({ code: 'noMonitor', message: 'no screen' });
    await projection.open();
    expect(projection.presentation).toBeNull();
    expect(projection.error).toBe(t('errorCodes.noMonitor', { detail: 'no screen' }));
    expect(projection.busy).toBe(false);
  });

  it('closes, and then sends nothing; opened again, the whole page goes again', async () => {
    const { f, projection } = make();
    const shown = note();
    projection.show(shown, null);
    await projection.open();
    await flush();
    await projection.close();
    expect(f.calls).toContain('close');
    expect(projection.presentation).toBeNull();
    projection.show(note({ strokes: [] }), null);
    projection.stroke({ kind: 'begin', id: 1, tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60, layer: 0, points: [], wallAtBase: 0, base: 0 });
    await flush();
    expect(f.updates.map((u) => u.kind)).toEqual(['page']);
    await projection.open();
    await flush();
    expect(f.updates.map((u) => u.kind)).toEqual(['page', 'page']);
  });

  it('forwards the stroke in progress only while open', async () => {
    const { f, projection } = make();
    projection.show(note(), null);
    const begin = { kind: 'begin', id: 3, tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60, layer: 0, points: [{ x: 1, y: 2, pressure: 0.5, t: 10 }], wallAtBase: 1000, base: 10 } as const;
    projection.stroke(begin);
    await flush();
    expect(f.updates).toHaveLength(0);
    await projection.open();
    await flush();
    projection.stroke({ ...begin, id: 4 });
    await flush();
    expect(f.updates.map((u) => u.kind)).toEqual(['page', 'strokeBegin']);
    expect(f.updates[1]).toMatchObject({ id: 4, points: [{ x: 1, y: 2, p: 0.5, t: 10, wall: 1000 }] });
  });

  it('closes the projection when the editor goes, and an opening under way once it resolves', async () => {
    const { f, projection } = make();
    await projection.open();
    projection.dispose();
    expect(f.calls.filter((c) => c === 'close')).toHaveLength(1);
    expect(projection.presentation).toBeNull();
    projection.show(note(), null);
    await flush();
    expect(f.updates).toHaveLength(0);

    const late = make();
    const opening = late.projection.open();
    late.projection.dispose();
    await opening;
    expect(late.f.calls).toEqual([`open null`, 'close']);
    expect(late.projection.presentation).toBeNull();
  });
});

describe('the projected area', () => {
  async function projecting(page = LANDSCAPE) {
    const made = make();
    await made.projection.refresh();
    made.projection.show(note({ page }), null);
    await made.projection.open();
    await flush();
    return made;
  }

  it('is the whole page at zoom 1, whatever the editor shows', async () => {
    const { f, projection } = await projecting();
    projection.visible({ x: 1000, y: 1000, width: 5000, height: 3000 });
    await flush();
    expect(projection.zoom).toBe(1);
    expect(projection.frame).toBeNull();
    expect(views(f.updates).at(-1)).toEqual({ zoom: 1, x: 14850, y: 10800 });
  });

  it('follows the centre of what the editor shows at its own zoom, kept on the page', async () => {
    const { f, projection } = await projecting();
    projection.visible({ x: 11000, y: 8000, width: 6000, height: 4000 });
    projection.zoomStep(1);
    projection.zoomStep(1);
    await flush();
    expect(projection.zoom).toBe(1.563);
    expect(views(f.updates).at(-1)).toEqual({ zoom: 1.563, x: 14000, y: 10000 });
    // Scrolling the editor moves the area, never its zoom.
    projection.visible({ x: 13000, y: 9000, width: 6000, height: 4000 });
    await flush();
    expect(views(f.updates).at(-1)).toEqual({ zoom: 1.563, x: 16000, y: 11000 });
    // Zooming the editor (a smaller visible area, same centre) changes nothing.
    const sent = f.updates.length;
    projection.visible({ x: 14500, y: 10000, width: 3000, height: 2000 });
    await flush();
    expect(f.updates).toHaveLength(sent);
    // Near a corner the area stays on the page.
    projection.visible({ x: -4000, y: -4000, width: 6000, height: 4000 });
    await flush();
    const corner = views(f.updates).at(-1)!;
    const area = projectedArea(LANDSCAPE, { ...corner, board: false, laser: false }, PROJECTOR.width / PROJECTOR.height);
    expect(area.x).toBeCloseTo(0, 0);
    expect(area.y).toBeCloseTo(0, 0);
  });

  it('takes the shape of the screen projected on, and shows it as the frame', async () => {
    const { projection } = await projecting();
    projection.visible({ x: 11000, y: 8000, width: 6000, height: 4000 });
    expect(projection.frame).toBeNull();
    projection.zoomStep(1);
    projection.zoomStep(1);
    const frame = projection.frame!;
    expect(frame).not.toBeNull();
    expect(frame.width / frame.height).toBeCloseTo(1280 / 800, 6);
    expect(frame.x + frame.width / 2).toBeCloseTo(14000, 0);
    expect(frame.y + frame.height / 2).toBeCloseTo(10000, 0);
    // Without a projection there is no frame.
    await projection.close();
    expect(projection.frame).toBeNull();
  });

  it('steps between 1 and 8, « Page entière » is 1, « Largeur » the zoom that fits the width (at least 1)', async () => {
    const { projection } = await projecting({ width: 21000, height: 29700 });
    for (let i = 0; i < 20; i++) projection.zoomStep(1);
    expect(projection.zoom).toBe(8);
    projection.zoomStep(-1);
    expect(projection.zoom).toBeCloseTo(8 / ZOOM_STEP, 3);
    projection.wholePage();
    expect(projection.zoom).toBe(1);
    expect(projection.frame).toBeNull();
    projection.zoomStep(-1);
    expect(projection.zoom).toBe(1);
    projection.pageWidth();
    const page = { width: 21000, height: 29700 };
    const want = fitWidthView({ zoom: 1, x: 0, y: 0, board: false, laser: false }, page, 1280 / 800).zoom;
    expect(want).toBeGreaterThan(1);
    expect(projection.zoom).toBeCloseTo(want, 2);
  });

  it('sends the view of a page of another size with its page', async () => {
    const { f, projection } = await projecting();
    projection.zoomStep(1);
    await flush();
    const portrait = note({ id: 'n2', page: { width: 21000, height: 29700 } });
    projection.show(portrait, null);
    await flush();
    const last = f.updates.at(-1)!;
    expect(last.kind).toBe('page');
    if (last.kind !== 'page') return;
    expect(last.note).toBe(portrait);
    expect(last.view.zoom).toBe(1.25);
    // Kept on the new page.
    const area = projectedArea(portrait.page, { ...last.view, board: false, laser: false }, 1280 / 800);
    expect(area.y).toBeGreaterThanOrEqual(-1);
    expect(area.y + area.height).toBeLessThanOrEqual(29701);
  });

  it('waits for the next frame to send the view: several changes in a frame send one', async () => {
    const f = fake();
    const frames: (() => void)[] = [];
    const projection = new EditorProjection(f.commands, { frame: (run) => frames.push(run) });
    projection.show(note(), null);
    await projection.open();
    await flush();
    projection.zoomStep(1);
    projection.visible({ x: 1000, y: 1000, width: 6000, height: 4000 });
    projection.visible({ x: 2000, y: 2000, width: 6000, height: 4000 });
    expect(frames).toHaveLength(1);
    frames.shift()!();
    await flush();
    expect(f.updates.map((u) => u.kind)).toEqual(['page', 'view']);
  });
});
