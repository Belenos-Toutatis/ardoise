// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M17 (docs/design/paper-projection-narration.md §2.6, §3.2, §3.8): the canvas times a stroke with the sound's
// clock during a writing recording (else after the ink and a note-time narration), tells its stroke in progress
// (`oninprogress`) and what it shows of the page (`onvisible`), and frames the projected area (`projected`).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TOOLS, docFromNote, setLayerVisible, type Doc, type ToolSettings } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import { visibleRect } from '../../lib/editor/view';
import type { StrokeClock } from '../../lib/editor/writing-clock';
import type { Rect } from '../../lib/editor/geometry';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { InProgress } from '../../lib/projection-feed';
import { touchPrefs } from '../../lib/touch-prefs.svelte';
import type { NarrationInfo } from '../../lib/video';
import Canvas from './Canvas.svelte';

let ink: Ink;
beforeAll(async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
});

let cleanup: (() => void) | null = null;

beforeEach(() => touchPrefs.set(false));
afterEach(() => {
  cleanup?.();
  cleanup = null;
  touchPrefs.set(false);
});

interface Init {
  pointerType?: string;
  buttons?: number;
  pointerId?: number;
  t?: number;
}

interface Props {
  doc?: Doc;
  tools?: Partial<ToolSettings>;
  strokeClock?: StrokeClock | null;
  narration?: NarrationInfo | null;
  projected?: Rect | null;
}

/** A canvas on a note with one stroke whose samples end at t = 200 (the fixture's 10 ms apart, 21 samples). */
async function render(props: Props = {}) {
  const doc = props.doc ?? docFromNote(note([hline(1000, 3000, 3000)]));
  const log: string[] = [];
  const events: InProgress[] = [];
  const onchange = vi.fn((next: Doc) => {
    void next;
    log.push('change');
  });
  const oninprogress = vi.fn((event: InProgress) => {
    // The points of a growing stroke are the canvas's own array: what was there when told.
    events.push(event.kind === 'end' ? event : { ...event, points: [...event.points] });
    log.push(event.kind === 'end' ? `end:${event.committed}` : event.kind);
  });
  const onvisible = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, {
    target,
    props: {
      doc,
      tools: { ...DEFAULT_TOOLS, mode: 'pen', ...props.tools },
      ink,
      onchange,
      onselect: vi.fn(),
      strokeClock: props.strokeClock ?? null,
      narration: props.narration ?? null,
      oninprogress,
      onvisible,
      projected: props.projected ?? null,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const view = () => {
    const [scale, x, y] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { scale, x, y };
  };
  const client = (px: number, py: number) => {
    const v = view();
    return { x: px * v.scale + v.x, y: py * v.scale + v.y };
  };
  const fire = (type: string, at: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: init.buttons ?? 1 });
    const extra: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(extra)) Object.defineProperty(event, key, { value, configurable: true });
    svg.dispatchEvent(event);
  };
  /** A stroke along y = 8000 from x = 8000, a move every 10 ms from `t0`. */
  const stroke = async (t0: number, init: Init = {}, end: 'pointerup' | 'pointercancel' = 'pointerup') => {
    let t = t0;
    fire('pointerdown', client(8000, 8000), { ...init, t });
    for (let i = 1; i <= 8; i++) fire('pointermove', client(8000 + i * 500, 8000), { ...init, t: (t += 10) });
    fire(end, client(12_000, 8000), { ...init, buttons: 0, t: (t += 10) });
    await tick();
  };
  const added = () => onchange.mock.calls.at(-1)![0].note.strokes.at(-1)!;
  return { svg, view, client, fire, stroke, onchange, oninprogress, onvisible, events, log, target, added };
}

describe('the times of a stroke', () => {
  it('without a clock, goes 500 ms after the note’s last sample, as before', async () => {
    const c = await render();
    await c.stroke(10_000);
    expect(c.added().points.map((p: { t: number }) => p.t)).toEqual([700, 710, 720, 730, 740, 750, 760, 770, 780]);
  });

  it('follows the sound’s clock during a writing recording: the note time of its events', async () => {
    const clock: StrokeClock = { noteTime: (timeStamp) => 50_000 + (timeStamp - 10_000) + 0.4 };
    const c = await render({ strokeClock: clock });
    await c.stroke(10_000);
    const times = c.added().points.map((p: { t: number }) => p.t);
    expect(times[0]).toBe(50_000);
    expect(times.at(-1)).toBe(50_080);
    // Whole milliseconds, as Rust's u32.
    expect(times.every((t: number) => Number.isInteger(t))).toBe(true);
    // A second stroke, 3 s later: 3 s later in note time too, whatever the note holds now.
    await c.stroke(13_000);
    expect(c.added().points[0].t).toBe(53_000);
  });

  it('reads the clock at the pen down only: the stroke keeps that time base to its end (the recording may stop meanwhile)', async () => {
    const noteTime = vi.fn((timeStamp: number) => 90_000 + timeStamp);
    const c = await render({ strokeClock: { noteTime } });
    c.fire('pointerdown', c.client(8000, 8000), { t: 100 });
    // The recording stops: whatever the clock would say now does not matter to this stroke.
    noteTime.mockImplementation(() => 0);
    c.fire('pointermove', c.client(9000, 8000), { t: 150 });
    c.fire('pointerup', c.client(9000, 8000), { buttons: 0, t: 200 });
    await tick();
    expect(c.added().points.map((p: { t: number }) => p.t)).toEqual([90_100, 90_150]);
    expect(noteTime).toHaveBeenCalledTimes(1);
  });

  it('after a narration in note time that ends after the ink, goes after its end (O14)', async () => {
    const narration: NarrationInfo = { durationMs: 5000, base: 'note', speed: 1, maxPauseMs: null, startMs: 1200 };
    const c = await render({ narration });
    await c.stroke(0);
    expect(c.added().points[0].t).toBe(1200 + 5000 + 500);
  });

  it('after a replay narration, keeps the editor’s rule', async () => {
    const narration: NarrationInfo = { durationMs: 60_000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 };
    const c = await render({ narration });
    await c.stroke(0);
    expect(c.added().points[0].t).toBe(700);
  });
});

describe('the stroke in progress', () => {
  it('is told when it begins, as it grows and when it ends, before the doc gets it', async () => {
    const doc = docFromNote(note([hline(1000, 3000, 3000)], 2));
    const c = await render({ doc: { ...doc, activeLayer: 1 }, tools: { tool: 'feltTip', width: 120, color: { r: 10, g: 20, b: 30, a: 255 } } });
    await c.stroke(4000);
    const [begin, ...rest] = c.events;
    expect(begin).toMatchObject({ kind: 'begin', tool: 'feltTip', width: 120, color: { r: 10, g: 20, b: 30, a: 255 }, layer: 1, base: 700 });
    if (begin.kind !== 'begin') throw new Error('no begin');
    expect(begin.points).toHaveLength(1);
    expect(begin.points[0].t).toBe(700);
    // A point's wall time is the page's time origin plus its event's timeStamp.
    expect(begin.wallAtBase).toBeCloseTo(performance.timeOrigin + 4000, 6);
    const grows = rest.filter((e) => e.kind === 'grow');
    expect(grows).toHaveLength(8);
    expect(grows.map((e) => (e.kind === 'grow' ? e.points.length : 0))).toEqual([2, 3, 4, 5, 6, 7, 8, 9]);
    expect(rest.every((e) => e.id === begin.id)).toBe(true);
    expect(c.log.slice(-2)).toEqual(['end:true', 'change']);
    expect(c.events.at(-1)).toEqual({ kind: 'end', id: begin.id, committed: true });
    // The stroke the doc gets has the points that were told.
    const last = rest.at(-2);
    expect(last?.kind === 'grow' ? last.points : []).toEqual(c.added().points);
  });

  it('gives each stroke its own id', async () => {
    const c = await render();
    await c.stroke(0);
    await c.stroke(1000);
    const ids = c.events.filter((e) => e.kind === 'begin').map((e) => e.id);
    expect(ids).toHaveLength(2);
    expect(ids[0]).not.toBe(ids[1]);
  });

  it('ends uncommitted when the pointer is taken away, and the doc gets nothing', async () => {
    const c = await render();
    await c.stroke(0, {}, 'pointercancel');
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.events.at(-1)).toMatchObject({ kind: 'end', committed: false });
  });

  it('ends uncommitted when a second finger takes over to pinch', async () => {
    touchPrefs.set(true);
    const c = await render();
    const finger = (id: number, t: number): Init => ({ pointerType: 'touch', pointerId: id, t });
    c.fire('pointerdown', c.client(8000, 8000), finger(1, 0));
    c.fire('pointermove', c.client(9000, 8000), finger(1, 10));
    c.fire('pointerdown', { x: 600, y: 400 }, finger(2, 20));
    await tick();
    expect(c.log).toEqual(['begin', 'grow', 'end:false']);
    expect(c.onchange).not.toHaveBeenCalled();
  });

  it('is never begun on a hidden layer, nor by the eraser or a pan', async () => {
    const hidden = setLayerVisible(docFromNote(note([hline(1000, 3000, 3000)])), 0, false);
    const c = await render({ doc: hidden });
    await c.stroke(0);
    expect(c.oninprogress).not.toHaveBeenCalled();
    cleanup?.();
    const eraser = await render({ tools: { mode: 'eraser' } });
    await eraser.stroke(0);
    expect(eraser.oninprogress).not.toHaveBeenCalled();
    cleanup?.();
    const pan = await render({ tools: { mode: 'pan' } });
    await pan.stroke(0);
    expect(pan.oninprogress).not.toHaveBeenCalled();
  });

  it('is dropped when the canvas goes in the middle of it (another note opens)', async () => {
    const c = await render();
    c.fire('pointerdown', c.client(8000, 8000), { t: 0 });
    c.fire('pointermove', c.client(9000, 8000), { t: 10 });
    cleanup?.();
    cleanup = null;
    expect(c.log).toEqual(['begin', 'grow', 'end:false']);
  });
});

describe('what the canvas shows, and the projected area', () => {
  it('tells what it shows of the page when the page is fitted, then at each zoom', async () => {
    const c = await render();
    const shown = () => visibleRect({ scale: c.view().scale, x: c.view().x, y: c.view().y }, { width: 1000, height: 700 });
    expect(c.onvisible).toHaveBeenCalled();
    expect(c.onvisible.mock.calls.at(-1)![0]).toEqual(shown());
    const calls = c.onvisible.mock.calls.length;
    c.target.querySelector<HTMLButtonElement>(`.zoom button[aria-label="Zoom avant"], .zoom button:nth-of-type(2)`)!.click();
    await tick();
    expect(c.onvisible.mock.calls.length).toBeGreaterThan(calls);
    const after = c.onvisible.mock.calls.at(-1)![0] as Rect;
    expect(after).toEqual(shown());
    expect(after.width).toBeLessThan((c.onvisible.mock.calls[calls - 1][0] as Rect).width);
  });

  it('draws the projected area as a frame that lets the pen through, and nothing without one', async () => {
    const c = await render({ projected: { x: 2000, y: 1500, width: 12_000, height: 6750 } });
    const frame = c.svg.querySelector<SVGRectElement>('[data-slot="projected-area"]')!;
    expect(frame).toBeTruthy();
    expect(frame.tagName.toLowerCase()).toBe('rect');
    expect([frame.getAttribute('x'), frame.getAttribute('y'), frame.getAttribute('width'), frame.getAttribute('height')]).toEqual([
      '2000',
      '1500',
      '12000',
      '6750',
    ]);
    expect(frame.getAttribute('pointer-events')).toBe('none');
    expect(frame.classList.contains('projected')).toBe(true);
    // The pen still draws through it.
    await c.stroke(0);
    expect(c.onchange).toHaveBeenCalledTimes(1);
    cleanup?.();
    const none = await render();
    expect(none.svg.querySelector('[data-slot="projected-area"]')).toBeNull();
  });
});
