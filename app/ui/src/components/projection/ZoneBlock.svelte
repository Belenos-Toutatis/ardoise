<script lang="ts">
  /**
   * « Zone projetée »: the block of a window's Projection tab that sets what the projector shows of the page (milestone
   * M16; docs/design/side-panel.md, §8): the zoom as − / value / + (O9: no slider), « whole page » and « page width »,
   * then what the window adds (`children`: live's blackboard and laser switches), then a hint. The window does the
   * acts (`zoomStep`, `showWholePage`, `showPageWidth`): this block only shows the zoom and calls back, so that the
   * live control window and, from M17, the editor use the same block.
   */
  import type { Snippet } from 'svelte';
  import { currentLocale, t } from '../../lib/i18n.svelte';

  let {
    zoom,
    min,
    max,
    onzoom,
    onwhole,
    onwidth,
    children,
    hint,
  }: {
    zoom: number;
    /** The smallest and the largest zoom: − and + are disabled at the ends. */
    min: number;
    max: number;
    /** One step in (1) or out (−1). */
    onzoom: (direction: 1 | -1) => void;
    onwhole: () => void;
    onwidth: () => void;
    /** Rows the window adds below the zoom (live: the blackboard and the laser pointer). */
    children?: Snippet;
    /** A sentence under the block's controls. */
    hint?: string;
  } = $props();

  const decimal = $derived(new Intl.NumberFormat(currentLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }));
</script>

<section class="pane-block" data-section="zone" aria-labelledby="zone-title">
  <h3 class="pane-title" id="zone-title">{t('live.zoom.area')}</h3>
  <div class="pane-actions" role="group" aria-label={t('live.zoom.label')}>
    <button
      type="button"
      class="button small-button"
      aria-label={t('live.zoom.out')}
      title={t('live.zoom.out')}
      disabled={zoom <= min + 1e-3}
      onclick={() => onzoom(-1)}>−</button
    >
    <span class="pane-value" aria-live="polite" data-zoom={zoom}>{t('live.zoom.level', { zoom: decimal.format(zoom) })}</span>
    <button
      type="button"
      class="button small-button"
      aria-label={t('live.zoom.in')}
      title={t('live.zoom.in')}
      disabled={zoom >= max - 1e-3}
      onclick={() => onzoom(1)}>+</button
    >
  </div>
  <div class="pane-actions">
    <button type="button" class="button small-button" data-action="whole-page" onclick={onwhole}>
      {t('live.zoom.fit')}
    </button>
    <button type="button" class="button small-button" data-action="page-width" title={t('live.zoom.fitWidthTitle')} onclick={onwidth}>
      {t('live.zoom.fitWidth')}
    </button>
  </div>
  {@render children?.()}
  {#if hint}
    <p class="pane-help">{hint}</p>
  {/if}
</section>

<style>
  .small-button {
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
