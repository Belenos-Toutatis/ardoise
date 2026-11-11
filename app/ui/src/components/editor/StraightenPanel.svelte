<script lang="ts">
  /**
   * The setting of "Redresser les formes" (M15, SPEC §4.4, Q12): a panel under the split button of
   * the toolbar with the tolerance, the minimum size and the automatic alignment, how many shapes
   * the selection would give with these values, and « Valeurs par défaut » / « Redresser ». The
   * page shows the preview while it is open (the editor draws it).
   *
   * It never goes past the bottom (or the sides) of the window: its height is the room left under
   * the button, it scrolls (wheel, finger) and its buttons stay at its bottom (CSS `sticky`).
   */
  import { onMount } from 'svelte';
  import { currentLocale, t, tn } from '../../lib/i18n.svelte';
  import type { ShapeSettings } from '../../lib/note';
  import { SHAPE_LIMITS, closedMinMm, formatShapeNumber, lettersAtRisk } from '../../lib/shape-settings';

  let {
    id,
    settings,
    selected,
    found,
    onchange,
    ondefaults,
    onapply,
  }: {
    /** The id the toolbar button's `aria-controls` points to. */
    id: string;
    settings: ShapeSettings;
    /** How many strokes are selected. */
    selected: number;
    /** How many shapes the selection gives with `settings`. */
    found: number;
    onchange: (next: ShapeSettings) => void;
    ondefaults: () => void;
    onapply: () => void;
  } = $props();

  /** What stays free between the panel and the edge of the window, in px. */
  const MARGIN = 12;
  /** The gap between the button and the panel, in px (as in the CSS). */
  const GAP = 6;
  /** The panel is never shorter than this, so that something stays to see on a very low window. */
  const MIN_HEIGHT = 160;
  const MAX_WIDTH = 360;

  let panel = $state<HTMLDivElement>();
  let tolerance = $state<HTMLInputElement>();

  const countText = $derived(
    selected === 0
      ? t('editor.straighten.settings.count.noSelection')
      : found === 0
        ? t('editor.straighten.settings.count.none')
        : t('editor.straighten.settings.count.some', {
            shapes: tn('editor.straighten.settings.count.shapes', found),
            strokes: tn('editor.straighten.settings.count.strokes', selected),
          }),
  );

  /** A number as the interface language writes it (7,5 in French): a hand-edited file may hold decimals. */
  const num = (value: number) => formatShapeNumber(value, currentLocale());

  const set = (change: Partial<ShapeSettings>) => onchange({ ...settings, ...change });
  const fromInput = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);

  /**
   * Fits the panel in the window: its height is the room from its top to the bottom of the
   * window less a margin (it scrolls inside), and it is shifted left when the button is so far
   * right that the panel would leave the window. Run when it opens and when the window resizes.
   */
  function fit() {
    const el = panel;
    const anchor = el?.parentElement;
    if (!el || !anchor) return;
    const rect = anchor.getBoundingClientRect();
    const room = window.innerHeight - rect.bottom - GAP - MARGIN;
    el.style.setProperty('--room', `${Math.max(MIN_HEIGHT, Math.floor(room))}px`);
    const width = Math.min(MAX_WIDTH, window.innerWidth - 2 * MARGIN);
    const shift = Math.max(MARGIN - rect.left, Math.min(0, window.innerWidth - MARGIN - width - rect.left));
    el.style.setProperty('--shift', `${Math.round(shift)}px`);
  }

  onMount(() => {
    fit();
    window.addEventListener('resize', fit);
    // An emulated or zoomed viewport may change size without a `resize` on the window.
    const viewport = window.visualViewport;
    viewport?.addEventListener('resize', fit);
    // The toolbar wraps and grows: the panel moves with its button.
    const toolbar = panel?.closest('.toolbar');
    const watcher = typeof ResizeObserver === 'undefined' || !toolbar ? null : new ResizeObserver(fit);
    if (toolbar) watcher?.observe(toolbar);
    tolerance?.focus({ preventScroll: true });
    return () => {
      window.removeEventListener('resize', fit);
      viewport?.removeEventListener('resize', fit);
      watcher?.disconnect();
    };
  });
</script>

<div {id} bind:this={panel} class="shapes-panel" role="dialog" aria-label={t('editor.straighten.settings.button')} data-panel="straighten">
  <h2>{t('editor.straighten.settings.title')}</h2>
  <p class="count" data-slot="straighten-count" aria-live="polite">{countText}</p>

  <div class="field">
    <div class="field-head">
      <label for="{id}-tolerance">{t('editor.straighten.settings.tolerance.label')}</label>
      <output for="{id}-tolerance" data-slot="tolerance-value">{t(`editor.straighten.settings.tolerance.levels.${settings.tolerance}`)}</output>
    </div>
    <input
      bind:this={tolerance}
      id="{id}-tolerance"
      type="range"
      min={SHAPE_LIMITS.tolerance[0]}
      max={SHAPE_LIMITS.tolerance[1]}
      step="1"
      value={settings.tolerance}
      aria-valuetext={t(`editor.straighten.settings.tolerance.levels.${settings.tolerance}`)}
      oninput={(e) => set({ tolerance: fromInput(e) })}
    />
    <div class="ends" aria-hidden="true">
      <span>{t('editor.straighten.settings.tolerance.strict')}</span>
      <span>{t('editor.straighten.settings.tolerance.normal')}</span>
      <span>{t('editor.straighten.settings.tolerance.lenient')}</span>
    </div>
    <p class="help">{t('editor.straighten.settings.tolerance.help')}</p>
  </div>

  <div class="field">
    <div class="field-head">
      <label for="{id}-size">{t('editor.straighten.settings.size.label')}</label>
      <output for="{id}-size" data-slot="size-value">{t('editor.straighten.settings.size.value', { mm: num(settings.minSizeMm) })}</output>
    </div>
    <input
      id="{id}-size"
      type="range"
      min={SHAPE_LIMITS.minSizeMm[0]}
      max={SHAPE_LIMITS.minSizeMm[1]}
      step="1"
      value={settings.minSizeMm}
      aria-valuetext={t('editor.straighten.settings.size.value', { mm: num(settings.minSizeMm) })}
      oninput={(e) => set({ minSizeMm: fromInput(e) })}
    />
    <div class="ends" aria-hidden="true">
      <span>{t('editor.straighten.settings.size.value', { mm: num(SHAPE_LIMITS.minSizeMm[0]) })}</span>
      <span>{t('editor.straighten.settings.size.value', { mm: num(SHAPE_LIMITS.minSizeMm[1]) })}</span>
    </div>
    <p class="help" data-slot="size-help">
      {t('editor.straighten.settings.size.help', { line: num(settings.minSizeMm), closed: num(closedMinMm(settings.minSizeMm)) })}
    </p>
    {#if lettersAtRisk(settings.minSizeMm)}
      <p class="warn" role="note" data-slot="size-warning">{t('editor.straighten.settings.size.warning')}</p>
    {/if}
  </div>

  <div class="field">
    <div class="field-head"><span class="strong">{t('editor.straighten.settings.snap.label')}</span></div>
    <label class="check">
      <input type="checkbox" data-field="snap-angle" checked={settings.snapAngle} onchange={(e) => set({ snapAngle: e.currentTarget.checked })} />
      <span>{t('editor.straighten.settings.snap.angle')}</span>
    </label>
    <div class="sub">
      <div class="field-head">
        <label for="{id}-angle" class="muted">{t('editor.straighten.settings.snap.angleLimit')}</label>
        <output for="{id}-angle">{t('editor.straighten.settings.snap.angleValue', { degrees: num(settings.snapAngleDeg) })}</output>
      </div>
      <input
        id="{id}-angle"
        type="range"
        min={SHAPE_LIMITS.snapAngleDeg[0]}
        max={SHAPE_LIMITS.snapAngleDeg[1]}
        step="1"
        value={settings.snapAngleDeg}
        disabled={!settings.snapAngle}
        aria-valuetext={t('editor.straighten.settings.snap.angleValue', { degrees: num(settings.snapAngleDeg) })}
        oninput={(e) => set({ snapAngleDeg: fromInput(e) })}
      />
    </div>
    <label class="check">
      <input type="checkbox" data-field="snap-round" checked={settings.snapRound} onchange={(e) => set({ snapRound: e.currentTarget.checked })} />
      <span>{t('editor.straighten.settings.snap.round')}</span>
    </label>
    <div class="sub">
      <div class="field-head">
        <label for="{id}-round" class="muted">{t('editor.straighten.settings.snap.roundLimit')}</label>
        <output for="{id}-round">{t('editor.straighten.settings.snap.roundValue', { percent: num(settings.snapRoundPercent) })}</output>
      </div>
      <input
        id="{id}-round"
        type="range"
        min={SHAPE_LIMITS.snapRoundPercent[0]}
        max={SHAPE_LIMITS.snapRoundPercent[1]}
        step="1"
        value={settings.snapRoundPercent}
        disabled={!settings.snapRound}
        aria-valuetext={t('editor.straighten.settings.snap.roundValue', { percent: num(settings.snapRoundPercent) })}
        oninput={(e) => set({ snapRoundPercent: fromInput(e) })}
      />
    </div>
  </div>

  <div class="actions">
    <button type="button" class="button" data-action="straighten-defaults" onclick={ondefaults}>{t('editor.straighten.settings.defaults')}</button>
    <button
      type="button"
      class="button primary"
      data-action="straighten-apply"
      disabled={found === 0}
      title={found === 0 ? t('editor.straighten.settings.applyNone') : undefined}
      onclick={onapply}
    >
      {t('editor.straighten.settings.apply')}
    </button>
  </div>
</div>

<style>
  /* Never taller than the room left down to the bottom of the window: it scrolls (wheel, finger)
     and its buttons stay in sight. `fit()` sets --room and --shift. */
  .shapes-panel {
    position: absolute;
    top: calc(100% + 6px);
    left: var(--shift, 0px);
    z-index: 20;
    width: min(360px, calc(100vw - 24px));
    max-height: var(--room, 70vh);
    padding: 0.85rem 0.95rem 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    color: var(--text);
    font-size: 0.875rem;
  }

  h2 {
    margin: 0 0 0.15rem;
    font-size: 0.95rem;
  }

  .count {
    margin: 0 0 0.7rem;
    color: var(--muted);
  }

  .field {
    margin: 0 0 0.75rem;
  }

  .field-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.5rem;
  }

  .field-head label:not(.muted),
  .strong {
    font-weight: 600;
  }

  .field-head output {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .field input[type='range'] {
    width: 100%;
    margin: 0.25rem 0 0;
    accent-color: var(--accent);
  }

  .ends {
    display: flex;
    justify-content: space-between;
    margin-top: -0.1rem;
    color: var(--muted);
    font-size: 0.78rem;
  }

  .help {
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }

  .warn {
    margin: 0.35rem 0 0;
    padding: 0.35rem 0.5rem;
    border-radius: 6px;
    background: var(--warn-bg);
    color: var(--warn);
    font-size: 0.8rem;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0.35rem 0 0;
  }

  .check input {
    margin-top: 0.2rem;
    accent-color: var(--accent);
  }

  .sub {
    margin: 0.1rem 0 0 1.55rem;
  }

  .muted {
    color: var(--muted);
  }

  .actions {
    position: sticky;
    bottom: 0;
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0 -0.95rem;
    padding: 0.6rem 0.95rem 0.8rem;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
</style>
