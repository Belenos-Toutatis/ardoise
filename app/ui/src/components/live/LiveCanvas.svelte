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
   */
  import { api } from '../../lib/api';
  import type { Vec } from '../../lib/editor/geometry';
  import type { Ink } from '../../lib/ink';
  import { wallClock, type LatencyRecorder } from '../../lib/latency';
  import type { LiveViewState } from '../../lib/live';
  import {
    LiveRenderer,
    backgroundPixels,
    buildScene,
    effectiveBackground,
    sceneTransform,
    type Ctx2D,
    type Surface,
  } from '../../lib/live-render';
  import type { LiveState } from '../../lib/live-state.svelte';
  import { DEFAULT_ASPECT, moveViewTo, projectedArea, screenToPage, zoomView, type Size } from '../../lib/live-view';

  let {
    live,
    ink,
    mode,
    label,
    aspect = DEFAULT_ASPECT,
    latency = null,
    onview,
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
    /** Preview: the user moved or zoomed the projected area. */
    onview?: (view: LiveViewState) => void;
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
    renderer.paint(context, buildScene(live, { mode, viewport: size, pixelRatio, aspect, backgroundImage }));
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

  function pagePoint(e: MouseEvent): Vec {
    const r = canvas?.getBoundingClientRect();
    const local = { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
    return screenToPage(sceneTransform(live, { mode, viewport: size }), local);
  }

  function pointerId(e: PointerEvent): number {
    return typeof e.pointerId === 'number' ? e.pointerId : 1;
  }

  /** Grabs the projected area (or, outside it, centres it there first). */
  function onpointerdown(e: PointerEvent) {
    if (mode !== 'preview' || !onview || e.button !== 0) return;
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
    drag = { id: pointerId(e), offset: { x: centre.x - p.x, y: centre.y - p.y } };
    dragging = true;
    try {
      canvas?.setPointerCapture?.(drag.id);
    } catch {
      // Synthetic events cannot be captured; moves still reach the canvas.
    }
  }

  function onpointermove(e: PointerEvent) {
    if (!drag || !onview || pointerId(e) !== drag.id) return;
    const p = pagePoint(e);
    onview(moveViewTo(live.view, { x: p.x + drag.offset.x, y: p.y + drag.offset.y }, live.pageSize, aspect));
  }

  function release(e: PointerEvent) {
    if (!drag || pointerId(e) !== drag.id) return;
    drag = null;
    dragging = false;
  }
</script>

<div class="live-canvas-box" class:presentation={mode === 'presentation'} role="img" aria-label={label} bind:this={box}>
  <canvas
    bind:this={canvas}
    class="live-canvas"
    class:interactive={mode === 'preview' && !!onview}
    class:dragging
    class:zoomed={live.view.zoom > 1.001}
    data-strokes={live.strokeCount}
    data-page="{live.pageIndex + 1}/{live.pageCount}"
    {onpointerdown}
    {onpointermove}
    onpointerup={release}
    onpointercancel={release}
    onlostpointercapture={release}
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
