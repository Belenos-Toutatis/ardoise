<script lang="ts" module>
  /** The id of the next stroke in progress (`oninprogress`): unique for the app's life, whatever canvas draws it. */
  let nextStrokeId = 1;
</script>

<script lang="ts">
  /**
   * The page: an SVG whose `viewBox` is the visible part of the page in device units, the
   * rendered background as an `<image>`, then one non-zero filled path per visible stroke in
   * drawing order (outlines cached per stroke object). Handles drawing, erasing, lasso, moving
   * and resizing the selection, manual crop, zoom (wheel at the cursor) and pan (middle button,
   * Space + drag, or the pan tool). A drag shows transient state and reports one change at the end.
   *
   * Pen and touch (docs/design/interface.md, "M8: pen and touch"): the back of the pen erases
   * whatever the tool, the pen's barrel button draws a lasso, a finger pans (two fingers pinch,
   * a double tap fits the page) unless "Draw with a finger" is on, and a finger is ignored while
   * a pen is in contact or hovering.
   *
   * Shape tools (docs/design/shape-tools.md): with the line, rectangle or ellipse tool a drag shows the shape between its
   * two ends, as it will be drawn (`lib/editor/shape-tools.ts`, the toggles in `shapePrefs`, Shift and Alt inverting two
   * of them while held), and one ordinary stroke is added when the pointer is let go; Escape drops it
   * (`cancelGesture`).
   *
   * M17 (docs/design/paper-projection-narration.md §2.6, §3.2): while a writing recording runs, a stroke takes its
   * times from `strokeClock` (the sound's clock); otherwise it goes after the note's ink and the end of a note-time
   * narration (`nextStrokeBase`). The stroke in progress is told to `oninprogress` (the projection streams it), what
   * the canvas shows of the page to `onvisible`, and the projected area is drawn as a dashed frame (`projected`).
   */
  import { untrack } from 'svelte';
  import { api } from '../../lib/api';
  import {
    applyAffine,
    boundsOfPoints,
    intersection,
    rectFromCorners,
    scalingAround,
    svgMatrix,
    translation,
    type Affine,
    type Rect,
    type Vec,
  } from '../../lib/editor/geometry';
  import { drawingOrder, hitStroke, onLayer, selectInLasso } from '../../lib/editor/hit';
  import {
    PenPresence,
    TouchGestures,
    barrelHeld,
    inkPressure,
    pinchView,
    pointerKind,
    type TouchAction,
  } from '../../lib/gestures';
  import { corners, grabAt as grabOnFrame } from '../../lib/selection-grab';
  import { touchPrefs } from '../../lib/touch-prefs.svelte';
  import { placedImage } from '../../lib/editor/image';
  import { MIN_CROP } from '../../lib/editor/ops';
  import type { SplitPreview } from '../../lib/editor/split';
  import {
    addStroke,
    canDraw,
    crop,
    erase,
    inksNewStrokes,
    select,
    selectionBounds,
    transformSelection,
    type Doc,
    type ToolSettings,
  } from '../../lib/editor/state';
  import {
    backgroundWidth,
    fitView,
    fitWidthView,
    panBy,
    screenToPage,
    visibleRect,
    zoomAt,
    type View,
    type Viewport,
  } from '../../lib/editor/view';
  import { shapePrefs } from '../../lib/editor/shape-prefs.svelte';
  import {
    MIN_SHAPE_MS,
    buildShapeStroke,
    effectiveToggles,
    shapeDuration,
    type ShapeKind,
    type SnapGrid,
  } from '../../lib/editor/shape-tools';
  import { nextStrokeBase, type StrokeClock } from '../../lib/editor/writing-clock';
  import { t } from '../../lib/i18n.svelte';
  import type { Ink, InkPoint } from '../../lib/ink';
  import { cssColor, type Stroke } from '../../lib/note';
  import type { InProgress } from '../../lib/projection-feed';
  import type { NarrationInfo } from '../../lib/video';

  let {
    doc,
    tools,
    ink,
    spacePan = false,
    readonly = false,
    onchange,
    onselect,
    oncropped,
    onblocked,
    split = null,
    onzone,
    straighten = null,
    snapGrid = null,
    strokeClock = null,
    narration = null,
    oninprogress,
    onvisible,
    projected = null,
  }: {
    doc: Doc;
    tools: ToolSettings;
    ink: Ink;
    /** Space is held: a left drag pans. */
    spacePan?: boolean;
    /** Replay: the page can only be moved around (no pen, eraser or lasso, whatever the tool). */
    readonly?: boolean;
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
    /**
     * The setting of "Redresser les formes" is open (M15): the strokes the shapes would replace
     * (`sources`, indices in `doc`) are faded, and the shapes are drawn over them as a dashed
     * accent line. Nothing of it is in the note.
     */
    straighten?: { sources: readonly number[]; strokes: readonly Stroke[] } | null;
    /** The lines of the page's template the shape tools snap to when asked (null: none to snap to). */
    snapGrid?: SnapGrid | null;
    /**
     * A writing recording runs (M17): a stroke begun now takes its times from the sound's clock (the note time of its
     * pointer events), to its end even if the recording stops meanwhile. Null: the editor's rule.
     */
    strokeClock?: StrokeClock | null;
    /** The note's narration: outside a recording, a stroke goes after the end of one in note time (O14). */
    narration?: NarrationInfo | null;
    /** The stroke in progress: begun, grown (all its points so far), ended (`committed`: the doc gets it next). */
    oninprogress?: (event: InProgress) => void;
    /** What the canvas shows of the page (page units), at each change of the view or of its size. */
    onvisible?: (rect: Rect) => void;
    /** The area the projection shows (page units), drawn as a dashed frame; null: none. */
    projected?: Rect | null;
  } = $props();

  /** Screen pixels per device unit at real size (96 dpi, 10 µm units). */
  const REAL_SIZE = 96 / 25.4 / 100;
  const HANDLE_PX = 10;

  /** A shape shorter than this on screen, in pixels, is a click: nothing is drawn. */
  const MIN_SHAPE_PX = 4;

  /** What a lasso does to the selection: replace it, add to it (Shift), remove from it (Ctrl). */
  type Combine = 'replace' | 'add' | 'remove';

  type Drag =
    | { kind: 'pan'; id: number; start: Vec; view: View }
    /** `stroke`: the id the stroke in progress is told by (`oninprogress`). */
    | { kind: 'pen'; id: number; points: InkPoint[]; t0: number; base: number; stroke: number }
    | { kind: 'eraser'; id: number; last: Vec }
    /** A shape tool: the two ends, the keys held now (Shift, Alt), and the time of the pointer-down as the pen has it. */
    | { kind: 'shape'; id: number; shape: ShapeKind; start: Vec; end: Vec; shift: boolean; alt: boolean; t0: number; base: number }
    /** `barrel`: started by the pen's barrel button, which ends when the button is released. */
    | { kind: 'lasso'; id: number; points: Vec[]; combine: Combine; barrel?: boolean }
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
  /** The back of the pen is over the page. */
  let eraserEnd = $state(false);
  const presence = new PenPresence();
  const fingers = new TouchGestures();
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
    const out: { i: number; d: string; fill: string; highlighter: boolean; selected: boolean; side: Side; faded: boolean }[] = [];
    const faded = new Set(straighten?.sources);
    for (const i of drawingOrder(note)) {
      const stroke = note.strokes[i];
      const base = {
        i,
        fill: cssColor(stroke.color),
        highlighter: stroke.tool === 'highlighter',
        selected: selected.has(i),
        faded: faded.has(i),
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

  /** The straightened shapes of the setting's preview, as centre lines (a few strokes at most, not cached). */
  const previewLines = $derived(
    straighten && !dragAffine
      ? straighten.strokes
          .filter((stroke) => stroke.points.length > 1)
          .map((stroke) => 'M' + stroke.points.map((p) => `${p.x} ${p.y}`).join('L'))
      : [],
  );

  const selectionBox = $derived.by(() => {
    const bounds = selectionBounds(shown);
    if (!bounds || !dragAffine) return bounds;
    return rectFromCorners(
      applyAffine(dragAffine, { x: bounds.x, y: bounds.y }),
      applyAffine(dragAffine, { x: bounds.x + bounds.width, y: bounds.y + bounds.height }),
    );
  });

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

  /** Escape: the gesture under way is dropped, the note is not changed (a view moved by a pan stays moved). */
  export function cancelGesture() {
    dropDrag();
  }

  /** The page as wide as the stage (its top shown when it is taller). */
  export function fitWidth() {
    measure();
    view = fitWidthView(doc.note.page, viewport);
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

  // What the canvas shows of the page, at each change of the view or of the stage's size (the projected area follows it).
  $effect(() => {
    const rect = box;
    untrack(() => onvisible?.(rect));
  });

  // A stroke still being drawn when the canvas goes (another note opens) is told to be dropped.
  $effect(() => () => {
    const current = drag;
    if (current?.kind === 'pen') oninprogress?.({ kind: 'end', id: current.stroke, committed: false });
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

  /**
   * The note time of a stroke's first sample put down at `timeStamp`: the sound's clock during a writing recording,
   * else after the note's ink and a note-time narration (whole milliseconds).
   */
  function strokeBase(timeStamp: number): number {
    return strokeClock ? Math.round(strokeClock.noteTime(timeStamp)) : nextStrokeBase(doc.note, narration);
  }

  /** The pen's pressure is recorded, so the line follows it; a mouse or a finger has a constant one. */
  function sample(e: PointerEvent, p: Vec, d: { t0: number; base: number }): InkPoint {
    const pressure = inkPressure(pointerKind(e), e.pressure);
    return { x: p.x, y: p.y, pressure, t: Math.max(0, Math.round(d.base + (e.timeStamp - d.t0))) };
  }

  /** The stroke of the shape being dragged as it would be added now, or null while it is too small. */
  function shapeStroke(d: Extract<Drag, { kind: 'shape' }>, base: number, durationMs: number) {
    return buildShapeStroke({
      kind: d.shape,
      start: d.start,
      end: d.end,
      toggles: effectiveToggles(shapePrefs, d),
      grid: snapGrid,
      minSize: MIN_SHAPE_PX / view.scale,
      tool: tools.tool,
      color: tools.color,
      width: tools.width,
      base,
      durationMs,
    });
  }

  let frame = 0;
  function showLive() {
    if (drag?.kind === 'shape') {
      const stroke = shapeStroke(drag, 0, MIN_SHAPE_MS);
      livePath = stroke ? ink.path({ tool: stroke.tool, width: stroke.width, points: stroke.points }) : '';
      return;
    }
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

  function begin(next: Drag) {
    drag = next;
    dragKind = next.kind;
  }

  /** Drops the gesture under way without reporting it (a second finger took over). */
  function dropDrag() {
    grabbingFinger = false;
    if (!drag) return;
    if (drag.kind === 'pen') oninprogress?.({ kind: 'end', id: drag.stroke, committed: false });
    drag = null;
    dragKind = null;
    if (frame && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(frame);
    frame = 0;
    livePath = '';
    transient = null;
    lasso = null;
    cropRect = null;
    dragAffine = null;
  }

  /** A finger draws (or selects, crops…) only when asked to; otherwise it navigates. */
  function fingerDraws(): boolean {
    return touchPrefs.drawWithFinger && !readonly && tools.mode !== 'pan';
  }

  /** A finger that navigates took hold of the selection (to move or resize it): it is not a pan. */
  let grabbingFinger = false;

  const touchOptions = () => ({ mode: fingerDraws() || grabbingFinger ? ('draw' as const) : ('navigate' as const) });

  /**
   * The selection was made with the pen's barrel button, whatever the tool in use: the pen's tip
   * and a finger then move and resize it, as with the selection tool, instead of drawing.
   */
  let barrelPicked = $state(false);
  $effect(() => {
    if (doc.selection.length === 0) barrelPicked = false;
  });
  const barrelActive = $derived(barrelPicked && doc.selection.length > 0 && tools.mode !== 'select');
  /** The selection can be grabbed: with the selection tool, or after a barrel-button lasso. */
  const grabbable = $derived(
    !readonly && !split && doc.selection.length > 0 && (tools.mode === 'select' || (barrelActive && (inksNewStrokes(tools.mode) || tools.mode === 'eraser'))),
  );

  /**
   * A press on a handle of the selection (resize) or on its frame (move) as a drag, or null. How near it must be depends
   * on the pointer: a pen and a finger reach farther than the mouse (`lib/selection-grab.ts`).
   */
  function frameGrab(e: PointerEvent): Drag | null {
    if (!selectionBox) return null;
    const p = pagePoint(e);
    const grab = grabOnFrame(selectionBox, p, view.scale, pointerKind(e));
    if (!grab) return null;
    const id = pointerId(e);
    return grab.kind === 'scale' ? { kind: 'scale', id, anchor: grab.anchor, start: p } : { kind: 'move', id, start: p };
  }

  /** The same, for a press that may take hold of the selection at all: with the tool for it, and no Shift, Ctrl or Meta. */
  function grabAt(e: PointerEvent): Drag | null {
    if (!grabbable || e.ctrlKey || e.metaKey || e.shiftKey) return null;
    return frameGrab(e);
  }

  function applyTouch(action: TouchAction) {
    if (action.kind === 'pan') view = panBy(view, action.dx, action.dy);
    else if (action.kind === 'pinch') view = pinchView(view, action.from, action.to, action.factor);
    else fit();
  }

  function capture(id: number) {
    try {
      svg?.setPointerCapture?.(id);
    } catch {
      // Not every pointer can be captured (synthetic events); moves still arrive on the canvas.
    }
  }

  /** The pen's barrel button is down: a lasso that ends with the button (or with the contact). */
  function startBarrelLasso(e: PointerEvent, id: number, p: Vec) {
    const combine: Combine = e.ctrlKey || e.metaKey ? 'remove' : e.shiftKey ? 'add' : 'replace';
    begin({ kind: 'lasso', id, points: [p], combine, barrel: true });
    lasso = [p];
    lassoRemoves = combine === 'remove';
  }

  function onpointerdown(e: PointerEvent) {
    const kind = pointerKind(e);
    const id = pointerId(e);
    if (kind === 'pen' || kind === 'eraser') {
      presence.seen(id, e.timeStamp, 'down');
      fingers.reset();
    } else if (kind === 'touch') {
      // Palm rejection, then the finger gestures; a finger belongs to the tools only when asked.
      if (presence.blocksTouch(e.timeStamp)) return;
      if (drag && !fingers.has(drag.id)) return;
      if (fingers.down(id, local(e), e.timeStamp)) {
        e.preventDefault();
        dropDrag();
        return;
      }
      if (fingers.count > 1 || !fingers.has(id)) return;
      if (!fingerDraws()) {
        e.preventDefault();
        capture(id);
        // A finger on the selection moves or resizes it instead of panning the page.
        const grab = e.button === 0 ? grabAt(e) : null;
        if (grab) {
          grabbingFinger = true;
          begin(grab);
        }
        return;
      }
    }
    if (drag) return;
    // The back of the pen erases and the barrel button lassoes, whatever the tool.
    if (!readonly && !split && (kind === 'eraser' || barrelHeld(e))) {
      e.preventDefault();
      if (document.activeElement instanceof HTMLInputElement) document.activeElement.blur();
      capture(id);
      const p = pagePoint(e);
      if (kind === 'eraser') {
        transient = erase(doc, [p], tools.eraserRadius, tools.eraser);
        begin({ kind: 'eraser', id, last: p });
        hover = p;
      } else {
        startBarrelLasso(e, id, p);
      }
      return;
    }
    const panRequested = e.button === 1 || (e.button === 0 && (spacePan || tools.mode === 'pan'));
    if (e.button !== 0 && !panRequested) return;
    e.preventDefault();
    // Splitting: the page is read-only, except for tracing the zone.
    if (split && !panRequested && !split.tracing) return;
    if (!split && !panRequested && inksNewStrokes(tools.mode) && !canDraw(doc)) {
      onblocked?.();
      return;
    }
    // A pending title or layer name edit is committed by leaving its field.
    if (document.activeElement instanceof HTMLInputElement) document.activeElement.blur();
    capture(id);
    if (panRequested) return begin({ kind: 'pan', id, start: local(e), view });
    const p = pagePoint(e);
    if (split) {
      begin({ kind: 'zone', id, points: [p] });
      lasso = [p];
      lassoRemoves = false;
      return;
    }
    // After a barrel-button lasso the pen's tip and a finger take hold of the selection, not the page.
    if (barrelActive && (kind === 'pen' || kind === 'touch')) {
      const grab = grabAt(e);
      if (grab) return begin(grab);
    }
    switch (tools.mode) {
      case 'pen': {
        const d = { t0: e.timeStamp, base: strokeBase(e.timeStamp) };
        const stroke = nextStrokeId++;
        const points = [sample(e, p, d)];
        begin({ kind: 'pen', id, points, ...d, stroke });
        showLive();
        oninprogress?.({
          kind: 'begin',
          id: stroke,
          tool: tools.tool,
          color: { ...tools.color },
          width: tools.width,
          layer: doc.activeLayer,
          points,
          // A point's wall time is the time origin plus its event's `timeStamp`.
          wallAtBase: performance.timeOrigin + d.t0,
          base: d.base,
        });
        break;
      }
      case 'eraser':
        transient = erase(doc, [p], tools.eraserRadius, tools.eraser);
        begin({ kind: 'eraser', id, last: p });
        break;
      case 'line':
      case 'rectangle':
      case 'ellipse':
        begin({ kind: 'shape', id, shape: tools.mode, start: p, end: p, shift: e.shiftKey, alt: e.altKey, t0: e.timeStamp, base: strokeBase(e.timeStamp) });
        break;
      case 'select': {
        // With Shift or Ctrl the gesture is always a lasso, even on the selection's frame.
        const combine: Combine = e.ctrlKey || e.metaKey ? 'remove' : e.shiftKey ? 'add' : 'replace';
        const grab = combine === 'replace' ? frameGrab(e) : null;
        if (grab) begin(grab);
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
    const kind = pointerKind(e);
    if (kind === 'pen' || kind === 'eraser') {
      presence.seen(pointerId(e), e.timeStamp, ((e.buttons ?? 0) & 33) !== 0 ? 'down' : 'hover');
      eraserEnd = kind === 'eraser' && !readonly && !split;
    } else if (kind === 'touch' && fingers.has(pointerId(e))) {
      const action = fingers.move(pointerId(e), local(e), touchOptions());
      if (action) applyTouch(action);
      if (fingers.count > 1) return;
    }
    if (tools.mode === 'eraser' || eraserEnd) hover = pagePoint(e);
    else if (hover) hover = null;
    // Pressing the barrel button of a pen that is not drawing arrives as a move whose buttons
    // changed (no `pointerdown`, e.g. through the DevTools protocol): it starts the lasso too, and
    // releasing the button ends it.
    if (!drag && kind === 'pen' && barrelHeld(e) && !readonly && !split) {
      e.preventDefault();
      if (document.activeElement instanceof HTMLInputElement) document.activeElement.blur();
      capture(pointerId(e));
      startBarrelLasso(e, pointerId(e), pagePoint(e));
      return;
    }
    if (drag?.kind === 'lasso' && drag.barrel && pointerId(e) === drag.id && kind === 'pen' && !barrelHeld(e)) {
      finish(e, false);
      return;
    }
    if (!drag || pointerId(e) !== drag.id) return;
    switch (drag.kind) {
      case 'pan': {
        const s = local(e);
        view = { ...drag.view, x: drag.view.x + s.x - drag.start.x, y: drag.view.y + s.y - drag.start.y };
        break;
      }
      case 'pen': {
        const events = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : [];
        const before = drag.points.length;
        for (const ev of events.length > 0 ? events : [e]) {
          const p = pagePoint(ev);
          const last = drag.points[drag.points.length - 1];
          if (Math.hypot(p.x - last.x, p.y - last.y) * view.scale < 0.5) continue;
          drag.points.push(sample(ev, p, drag));
        }
        scheduleLive();
        if (drag.points.length > before) oninprogress?.({ kind: 'grow', id: drag.stroke, points: drag.points });
        break;
      }
      case 'eraser': {
        const p = pagePoint(e);
        transient = erase(transient ?? doc, [drag.last, p], tools.eraserRadius, tools.eraser);
        drag.last = p;
        break;
      }
      case 'shape':
        drag.end = pagePoint(e);
        drag.shift = e.shiftKey;
        drag.alt = e.altKey;
        scheduleLive();
        break;
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

  /** The pointer is up (or was taken away): finger and pen bookkeeping, then the gesture ends. */
  function release(e: PointerEvent, cancelled: boolean) {
    const kind = pointerKind(e);
    if (kind === 'pen' || kind === 'eraser') presence.lifted(pointerId(e), e.timeStamp);
    else if (kind === 'touch') {
      let action: TouchAction | null = null;
      if (cancelled) fingers.cancel(pointerId(e));
      else action = fingers.up(pointerId(e), e.timeStamp, touchOptions());
      if (action) applyTouch(action);
    }
    finish(e, cancelled);
  }

  /** The pen's barrel button is a right click for Windows: no menu over the page then. */
  function noPenMenu(e: MouseEvent) {
    if ((e as PointerEvent).pointerType === 'pen') e.preventDefault();
  }

  function leave(e: PointerEvent) {
    hover = null;
    eraserEnd = false;
    if (e.pointerType === 'pen') presence.gone(pointerId(e));
  }

  /** Shift or Alt pressed or let go while a shape is dragged: the preview follows without waiting for the pointer to move. */
  function shapeKeys(e: KeyboardEvent) {
    if (drag?.kind !== 'shape' || (e.key !== 'Shift' && e.key !== 'Alt')) return;
    drag.shift = e.shiftKey;
    drag.alt = e.altKey;
    scheduleLive();
  }

  function finish(e: PointerEvent, cancelled: boolean) {
    if (!drag || pointerId(e) !== drag.id) return;
    const d = drag;
    drag = null;
    dragKind = null;
    grabbingFinger = false;
    if (frame && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(frame);
    frame = 0;
    switch (d.kind) {
      case 'pen':
        // Told first: the doc that gets the stroke is what the projection compares with next.
        oninprogress?.({ kind: 'end', id: d.stroke, committed: !cancelled });
        if (!cancelled) {
          onchange(addStroke(doc, { tool: tools.tool, color: { ...tools.color }, width: tools.width, points: d.points }));
        }
        livePath = '';
        break;
      case 'eraser':
        if (!cancelled && transient && transient !== doc) onchange(transient);
        transient = null;
        break;
      case 'shape': {
        livePath = '';
        if (cancelled) break;
        // The keys held as the pointer is let go count, and the shape takes the time the drag took.
        d.end = pagePoint(e);
        d.shift = e.shiftKey;
        d.alt = e.altKey;
        const stroke = shapeStroke(d, d.base, shapeDuration(e.timeStamp - d.t0, strokeClock !== null));
        if (stroke) onchange(addStroke(doc, stroke));
        break;
      }
      case 'lasso': {
        lasso = null;
        if (cancelled) break;
        const bounds = boundsOfPoints(d.points);
        // The selection takes strokes of the chosen layer only.
        const chosen = onLayer(doc.note, doc.activeLayer);
        let picked: number[];
        if (!bounds || Math.max(bounds.width, bounds.height) * view.scale < 4) {
          const hit = hitStroke(chosen, d.points[0], 6 / view.scale);
          picked = hit === null ? [] : [hit];
        } else {
          picked = selectInLasso(chosen, d.points);
        }
        const next =
          d.combine === 'add'
            ? [...doc.selection, ...picked]
            : d.combine === 'remove'
              ? doc.selection.filter((i) => !picked.includes(i))
              : picked;
        onselect(select(doc, next));
        barrelPicked = !!d.barrel && next.length > 0;
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

<svelte:window onkeydown={shapeKeys} onkeyup={shapeKeys} />

<div class="stage" bind:this={stage}>
  <svg
    bind:this={svg}
    class="canvas mode-{split?.tracing ? 'zone' : tools.mode}"
    class:space={spacePan}
    class:eraser-end={eraserEnd}
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
    onpointerup={(e) => release(e, false)}
    onpointercancel={(e) => release(e, true)}
    onlostpointercapture={(e) => {
      // The capture of a child that had it implicitly (a finger on a stroke) is not the gesture's end.
      if (e.target === e.currentTarget) release(e, true);
    }}
    onpointerleave={leave}
    oncontextmenu={noPenMenu}
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
          class:straighten-faded={item.faded}
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
    {#if previewLines.length > 0}
      <g class="straighten-preview" data-slot="straighten-preview" pointer-events="none">
        {#each previewLines as d}
          <path {d} fill="none" />
        {/each}
      </g>
    {/if}
    {#if selectionBox}
      <rect
        class="selection"
        x={selectionBox.x}
        y={selectionBox.y}
        width={selectionBox.width}
        height={selectionBox.height}
      />
      {#if tools.mode === 'select' || barrelActive}
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
    {#if hover && (tools.mode === 'eraser' || eraserEnd) && !split && !readonly}
      <circle class="eraser" cx={hover.x} cy={hover.y} r={tools.eraserRadius} />
    {/if}
    {#if projected}
      <rect
        class="projected"
        data-slot="projected-area"
        x={projected.x}
        y={projected.y}
        width={projected.width}
        height={projected.height}
        pointer-events="none"
      />
    {/if}
  </svg>
  <div class="zoom" role="group" aria-label={t('editor.zoom.label')}>
    <button class="button ghost" type="button" title={t('editor.zoom.out')} aria-label={t('editor.zoom.out')} onclick={() => zoomBy(1 / 1.25)}>−</button>
    <span class="zoom-level" aria-live="polite">{t('editor.zoom.level', { percent: zoomPercent })}</span>
    <button class="button ghost" type="button" title={t('editor.zoom.in')} aria-label={t('editor.zoom.in')} onclick={() => zoomBy(1.25)}>+</button>
    <button class="button ghost" type="button" onclick={fit}>{t('editor.zoom.fit')}</button>
    <button class="button ghost" type="button" data-action="fit-width" title={t('editor.zoom.fitWidthTitle')} onclick={fitWidth}>{t('editor.zoom.fitWidth')}</button>
    {#if !readonly}
      <button
        class="button ghost"
        type="button"
        data-action="finger-draw"
        aria-pressed={touchPrefs.drawWithFinger}
        class:on={touchPrefs.drawWithFinger}
        title={t('editor.touch.drawTitle')}
        onclick={() => touchPrefs.set(!touchPrefs.drawWithFinger)}>{t('editor.touch.draw')}</button
      >
    {/if}
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
  .mode-line,
  .mode-rectangle,
  .mode-ellipse,
  .mode-crop,
  .mode-zone {
    cursor: crosshair;
  }

  .mode-eraser,
  .eraser-end {
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

  .split-kept,
  .straighten-faded {
    opacity: 0.3;
  }

  .straighten-preview path {
    stroke: var(--accent);
    stroke-width: 2px;
    stroke-dasharray: 6 4;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
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

  /* What the class sees on the projector (M17, O5): a dashed frame, never in the way of the pen. */
  .projected {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2px;
    stroke-dasharray: 8 5;
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

  .zoom .on {
    border-color: var(--accent);
    background: rgb(47 111 223 / 12%);
  }

  .zoom-level {
    min-width: 4.5rem;
    color: var(--muted);
    font-size: 0.875rem;
    text-align: center;
  }
</style>
