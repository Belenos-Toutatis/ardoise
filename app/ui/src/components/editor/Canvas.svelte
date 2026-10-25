<script lang="ts">
  /**
   * The page: an SVG whose `viewBox` is the visible part of the page in device units, the
   * rendered background as an `<image>`, then one non-zero filled path per visible stroke in
   * drawing order (outlines cached per stroke object). Handles drawing, erasing, lasso, moving
   * and resizing the selection, manual crop, zoom (wheel at the cursor) and pan (middle button,
   * Space + drag, or the pan tool). A drag shows transient state and reports one change at the end.
   */
  import { untrack } from 'svelte';
  import { api } from '../../lib/api';
  import {
    applyAffine,
    boundsOfPoints,
    containsPoint,
    intersection,
    rectFromCorners,
    scalingAround,
    svgMatrix,
    translation,
    type Affine,
    type Rect,
    type Vec,
  } from '../../lib/editor/geometry';
  import { drawingOrder, hitStroke, selectInLasso } from '../../lib/editor/hit';
  import { placedImage } from '../../lib/editor/image';
  import { MIN_CROP } from '../../lib/editor/ops';
  import type { SplitPreview } from '../../lib/editor/split';
  import {
    addStroke,
    canDraw,
    crop,
    erase,
    select,
    selectionBounds,
    transformSelection,
    type Doc,
    type ToolSettings,
  } from '../../lib/editor/state';
  import {
    backgroundWidth,
    fitView,
    screenToPage,
    visibleRect,
    zoomAt,
    type View,
    type Viewport,
  } from '../../lib/editor/view';
  import { t } from '../../lib/i18n.svelte';
  import type { Ink, InkPoint } from '../../lib/ink';
  import { cssColor, type Note, type Stroke } from '../../lib/note';

  let {
    doc,
    tools,
    ink,
    spacePan = false,
    onchange,
    onselect,
    oncropped,
    onblocked,
    split = null,
    onzone,
  }: {
    doc: Doc;
    tools: ToolSettings;
    ink: Ink;
    /** Space is held: a left drag pans. */
    spacePan?: boolean;
    /** An undoable change (one per gesture). */
    onchange: (next: Doc) => void;
    /** A selection change (not an undo step). */
    onselect: (next: Doc) => void;
    oncropped?: () => void;
    /** The pen was put down while the active layer is hidden: nothing is drawn. */
    onblocked?: () => void;
    /**
     * The split panel is open: the page is read-only, the strokes that would move to the new
     * note are highlighted (the others dimmed), and with `tracing` a drag traces the zone
     * (reported to `onzone`, drawn as `zone`).
     */
    split?: { preview: SplitPreview; tracing: boolean; zone: readonly (readonly [number, number])[] | null } | null;
    onzone?: (polygon: [number, number][]) => void;
  } = $props();

  /** Screen pixels per device unit at real size (96 dpi, 10 µm units). */
  const REAL_SIZE = 96 / 25.4 / 100;
  const HANDLE_PX = 10;

  /** What a lasso does to the selection: replace it, add to it (Shift), remove from it (Ctrl). */
  type Combine = 'replace' | 'add' | 'remove';

  type Drag =
    | { kind: 'pan'; id: number; start: Vec; view: View }
    | { kind: 'pen'; id: number; points: InkPoint[]; t0: number; base: number }
    | { kind: 'eraser'; id: number; last: Vec }
    | { kind: 'lasso'; id: number; points: Vec[]; combine: Combine }
    | { kind: 'zone'; id: number; points: Vec[] }
    | { kind: 'move'; id: number; start: Vec }
    | { kind: 'scale'; id: number; anchor: Vec; start: Vec }
    | { kind: 'crop'; id: number; start: Vec };

  let stage = $state<HTMLDivElement>();
  let svg = $state<SVGSVGElement>();
  let viewport = $state<Viewport>({ width: 1000, height: 700 });
  let view = $state.raw<View>({ scale: 0.03, x: 0, y: 0 });
  let drag: Drag | null = null;
  let dragKind = $state<Drag['kind'] | null>(null);
  let transient = $state.raw<Doc | null>(null);
  let dragAffine = $state.raw<Affine | null>(null);
  let lasso = $state.raw<Vec[] | null>(null);
  /** The lasso removes from the selection (drawn in the danger colour). */
  let lassoRemoves = $state(false);
  let cropRect = $state.raw<Rect | null>(null);
  let livePath = $state('');
  let hover = $state.raw<Vec | null>(null);
  let background = $state<string | null>(null);

  const shown = $derived(transient ?? doc);
  const page = $derived(shown.note.page);
  const pageKey = $derived(`${doc.note.page.width}x${doc.note.page.height}`);
  // The paper and the background image with the key of its pixels (undo can bring another back).
  const backgroundKey = $derived(JSON.stringify([doc.note.background, placedImage(doc) ?? null]));
  const pixelRatio = typeof window !== 'undefined' && window.devicePixelRatio ? window.devicePixelRatio : 1;
  const bgWidth = $derived(backgroundWidth(doc.note.page, view.scale, pixelRatio));
  const box = $derived(visibleRect(view, viewport));
  const zoomPercent = $derived(Math.round((view.scale / REAL_SIZE) * 100));

  // Outlines are cached per stroke object: strokes are immutable, an edit makes new ones.
  const outlines = new WeakMap<Stroke, string>();
  function outline(stroke: Stroke): string {
    let d = outlines.get(stroke);
    if (d === undefined) {
      d = ink.path(stroke);
      outlines.set(stroke, d);
    }
    return d;
  }

  /** Which note the stroke goes to while splitting; null when not splitting. */
  type Side = 'kept' | 'moved' | null;

  const items = $derived.by(() => {
    const note = shown.note;
    const selected = new Set(shown.selection);
    // Nothing to highlight yet (no zone traced): the page stays as it is.
    const preview = split && split.preview.movedCount > 0 ? split.preview : null;
    const out: { i: number; d: string; fill: string; highlighter: boolean; selected: boolean; side: Side }[] = [];
    for (const i of drawingOrder(note)) {
      const stroke = note.strokes[i];
      const base = {
        i,
        fill: cssColor(stroke.color),
        highlighter: stroke.tool === 'highlighter',
        selected: selected.has(i),
      };
      const part = preview?.cut.get(i);
      if (part) {
        // Cut at the instant: the two parts are drawn apart (not cached, a few strokes at most).
        out.push({ ...base, d: ink.path(part.kept), side: 'kept' }, { ...base, d: ink.path(part.moved), side: 'moved' });
      } else {
        out.push({ ...base, d: outline(stroke), side: !preview ? null : preview.moved.has(i) ? 'moved' : 'kept' });
      }
    }
    return out;
  });

  const selectionBox = $derived.by(() => {
    const bounds = selectionBounds(shown);
    if (!bounds || !dragAffine) return bounds;
    return rectFromCorners(
      applyAffine(dragAffine, { x: bounds.x, y: bounds.y }),
      applyAffine(dragAffine, { x: bounds.x + bounds.width, y: bounds.y + bounds.height }),
    );
  });

  function corners(r: Rect): Vec[] {
    return [
      { x: r.x, y: r.y },
      { x: r.x + r.width, y: r.y },
      { x: r.x + r.width, y: r.y + r.height },
      { x: r.x, y: r.y + r.height },
    ];
  }

  function measure() {
    const width = stage?.clientWidth ?? 0;
    const height = stage?.clientHeight ?? 0;
    // jsdom (tests) has no layout: keep a nominal size.
    if (width > 0 && height > 0) viewport = { width, height };
  }

  export function fit() {
    measure();
    view = fitView(doc.note.page, viewport);
  }

  /** True while a gesture (stroke, erasing, lasso, move, crop, pan) is under way. */
  export function dragging(): boolean {
    return drag !== null;
  }

  export function zoomBy(factor: number) {
    view = zoomAt(view, { x: viewport.width / 2, y: viewport.height / 2 }, factor);
  }

  // Fit the page when it first shows and whenever its size changes (rotation, crop).
  let fittedKey = '';
  $effect(() => {
    const key = pageKey;
    untrack(() => {
      if (key !== fittedKey) {
        fittedKey = key;
        fit();
      }
    });
  });

  // Follow the size of the stage.
  $effect(() => {
    const el = stage;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(el);
    return () => observer.disconnect();
  });

  // The background, re-rendered when the page, the paper, the background image or the zoom
  // bucket changes.
  $effect(() => {
    void pageKey;
    void backgroundKey;
    const width = bgWidth;
    const current = untrack(() => doc);
    const note = current.note;
    let cancelled = false;
    api
      .renderBackground(note.page, note.background, width, placedImage(current))
      .then((url) => {
        if (!cancelled) background = url;
      })
      .catch(() => {
        // The white sheet stays; drawing still works.
      });
    return () => {
      cancelled = true;
    };
  });

  // Wheel zoom at the cursor (a non-passive listener, so that the page does not scroll).
  $effect(() => {
    const el = svg;
    if (!el) return;
    const onwheel = (e: WheelEvent) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
      view = zoomAt(view, local(e), Math.exp(-e.deltaY * unit * 0.0015));
    };
    el.addEventListener('wheel', onwheel, { passive: false });
    return () => el.removeEventListener('wheel', onwheel);
  });

  function local(e: MouseEvent): Vec {
    const r = svg?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
  }

  function pagePoint(e: MouseEvent): Vec {
    return screenToPage(view, local(e));
  }

  function pointerId(e: PointerEvent): number {
    return typeof e.pointerId === 'number' ? e.pointerId : 1;
  }

  function lastTime(note: Note): number {
    let last = -1;
    for (const stroke of note.strokes) for (const p of stroke.points) if (p.t > last) last = p.t;
    return last;
  }

  function sample(e: PointerEvent, p: Vec, d: { t0: number; base: number }): InkPoint {
    const pressure = typeof e.pressure === 'number' && e.pressure > 0 ? e.pressure : 0.6;
    return { x: p.x, y: p.y, pressure, t: Math.max(0, Math.round(d.base + (e.timeStamp - d.t0))) };
  }

  let frame = 0;
  function showLive() {
    if (drag?.kind !== 'pen') return;
    livePath = ink.path({ tool: tools.tool, width: tools.width, points: drag.points });
  }
  function scheduleLive() {
    if (typeof requestAnimationFrame === 'undefined') return showLive();
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      showLive();
    });
  }

  function handleAt(screen: Vec): Vec | null {
    if (!selectionBox) return null;
    const cs = corners(selectionBox);
    for (let k = 0; k < 4; k++) {
      const c = cs[k];
      const sx = c.x * view.scale + view.x;
      const sy = c.y * view.scale + view.y;
      if (Math.abs(sx - screen.x) <= HANDLE_PX && Math.abs(sy - screen.y) <= HANDLE_PX) return cs[(k + 2) % 4];
    }
    return null;
  }

  function begin(next: Drag) {
    drag = next;
    dragKind = next.kind;
  }

  function onpointerdown(e: PointerEvent) {
    if (drag) return;
    const panRequested = e.button === 1 || (e.button === 0 && (spacePan || tools.mode === 'pan'));
    if (e.button !== 0 && !panRequested) return;
    e.preventDefault();
    // Splitting: the page is read-only, except for tracing the zone.
    if (split && !panRequested && !split.tracing) return;
    if (!split && !panRequested && tools.mode === 'pen' && !canDraw(doc)) {
      onblocked?.();
      return;
    }
    // A pending title or layer name edit is committed by leaving its field.
    if (document.activeElement instanceof HTMLInputElement) document.activeElement.blur();
    const id = pointerId(e);
    try {
      svg?.setPointerCapture?.(id);
    } catch {
      // Not every pointer can be captured (synthetic events); moves still arrive on the canvas.
    }
    if (panRequested) return begin({ kind: 'pan', id, start: local(e), view });
    const p = pagePoint(e);
    if (split) {
      begin({ kind: 'zone', id, points: [p] });
      lasso = [p];
      lassoRemoves = false;
      return;
    }
    switch (tools.mode) {
      case 'pen': {
        const d = { t0: e.timeStamp, base: lastTime(doc.note) + 500 };
        begin({ kind: 'pen', id, points: [sample(e, p, d)], ...d });
        showLive();
        break;
      }
      case 'eraser':
        transient = erase(doc, [p], tools.eraserRadius, tools.eraser);
        begin({ kind: 'eraser', id, last: p });
        break;
      case 'select': {
        // With Shift or Ctrl the gesture is always a lasso, even on the selection's frame.
        const combine: Combine = e.ctrlKey || e.metaKey ? 'remove' : e.shiftKey ? 'add' : 'replace';
        const anchor = combine === 'replace' ? handleAt(local(e)) : null;
        if (anchor) begin({ kind: 'scale', id, anchor, start: p });
        else if (combine === 'replace' && selectionBox && containsPoint(selectionBox, p)) begin({ kind: 'move', id, start: p });
        else {
          begin({ kind: 'lasso', id, points: [p], combine });
          lasso = [p];
          lassoRemoves = combine === 'remove';
        }
        break;
      }
      case 'crop':
        begin({ kind: 'crop', id, start: p });
        cropRect = rectFromCorners(p, p);
        break;
      default:
        break;
    }
  }

  function onpointermove(e: PointerEvent) {
    if (tools.mode === 'eraser') hover = pagePoint(e);
    if (!drag || pointerId(e) !== drag.id) return;
    switch (drag.kind) {
      case 'pan': {
        const s = local(e);
        view = { ...drag.view, x: drag.view.x + s.x - drag.start.x, y: drag.view.y + s.y - drag.start.y };
        break;
      }
      case 'pen': {
        const events = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : [];
        for (const ev of events.length > 0 ? events : [e]) {
          const p = pagePoint(ev);
          const last = drag.points[drag.points.length - 1];
          if (Math.hypot(p.x - last.x, p.y - last.y) * view.scale < 0.5) continue;
          drag.points.push(sample(ev, p, drag));
        }
        scheduleLive();
        break;
      }
      case 'eraser': {
        const p = pagePoint(e);
        transient = erase(transient ?? doc, [drag.last, p], tools.eraserRadius, tools.eraser);
        drag.last = p;
        break;
      }
      case 'lasso':
      case 'zone':
        drag.points.push(pagePoint(e));
        lasso = [...drag.points];
        break;
      case 'move': {
        const p = pagePoint(e);
        dragAffine = translation(p.x - drag.start.x, p.y - drag.start.y);
        break;
      }
      case 'scale': {
        const p = pagePoint(e);
        const ax = drag.start.x - drag.anchor.x;
        const ay = drag.start.y - drag.anchor.y;
        const length = ax * ax + ay * ay;
        const f = length > 0 ? ((p.x - drag.anchor.x) * ax + (p.y - drag.anchor.y) * ay) / length : 1;
        dragAffine = scalingAround(drag.anchor, Math.max(0.05, f));
        break;
      }
      case 'crop':
        cropRect = rectFromCorners(drag.start, pagePoint(e));
        break;
    }
  }

  function finish(e: PointerEvent, cancelled: boolean) {
    if (!drag || pointerId(e) !== drag.id) return;
    const d = drag;
    drag = null;
    dragKind = null;
    if (frame && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(frame);
    frame = 0;
    switch (d.kind) {
      case 'pen':
        if (!cancelled) {
          onchange(addStroke(doc, { tool: tools.tool, color: { ...tools.color }, width: tools.width, points: d.points }));
        }
        livePath = '';
        break;
      case 'eraser':
        if (!cancelled && transient && transient !== doc) onchange(transient);
        transient = null;
        break;
      case 'lasso': {
        lasso = null;
        if (cancelled) break;
        const bounds = boundsOfPoints(d.points);
        let picked: number[];
        if (!bounds || Math.max(bounds.width, bounds.height) * view.scale < 4) {
          const hit = hitStroke(doc.note, d.points[0], 6 / view.scale);
          picked = hit === null ? [] : [hit];
        } else {
          picked = selectInLasso(doc.note, d.points);
        }
        const next =
          d.combine === 'add'
            ? [...doc.selection, ...picked]
            : d.combine === 'remove'
              ? doc.selection.filter((i) => !picked.includes(i))
              : picked;
        onselect(select(doc, next));
        break;
      }
      case 'zone': {
        lasso = null;
        const bounds = boundsOfPoints(d.points);
        if (!cancelled && d.points.length >= 3 && bounds && Math.max(bounds.width, bounds.height) * view.scale >= 8) {
          onzone?.(d.points.map((p): [number, number] => [p.x, p.y]));
        }
        break;
      }
      case 'move':
      case 'scale':
        if (!cancelled && dragAffine) onchange(transformSelection(doc, dragAffine));
        dragAffine = null;
        break;
      case 'crop': {
        const rect = cropRect && intersection(cropRect, { x: 0, y: 0, width: page.width, height: page.height });
        cropRect = null;
        if (!cancelled && rect && rect.width >= MIN_CROP && rect.height >= MIN_CROP) {
          onchange(crop(doc, rect));
          oncropped?.();
        }
        break;
      }
      case 'pan':
        break;
    }
  }
</script>

<div class="stage" bind:this={stage}>
  <svg
    bind:this={svg}
    class="canvas mode-{split?.tracing ? 'zone' : tools.mode}"
    class:space={spacePan}
    class:dragging={dragKind === 'pan'}
    role="application"
    aria-label={t('editor.canvas')}
    viewBox="{box.x} {box.y} {box.width} {box.height}"
    preserveAspectRatio="none"
    data-page-width={page.width}
    data-page-height={page.height}
    data-view="{view.scale} {view.x} {view.y}"
    {onpointerdown}
    {onpointermove}
    onpointerup={(e) => finish(e, false)}
    onpointercancel={(e) => finish(e, true)}
    onlostpointercapture={(e) => finish(e, true)}
    onpointerleave={() => (hover = null)}
  >
    <rect class="sheet" x="0" y="0" width={page.width} height={page.height} />
    {#if background}
      <image href={background} x="0" y="0" width={page.width} height={page.height} preserveAspectRatio="none" />
    {/if}
    <g class="ink">
      {#each items as item}
        <path
          d={item.d}
          fill={item.fill}
          fill-rule="nonzero"
          data-stroke={item.i}
          class:highlighter={item.highlighter}
          class:split-kept={item.side === 'kept'}
          class:split-moved={item.side === 'moved'}
          transform={item.selected && dragAffine ? svgMatrix(dragAffine) : undefined}
        />
      {/each}
      {#if livePath}
        <path
          class="live"
          class:highlighter={tools.tool === 'highlighter'}
          d={livePath}
          fill={cssColor(tools.color)}
          fill-rule="nonzero"
        />
      {/if}
    </g>
    {#if selectionBox}
      <rect
        class="selection"
        x={selectionBox.x}
        y={selectionBox.y}
        width={selectionBox.width}
        height={selectionBox.height}
      />
      {#if tools.mode === 'select'}
        {#each corners(selectionBox) as c}
          <rect
            class="handle"
            x={c.x - HANDLE_PX / 2 / view.scale}
            y={c.y - HANDLE_PX / 2 / view.scale}
            width={HANDLE_PX / view.scale}
            height={HANDLE_PX / view.scale}
          />
        {/each}
      {/if}
    {/if}
    {#if lasso}
      <polyline class="lasso" class:removing={lassoRemoves} points={lasso.map((p) => `${p.x},${p.y}`).join(' ')} />
    {/if}
    {#if cropRect}
      <rect class="crop" x={cropRect.x} y={cropRect.y} width={cropRect.width} height={cropRect.height} />
    {/if}
    {#if split?.zone}
      <polygon class="split-zone" points={split.zone.map(([x, y]) => `${x},${y}`).join(' ')} />
    {/if}
    {#if hover && tools.mode === 'eraser' && !split}
      <circle class="eraser" cx={hover.x} cy={hover.y} r={tools.eraserRadius} />
    {/if}
  </svg>
  <div class="zoom" role="group" aria-label={t('editor.zoom.label')}>
    <button class="button ghost" type="button" title={t('editor.zoom.out')} aria-label={t('editor.zoom.out')} onclick={() => zoomBy(1 / 1.25)}>−</button>
    <span class="zoom-level" aria-live="polite">{t('editor.zoom.level', { percent: zoomPercent })}</span>
    <button class="button ghost" type="button" title={t('editor.zoom.in')} aria-label={t('editor.zoom.in')} onclick={() => zoomBy(1.25)}>+</button>
    <button class="button ghost" type="button" onclick={fit}>{t('editor.zoom.fit')}</button>
  </div>
</div>

<style>
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--surface-2);
  }

  .canvas {
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
    user-select: none;
  }

  .mode-pen,
  .mode-crop,
  .mode-zone {
    cursor: crosshair;
  }

  .mode-eraser {
    cursor: none;
  }

  .mode-pan,
  .space {
    cursor: grab;
  }

  .dragging {
    cursor: grabbing;
  }

  .sheet {
    fill: #fff;
    stroke: var(--border);
    stroke-width: 1px;
    vector-effect: non-scaling-stroke;
  }

  .highlighter {
    mix-blend-mode: multiply;
  }

  .selection,
  .crop {
    fill: rgb(47 111 223 / 6%);
    stroke: var(--accent);
    stroke-width: 1.5px;
    stroke-dasharray: 6 4;
    vector-effect: non-scaling-stroke;
  }

  .crop {
    fill: rgb(47 111 223 / 12%);
  }

  .split-kept {
    opacity: 0.3;
  }

  .split-moved {
    stroke: var(--accent);
    stroke-width: 3px;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .split-zone {
    fill: rgb(47 111 223 / 10%);
    stroke: var(--accent);
    stroke-width: 2px;
    vector-effect: non-scaling-stroke;
    pointer-events: none;
  }

  .handle {
    fill: var(--surface);
    stroke: var(--accent);
    stroke-width: 1.5px;
    vector-effect: non-scaling-stroke;
    cursor: nwse-resize;
  }

  .lasso {
    fill: rgb(47 111 223 / 8%);
    stroke: var(--accent);
    stroke-width: 1.5px;
    stroke-dasharray: 4 3;
    vector-effect: non-scaling-stroke;
  }

  .lasso.removing {
    fill: rgb(180 35 24 / 8%);
    stroke: var(--danger);
  }

  .eraser {
    fill: rgb(255 255 255 / 35%);
    stroke: var(--muted);
    stroke-width: 1px;
    vector-effect: non-scaling-stroke;
    pointer-events: none;
  }

  .zoom {
    position: absolute;
    right: 0.75rem;
    bottom: 0.75rem;
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.2rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .zoom .button {
    padding: 0.25rem 0.6rem;
  }

  .zoom-level {
    min-width: 4.5rem;
    color: var(--muted);
    font-size: 0.875rem;
    text-align: center;
  }
</style>
