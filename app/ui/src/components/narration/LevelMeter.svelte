<script lang="ts">
  /**
   * The input level: a bar for the RMS (what the ear hears) and a thinner one for the peak, on a
   * decibel scale. One `role="meter"` for assistive technology, whose value text gives both.
   * Nothing depends on colour or hover alone.
   */
  import { t } from '../../lib/i18n.svelte';
  import { levelPercent } from './format';

  let { rms, peak, active = true }: { rms: number; peak: number; active?: boolean } = $props();

  const rmsPercent = $derived(active ? levelPercent(rms) : 0);
  const peakPercent = $derived(active ? levelPercent(peak) : 0);
</script>

<div
  class="meter"
  class:clipping={peakPercent >= 98}
  role="meter"
  aria-label={t('narration.picker.level')}
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={rmsPercent}
  aria-valuetext={t('narration.picker.levelValue', { rms: rmsPercent, peak: peakPercent })}
  data-rms={rmsPercent}
  data-peak={peakPercent}
>
  <span class="bar rms" style:width="{rmsPercent}%"></span>
  <span class="bar peak" style:width="{peakPercent}%"></span>
</div>

<style>
  .meter {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    min-width: 6rem;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-2);
  }

  .bar {
    display: block;
    height: 6px;
    border-radius: 2px;
    transition: width 80ms linear;
  }

  .rms {
    background: var(--accent);
  }

  .peak {
    height: 3px;
    background: var(--muted);
  }

  .clipping .peak {
    background: var(--danger);
  }

  @media (prefers-reduced-motion: reduce) {
    .bar {
      transition: none;
    }
  }
</style>
