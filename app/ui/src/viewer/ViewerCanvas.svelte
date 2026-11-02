<script lang="ts">
  /**
   * The shared page on a `<canvas>`, full window on a black surround, as the presentation window
   * shows it (`components/live/LiveCanvas.svelte`, mode `presentation`: zoom, pan, blackboard,
   * laser dot, and the marks written over the presentation, which the hub sends), without what
   * the viewer does not do: no pointer, no latency, no call to Rust. Drawing is `lib/live-render.ts`; this component sizes the canvas (CSS pixels ×
   * `devicePixelRatio`), shows the paper image of the message instead of `render_background`
   * (`background`: PNG data URL, null for plain paper, which `LiveRenderer` fills with the paper
   * colour), and paints on the next animation frame after a change.
   * Attributes for tests: `data-strokes` (strokes of the page, the one in progress included),
   * `data-page` (`n/N`), `data-shown` (`live` or `note`).
   */
  import { onMount } from 'svelte';
  import type { Ink } from '../lib/ink';
  import { LiveRenderer, buildScene, type Ctx2D, type Surface } from '../lib/live-render';
  import type { LiveState } from '../lib/live-state.svelte';
  import { DEFAULT_ASPECT, type Size } from '../lib/live-view';

  let {
    live,
    ink,
    background,
    shown,
    label,
  }: {
    live: LiveState;
    ink: Ink | null;
    /** The paper as a PNG data URL, or null (the paper colour only). */
    background: string | null;
    shown: 'live' | 'note';
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
    renderer.paint(
      context,
      buildScene(live, {
        mode: 'presentation',
        viewport: size,
        pixelRatio,
        aspect: DEFAULT_ASPECT,
        backgroundImage,
        marks: live.marks,
      }),
    );
    // Nobody measures the latency here: the points painted are not kept.
    live.takeUnpainted();
  }

  onMount(() => () => {
    destroyed = true;
  });

  // Every change of the sheet asks for a frame; another sheet (a new `LiveState`) starts again.
  $effect(() => {
    const stop = live.subscribe(schedule);
    schedule();
    return stop;
  });

  // Geometry module and size: draw again.
  $effect(() => {
    renderer.setInk(ink);
    void size;
    void pixelRatio;
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

  // The paper image of the message (a data URL): shown once decoded; without one the paper
  // colour shows.
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
  }
</style>
