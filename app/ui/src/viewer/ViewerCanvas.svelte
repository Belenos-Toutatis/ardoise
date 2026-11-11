<script lang="ts">
  /**
   * The shared page on a `<canvas>`, full window on a black surround, as the presentation window
   * shows it (`components/live/LiveCanvas.svelte`, mode `presentation`: zoom, pan, blackboard,
   * laser dot, and the marks written over the presentation, which the hub sends), without what
   * the viewer does not do: no pointer, no latency, no call to Rust. Drawing is `lib/live-render.ts`; this component sizes the canvas (CSS pixels ×
   * `devicePixelRatio`), shows the paper image it is given instead of `render_background`
   * (`background`: a URL an image can load, null for plain paper, which `LiveRenderer` fills with the
   * paper colour), and paints on the next animation frame after a change.
   * The view drawn is the PC's or this screen's own (`ViewerView`: `view.current`). The mouse wheel
   * zooms around the pointer (as in the editor), a drag with the mouse or the pen and one finger
   * move the page, two fingers pinch it (`TouchGestures`); each takes the own view.
   * Attributes for tests: `data-strokes` (strokes of the page, the one in progress included),
   * `data-page` (`n/N`), `data-shown` (`live` or `note`), `data-fit` (`follow`, `page`, `width`
   * or `free`), `data-zoom` and `data-scale` (CSS pixels per page unit) of the view drawn.
   */
  import { onMount } from 'svelte';
  import type { Vec } from '../lib/editor/geometry';
  import { TouchGestures, pointerKind, type TouchAction } from '../lib/gestures';
  import type { Ink } from '../lib/ink';
  import { LiveRenderer, buildScene, type Ctx2D, type LiveSource, type Surface } from '../lib/live-render';
  import type { LiveState } from '../lib/live-state.svelte';
  import { DEFAULT_ASPECT, type Size } from '../lib/live-view';
  import { exposeRepaints, noteLiveRedraw } from '../lib/perf';
  import type { ViewerView } from '../lib/viewer-view.svelte';

  let {
    live,
    ink,
    background,
    shown,
    view,
    label,
  }: {
    live: LiveState;
    ink: Ink | null;
    /** The paper as a URL an image can load (a note's: data, a live page's: blob), or null (the paper colour only). */
    background: string | null;
    shown: 'live' | 'note';
    /** The view drawn (the PC's, or this screen's own) and what changes it. */
    view: ViewerView;
    /** Accessible name of the canvas. */
    label: string;
  } = $props();

  /** jsdom (tests) has no layout: a nominal size. */
  const NOMINAL: Size = { width: 1280, height: 720 };

  let box = $state<HTMLDivElement>();
  let canvas = $state<HTMLCanvasElement>();
  let size = $state.raw<Size>(NOMINAL);
  let pixelRatio = $state(currentPixelRatio());
  /** The paper image once loaded. */
  let backgroundImage: HTMLImageElement | null = null;
  /** Bumped by each paper change: only the latest image is shown. */
  let backgroundRequest = 0;
  let context: Ctx2D | null = null;
  let scheduled = false;
  let destroyed = false;
  let dragging = $state(false);
  /** The mouse or pen drag moving the page. */
  let drag: { id: number; last: Vec } | null = null;
  const fingers = new TouchGestures();

  const renderer = new LiveRenderer(createSurface);

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

  function paint() {
    scheduled = false;
    const el = canvas;
    if (!el || destroyed) return;
    context ??= el.getContext('2d');
    if (!context) return;
    const width = Math.max(1, Math.round(size.width * pixelRatio));
    const height = Math.max(1, Math.round(size.height * pixelRatio));
    if (el.width !== width) el.width = width;
    if (el.height !== height) el.height = height;
    const source: LiveSource = {
      pageSize: live.pageSize,
      background: live.background,
      layers: live.layers,
      strokes: live.strokes,
      generation: live.generation,
      edits: live.edits,
      current: live.current,
      computerCurrent: live.computerCurrent,
      hover: live.hover,
      view: view.current,
    };
    const startedAt = performance.now();
    const report = renderer.paint(
      context,
      buildScene(source, {
        mode: 'presentation',
        viewport: size,
        pixelRatio,
        aspect: DEFAULT_ASPECT,
        backgroundImage,
        marks: live.marks,
      }),
    );
    noteLiveRedraw(el, report, startedAt);
    // Nobody measures the latency here: the points painted are not kept.
    live.takeUnpainted();
    live.takeUnpaintedComputer();
  }

  onMount(() => () => {
    destroyed = true;
  });

  $effect(() => {
    if (canvas) exposeRepaints(canvas, paint, () => renderer.invalidate());
  });

  // Every change of the sheet asks for a frame; another sheet (a new `LiveState`) starts again.
  $effect(() => {
    const stop = live.subscribe(schedule);
    schedule();
    return stop;
  });

  // Geometry module, size, view (the PC's or the own one): draw again.
  $effect(() => {
    renderer.setInk(ink);
    void size;
    void pixelRatio;
    void view.current;
    schedule();
  });

  // Follow the size of the box and the screen's pixel density.
  $effect(() => {
    const el = box;
    if (!el) return;
    const measure = () => {
      const width = el.clientWidth;
      const height = el.clientHeight;
      if (width > 0 && height > 0 && (width !== size.width || height !== size.height)) {
        size = { width, height };
        view.setSize(size);
      }
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

  // The wheel zooms around the pointer, as in the editor (a non-passive listener: no page scroll
  // nor browser zoom).
  $effect(() => {
    const el = canvas;
    if (!el) return;
    const onwheel = (e: WheelEvent) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
      view.zoomAt(Math.exp(-e.deltaY * unit * 0.0015), localPoint(e));
    };
    el.addEventListener('wheel', onwheel, { passive: false });
    return () => el.removeEventListener('wheel', onwheel);
  });

  /** The pointer in CSS pixels relative to the canvas. */
  function localPoint(e: MouseEvent): Vec {
    const r = canvas?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
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

  function applyTouch(action: TouchAction) {
    if (action.kind === 'pan') view.drag(action.dx, action.dy);
    else if (action.kind === 'pinch') view.pinch(action.from, action.to, action.factor);
  }

  /** The mouse and the pen grab the page; a finger is followed by `fingers` (pan, pinch). */
  function onpointerdown(e: PointerEvent) {
    const kind = pointerKind(e);
    const id = pointerId(e);
    if (kind === 'eraser') return;
    if (kind === 'touch') {
      e.preventDefault();
      capture(id);
      if (fingers.down(id, localPoint(e), e.timeStamp)) drag = null;
      return;
    }
    if (e.button !== 0) return;
    e.preventDefault();
    capture(id);
    drag = { id, last: localPoint(e) };
    dragging = true;
  }

  function onpointermove(e: PointerEvent) {
    const id = pointerId(e);
    if (pointerKind(e) === 'touch') {
      if (!fingers.has(id)) return;
      const action = fingers.move(id, localPoint(e), { mode: 'navigate' });
      if (action) applyTouch(action);
      return;
    }
    if (!drag || drag.id !== id) return;
    const p = localPoint(e);
    view.drag(p.x - drag.last.x, p.y - drag.last.y);
    drag = { id, last: p };
  }

  function release(e: PointerEvent, cancelled = false) {
    const id = pointerId(e);
    if (pointerKind(e) === 'touch') {
      if (cancelled) fingers.cancel(id);
      else fingers.up(id, e.timeStamp, { mode: 'navigate' });
      return;
    }
    if (!drag || drag.id !== id) return;
    drag = null;
    dragging = false;
  }

  // The paper image (a data URL from the message, or a blob URL of what was fetched): shown once
  // decoded; without one the paper colour shows.
  $effect(() => {
    const url = background;
    const request = ++backgroundRequest;
    if (backgroundImage) {
      backgroundImage = null;
      schedule();
    }
    if (!url || typeof Image === 'undefined') return;
    const image = new Image();
    image.onload = () => {
      if (destroyed || request !== backgroundRequest) return;
      backgroundImage = image;
      schedule();
    };
    image.src = url;
  });
</script>

<div class="viewer-canvas-box" role="img" aria-label={label} bind:this={box}>
  <canvas
    bind:this={canvas}
    class="viewer-canvas"
    data-strokes={live.strokeCount}
    data-page="{live.pageIndex + 1}/{live.pageCount}"
    data-shown={shown}
    data-fit={view.fit}
    data-zoom={view.current.zoom.toFixed(3)}
    data-scale={view.transform.scale.toPrecision(5)}
    class:grab={view.current.zoom > 1.001}
    class:dragging
    {onpointerdown}
    {onpointermove}
    onpointerup={(e) => release(e)}
    onpointercancel={(e) => release(e, true)}
    onlostpointercapture={(e) => release(e, true)}
  ></canvas>
</div>

<style>
  .viewer-canvas-box {
    position: fixed;
    inset: 0;
    overflow: hidden;
    background: #000;
  }

  .viewer-canvas {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
    touch-action: none;
  }

  .viewer-canvas.grab {
    cursor: grab;
  }

  .viewer-canvas.dragging {
    cursor: grabbing;
  }
</style>
