<script lang="ts">
  /**
   * The live page on a `<canvas class="live-canvas">` (docs/design/tablet-live-classroom.md,
   * "Interface"). `preview`: the whole page with the projected area drawn on it; a drag moves
   * the area and the wheel zooms it (control window). `presentation`: the projected area fills
   * the canvas (presentation window).
   *
   * Drawing is `lib/live-render.ts`; this component sizes the canvas (CSS pixels ×
   * `devicePixelRatio`), loads the paper (`render_background`), and paints on the next
   * animation frame after a change, then records the latency of the points it painted: the
   * interface part when the drawing ends, the end-to-end figure at the start of the next frame
   * (`lib/latency.ts`).
   * Attributes for tests: `data-strokes` (strokes of the page, the one in progress included),
   * `data-page` (`n/N`).
   *
   * Pen and touch (docs/design/tablet-live-classroom.md, "M8: pen and touch"). `preview`: a
   * finger drags the projected area as the mouse does, two fingers pinch it (zoom and move), a
   * double tap shows the whole page. `presentation`: the pen, a finger or the mouse write
   * temporary marks over the page (the back of the pen erases them; they are not saved and go
   * with the page), two fingers pinch the projected area. A finger is ignored while a pen is in
   * contact or hovering (palm rejection). `onmarkschange` reports every change of the marks, the
   * one in progress included, for the window to pass them on to the viewers of the share.
   */
  import { api } from '../../lib/api';
  import type { Vec } from '../../lib/editor/geometry';
  import { PenPresence, TouchGestures, inkPressure, pointerKind, type PointerKind, type TouchAction } from '../../lib/gestures';
  import type { Ink, InkPoint } from '../../lib/ink';
  import { wallClock, type LatencyRecorder } from '../../lib/latency';
  import type { LiveViewState } from '../../lib/live';
  import {
    LiveRenderer,
    backgroundPixels,
    buildScene,
    effectiveBackground,
    isDarkPaper,
    sceneTransform,
    type Ctx2D,
    type Surface,
  } from '../../lib/live-render';
  import type { LiveState } from '../../lib/live-state.svelte';
  import { MARK_ERASER_PX, eraseMarks, markStyle, newMark } from '../../lib/live-marks';
  import {
    DEFAULT_ASPECT,
    moveViewTo,
    pinchPresentationView,
    pinchPreviewView,
    projectedArea,
    screenToPage,
    wholePage,
    zoomView,
    type Size,
  } from '../../lib/live-view';
  import type { Stroke } from '../../lib/note';

  let {
    live,
    ink,
    mode,
    label,
    aspect = DEFAULT_ASPECT,
    latency = null,
    onview,
    onmarks,
    onmarkschange,
  }: {
    live: LiveState;
    ink: Ink | null;
    mode: 'preview' | 'presentation';
    /** Accessible name of the canvas. */
    label: string;
    /** Preview: shape (width / height) of the projected area. */
    aspect?: number;
    /** Records the latency of the points painted. */
    latency?: LatencyRecorder | null;
    /** The user moved or zoomed the projected area (preview: drag, wheel, pinch; presentation: pinch). */
    onview?: (view: LiveViewState) => void;
    /** Presentation: the number of marks written over the page changed. */
    onmarks?: (count: number) => void;
    /** Presentation: the marks changed, or the mark being written grew (its points change in place). */
    onmarkschange?: (marks: readonly Stroke[], current: Stroke | null) => void;
  } = $props();

  /** jsdom (tests) has no layout: a nominal size. */
  const NOMINAL: Record<'preview' | 'presentation', Size> = {
    preview: { width: 800, height: 560 },
    presentation: { width: 1280, height: 720 },
  };

  let box = $state<HTMLDivElement>();
  let canvas = $state<HTMLCanvasElement>();
  let size = $state.raw<Size>(nominalSize());
  let pixelRatio = $state(currentPixelRatio());
  let dragging = $state(false);
  /** The rendered paper once loaded, and what it was rendered for. */
  let backgroundImage: HTMLImageElement | null = null;
  let backgroundFor = '';
  /** Bumped by each paper request: only the latest one is shown. */
  let backgroundRequest = 0;
  let context: Ctx2D | null = null;
  let scheduled = false;
  let destroyed = false;
  let drag: { id: number; offset: Vec } | null = null;
  /** Marks written over the presentation (page units), the one in progress, and the gesture writing or erasing. */
  let marks: readonly Stroke[] = [];
  let markCurrent: Stroke | null = null;
  let markDrag: { id: number; kind: 'write' | 'erase'; t0: number } | null = null;
  const presence = new PenPresence();
  const fingers = new TouchGestures();
  /** Bumped when the window is hidden or shown: frames awaited across that say nothing. */
  let visibilityEpoch = 0;

  const renderer = new LiveRenderer(createSurface);

  function nominalSize(): Size {
    return NOMINAL[mode];
  }

  function currentPixelRatio(): number {
    return typeof window !== 'undefined' && window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  }

  function createSurface(): Surface | null {
    if (typeof document === 'undefined') return null;
    const offscreen = document.createElement('canvas');
    const ctx = offscreen.getContext('2d');
    if (!ctx) return null;
    return {
      image: offscreen,
      ctx,
      resize(width, height) {
        offscreen.width = width;
        offscreen.height = height;
      },
    };
  }

  function schedule() {
    if (scheduled) return;
    scheduled = true;
    if (typeof requestAnimationFrame === 'undefined') {
      paint();
      return;
    }
    requestAnimationFrame(paint);
  }

  /**
   * One frame: draw, take the end of the drawing for the points it shows (interface part), and
   * count them as displayed when the next frame begins (end to end).
   */
  function paint() {
    scheduled = false;
    const el = canvas;
    if (!el || destroyed) return;
    context ??= el.getContext('2d');
    if (!context) {
      live.takeUnpainted();
      return;
    }
    const width = Math.max(1, Math.round(size.width * pixelRatio));
    const height = Math.max(1, Math.round(size.height * pixelRatio));
    if (el.width !== width) el.width = width;
    if (el.height !== height) el.height = height;
    renderer.paint(context, buildScene(live, { mode, viewport: size, pixelRatio, aspect, backgroundImage, marks, markCurrent }));
    const drawEnd = wallClock();
    const painted = live.takeUnpainted();
    // Points drawn without their outline (geometry still loading) or in a hidden window say
    // nothing about the latency.
    if (!latency || !ink || document.hidden || painted.length === 0) return;
    const recorder = latency;
    recorder.painted(painted, drawEnd);
    afterNextFrame(() => recorder.presented(painted, wallClock()));
  }

  /**
   * Runs `then` at the start of the next animation frame, the closest the page gets to the
   * moment the frame just drawn reaches the screen. Dropped if the window was hidden meanwhile.
   */
  function afterNextFrame(then: () => void) {
    if (typeof requestAnimationFrame === 'undefined') {
      then();
      return;
    }
    const epoch = visibilityEpoch;
    requestAnimationFrame(() => {
      if (!destroyed && epoch === visibilityEpoch && !document.hidden) then();
    });
  }

  // Every change of the session asks for a frame.
  $effect(() => {
    const stop = live.subscribe(schedule);
    schedule();
    return () => {
      stop();
      destroyed = true;
    };
  });

  // Geometry module, size, shape of the projected area: draw again.
  $effect(() => {
    renderer.setInk(ink);
    void size;
    void pixelRatio;
    void aspect;
    schedule();
  });

  // Follow the size of the box and the screen's pixel density.
  $effect(() => {
    const el = box;
    if (!el) return;
    const measure = () => {
      const width = el.clientWidth;
      const height = el.clientHeight;
      if (width > 0 && height > 0 && (width !== size.width || height !== size.height)) size = { width, height };
      const ratio = currentPixelRatio();
      if (ratio !== pixelRatio) pixelRatio = ratio;
    };
    measure();
    window.addEventListener('resize', measure);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(el);
    return () => {
      window.removeEventListener('resize', measure);
      observer?.disconnect();
    };
  });

  // Back from a hidden window: the points received meanwhile are not latency samples.
  $effect(() => {
    const onvisibility = () => {
      visibilityEpoch += 1;
      if (!document.hidden) {
        live.takeUnpainted();
        schedule();
      }
    };
    document.addEventListener('visibilitychange', onvisibility);
    return () => document.removeEventListener('visibilitychange', onvisibility);
  });

  // The paper, rendered by Rust at the width the zoom needs (re-rendered when it doubles).
  $effect(() => {
    const page = live.pageSize;
    const background = effectiveBackground(live.background, live.view.board);
    const transform = sceneTransform({ pageSize: page, view: live.view }, { mode, viewport: size });
    const widthPx = backgroundPixels({ page, transform, pixelRatio });
    const paper = JSON.stringify([page, background]);
    const wanted = `${paper}@${widthPx}`;
    if (wanted === backgroundFor) return;
    // Another paper: the old image must not show meanwhile (the paper colour does).
    if (!backgroundFor.startsWith(paper + '@')) {
      backgroundImage = null;
      schedule();
    }
    backgroundFor = wanted;
    const request = ++backgroundRequest;
    const current = () => !destroyed && request === backgroundRequest;
    api
      .renderBackground(page, background, widthPx)
      .then((url) => {
        if (!current() || typeof Image === 'undefined') return;
        const image = new Image();
        image.onload = () => {
          if (!current()) return;
          backgroundImage = image;
          schedule();
        };
        image.src = url;
      })
      .catch(() => {
        // The paper colour stays; writing still shows.
      });
  });

  // Wheel: zoom the projected area around the pointer (a non-passive listener: no page scroll).
  $effect(() => {
    const el = canvas;
    if (!el || mode !== 'preview' || !onview) return;
    const onwheel = (e: WheelEvent) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
      const factor = Math.exp(-e.deltaY * unit * 0.0015);
      onview?.(zoomView(live.view, factor, live.pageSize, aspect, pagePoint(e)));
    };
    el.addEventListener('wheel', onwheel, { passive: false });
    return () => el.removeEventListener('wheel', onwheel);
  });

  /** The pointer in CSS pixels relative to the canvas. */
  function localPoint(e: MouseEvent): Vec {
    const r = canvas?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
  }

  function pagePoint(e: MouseEvent): Vec {
    return screenToPage(sceneTransform(live, { mode, viewport: size }), localPoint(e));
  }

  function pointerId(e: PointerEvent): number {
    return typeof e.pointerId === 'number' ? e.pointerId : 1;
  }

  function capture(id: number) {
    try {
      canvas?.setPointerCapture?.(id);
    } catch {
      // Synthetic events cannot be captured; moves still reach the canvas.
    }
  }

  function setMarks(next: readonly Stroke[]) {
    if (next === marks) return;
    marks = next;
    onmarks?.(next.length);
    onmarkschange?.(marks, markCurrent);
    schedule();
  }

  /** Removes every mark (and the one being written). */
  export function clearMarks() {
    markCurrent = null;
    markDrag = null;
    setMarks([]);
    onmarkschange?.(marks, null);
    schedule();
  }

  // The marks belong to a page in the way it is held: another page, or the same page turned
  // (the quick switch of the orientation, which keeps the index but swaps or flips the page),
  // removes them.
  $effect(() => {
    void live.pageIndex;
    void live.orientation;
    void live.pageSize.width;
    void live.pageSize.height;
    if (marks.length > 0 || markCurrent) clearMarks();
  });

  // The control window's pinch and double tap; the presentation's pinch.
  function applyTouch(action: TouchAction) {
    if (!onview) return;
    if (action.kind === 'pinch') {
      if (mode === 'presentation') {
        onview(pinchPresentationView(live.view, live.pageSize, size, action.from, action.to, action.factor));
      } else {
        const transform = sceneTransform(live, { mode, viewport: size });
        onview(
          pinchPreviewView(live.view, live.pageSize, aspect, screenToPage(transform, action.from), screenToPage(transform, action.to), action.factor),
        );
      }
    } else if (action.kind === 'doubleTap' && mode === 'preview') {
      onview(wholePage(live.view, live.pageSize));
    }
  }

  const touchOptions = () => ({ mode: mode === 'presentation' ? ('draw' as const) : ('navigate' as const) });

  function sample(e: PointerEvent, kind: PointerKind, p: Vec, t0: number): InkPoint {
    return { x: p.x, y: p.y, pressure: inkPressure(kind, e.pressure), t: Math.max(0, Math.round(e.timeStamp - t0)) };
  }

  /** Starts a mark with the pen, a finger or the mouse; the back of the pen erases marks. */
  function startMark(e: PointerEvent, kind: PointerKind, id: number) {
    if (kind === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    capture(id);
    const p = pagePoint(e);
    const { scale } = sceneTransform(live, { mode, viewport: size });
    if (kind === 'eraser') {
      markDrag = { id, kind: 'erase', t0: e.timeStamp };
      setMarks(eraseMarks(marks, [p], MARK_ERASER_PX / scale));
      return;
    }
    const dark = isDarkPaper(effectiveBackground(live.background, live.view.board).paper);
    markDrag = { id, kind: 'write', t0: e.timeStamp };
    markCurrent = newMark(markStyle(dark), sample(e, kind, p, e.timeStamp), scale);
    onmarkschange?.(marks, markCurrent);
    schedule();
  }

  /** Preview: grabs the projected area (or, outside it, centres it there first). Presentation: starts a mark. */
  function onpointerdown(e: PointerEvent) {
    const kind = pointerKind(e);
    const id = pointerId(e);
    if (kind === 'pen' || kind === 'eraser') {
      presence.seen(id, e.timeStamp, 'down');
      fingers.reset();
    } else if (kind === 'touch') {
      if (presence.blocksTouch(e.timeStamp)) return;
      const second = fingers.down(id, localPoint(e), e.timeStamp);
      if (second) {
        // Two fingers: whatever the first one began is dropped, the pinch takes over.
        e.preventDefault();
        const dropped = markCurrent !== null;
        markCurrent = null;
        markDrag = null;
        drag = null;
        dragging = false;
        if (dropped) onmarkschange?.(marks, null);
        schedule();
        return;
      }
      if (fingers.count > 1 || !fingers.has(id)) return;
    }
    if (mode === 'presentation') {
      startMark(e, kind, id);
      return;
    }
    if (!onview || e.button !== 0 || kind === 'eraser') return;
    e.preventDefault();
    const p = pagePoint(e);
    const area = projectedArea(live.pageSize, live.view, aspect);
    let centre = { x: area.x + area.width / 2, y: area.y + area.height / 2 };
    const inside = p.x >= area.x && p.x <= area.x + area.width && p.y >= area.y && p.y <= area.y + area.height;
    if (!inside) {
      const view = moveViewTo(live.view, p, live.pageSize, aspect);
      onview(view);
      centre = { x: view.x, y: view.y };
    }
    drag = { id, offset: { x: centre.x - p.x, y: centre.y - p.y } };
    dragging = true;
    capture(id);
  }

  function onpointermove(e: PointerEvent) {
    const kind = pointerKind(e);
    const id = pointerId(e);
    if (kind === 'pen' || kind === 'eraser') {
      presence.seen(id, e.timeStamp, ((e.buttons ?? 0) & 33) !== 0 ? 'down' : 'hover');
    } else if (kind === 'touch') {
      if (!fingers.has(id)) return;
      const action = fingers.move(id, localPoint(e), touchOptions());
      if (action) applyTouch(action);
      if (fingers.count > 1) return;
    }
    if (mode === 'presentation') {
      if (!markDrag || markDrag.id !== id) return;
      const { scale } = sceneTransform(live, { mode, viewport: size });
      if (markDrag.kind === 'erase') {
        setMarks(eraseMarks(marks, [pagePoint(e)], MARK_ERASER_PX / scale));
        return;
      }
      const current = markCurrent;
      if (!current) return;
      const events = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : [];
      for (const ev of events.length > 0 ? events : [e]) {
        const q = pagePoint(ev);
        const last = current.points[current.points.length - 1];
        if (Math.hypot(q.x - last.x, q.y - last.y) * scale < 0.5) continue;
        current.points.push(sample(ev, kind, q, markDrag.t0));
      }
      onmarkschange?.(marks, current);
      schedule();
      return;
    }
    if (!drag || !onview || id !== drag.id) return;
    const p = pagePoint(e);
    onview(moveViewTo(live.view, { x: p.x + drag.offset.x, y: p.y + drag.offset.y }, live.pageSize, aspect));
  }

  function release(e: PointerEvent, cancelled = false) {
    const kind = pointerKind(e);
    const id = pointerId(e);
    if (kind === 'pen' || kind === 'eraser') presence.lifted(id, e.timeStamp);
    else if (kind === 'touch') {
      let action: TouchAction | null = null;
      if (cancelled) fingers.cancel(id);
      else action = fingers.up(id, e.timeStamp, touchOptions());
      if (action) applyTouch(action);
    }
    if (markDrag && markDrag.id === id) {
      const current = markCurrent;
      markDrag = null;
      markCurrent = null;
      if (current && !cancelled) setMarks([...marks, current]);
      else if (current) onmarkschange?.(marks, null);
      schedule();
    }
    if (!drag || id !== drag.id) return;
    drag = null;
    dragging = false;
  }

  function onpointerleave(e: PointerEvent) {
    const kind = pointerKind(e);
    if (kind === 'pen' || kind === 'eraser') presence.gone(pointerId(e));
  }
</script>

<div class="live-canvas-box" class:presentation={mode === 'presentation'} role="img" aria-label={label} bind:this={box}>
  <canvas
    bind:this={canvas}
    class="live-canvas"
    class:interactive={mode === 'preview' && !!onview}
    class:dragging
    class:zoomed={live.view.zoom > 1.001}
    class:writable={mode === 'presentation'}
    data-strokes={live.strokeCount}
    data-page="{live.pageIndex + 1}/{live.pageCount}"
    {onpointerdown}
    {onpointermove}
    onpointerup={(e) => release(e)}
    onpointercancel={(e) => release(e, true)}
    onlostpointercapture={(e) => release(e, true)}
    {onpointerleave}
  ></canvas>
</div>

<style>
  .live-canvas-box {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .live-canvas-box.presentation {
    position: fixed;
    inset: 0;
    background: #000;
  }

  .live-canvas {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
    user-select: none;
  }

  .live-canvas.writable {
    cursor: crosshair;
  }

  .live-canvas.interactive {
    cursor: crosshair;
  }

  .live-canvas.interactive.zoomed {
    cursor: grab;
  }

  .live-canvas.interactive.dragging {
    cursor: grabbing;
  }
</style>
