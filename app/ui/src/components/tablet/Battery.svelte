<script lang="ts">
  /** Battery icon with its level in words; a bolt while charging, red when nearly empty. */
  import { t } from '../../lib/i18n.svelte';

  let { percent, charging = false }: { percent: number; charging?: boolean } = $props();

  const level = $derived(Math.max(0, Math.min(100, Math.round(percent))));
  const label = $derived(t(charging ? 'tablet.batteryCharging' : 'tablet.battery', { percent: level }));
  // Inner width of the icon: 14 units for 100 %.
  const fill = $derived((14 * level) / 100);
</script>

<span class="battery" class:low={level <= 15 && !charging} data-battery={level} data-charging={charging} role="img" aria-label={label}>
  <svg viewBox="0 0 20 10" width="20" height="10" aria-hidden="true">
    <rect x="0.5" y="0.5" width="16" height="9" rx="1.5" fill="none" stroke="currentColor" />
    <rect x="17" y="3" width="2.5" height="4" rx="0.8" fill="currentColor" />
    <rect x="2" y="2" width={fill} height="6" rx="0.8" fill="currentColor" />
  </svg>
  <span class="percent">{level} %</span>
  {#if charging}
    <svg class="bolt" viewBox="0 0 8 12" width="8" height="12" aria-hidden="true">
      <path d="M5 0 0 7h3L2.5 12 8 4.5H5z" fill="currentColor" />
    </svg>
  {/if}
</span>

<style>
  .battery {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--muted);
    font-size: 0.8125rem;
  }

  .battery.low {
    color: var(--danger);
  }

  .bolt {
    color: #d99a1b;
  }
</style>
