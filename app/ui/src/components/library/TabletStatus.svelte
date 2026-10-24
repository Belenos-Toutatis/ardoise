<script lang="ts">
  /**
   * Tablet block at the bottom of the sidebar: name, connection state (dot and words), battery,
   * and a progress bar while syncing. A click opens or closes the tablet panel. It follows the
   * window's shared status (`tablet-state.svelte.ts`).
   */
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { tablet } from '../../lib/tablet-state.svelte';
  import Battery from '../tablet/Battery.svelte';

  let { open = false, onopen = () => {} }: { open?: boolean; onopen?: () => void } = $props();

  onMount(() => tablet.start());

  const status = $derived(tablet.status);
  const connection = $derived(status?.connection ?? 'unavailable');
  const name = $derived(status?.registered?.name ?? null);
  const sync = $derived(status?.sync ?? null);
  const device = $derived(status?.device ?? null);
  /** The battery is known only while the tablet has been read, and a forgotten tablet has none. */
  const showBattery = $derived(device !== null && status?.registered != null);
  const stateText = $derived(
    status === null && !tablet.unavailable ? t('common.loading') : t(`tablet.state.${connection}`),
  );
</script>

<button
  type="button"
  class="tablet-status"
  data-slot="tablet-status"
  data-connection={connection}
  aria-expanded={open}
  title={t('library.tabletState')}
  onclick={onopen}
>
  <span class="tablet-label">{t('library.tabletStatus')}</span>
  <span class="tablet-name" class:muted={!name}>{name ?? t('library.tabletNone')}</span>
  {#if name || status?.connection === 'registering'}
    <span class="line">
      <span class="dot" aria-hidden="true"></span>
      <span class="state">{stateText}</span>
    </span>
  {/if}
  {#if showBattery && device}
    <Battery percent={device.batteryPercent} charging={device.charging} />
  {/if}
  {#if sync}
    <span class="sync">
      <progress
        class="bar"
        max={Math.max(sync.total, 1)}
        value={sync.current}
        aria-label={t('tablet.sync.progressLabel')}
      ></progress>
      <span class="small">{t('tablet.sync.progress', { current: sync.current, total: sync.total })}</span>
    </span>
  {/if}
</button>

<style>
  .tablet-status {
    gap: 0.15rem;
    width: 100%;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.875rem;
    text-align: left;
    cursor: pointer;
  }

  .tablet-status:hover .tablet-name {
    text-decoration: underline;
  }

  .tablet-status:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 6px;
  }

  .tablet-status[aria-expanded='true'] .tablet-name {
    color: var(--accent);
  }

  .tablet-name {
    font-weight: 600;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .dot {
    flex: none;
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--muted);
  }

  [data-connection='connected'] .dot,
  [data-connection='syncing'] .dot,
  [data-connection='live'] .dot {
    background: #2fa866;
  }

  [data-connection='syncing'] .dot,
  [data-connection='live'] .dot {
    background: var(--accent);
  }

  [data-connection='searching'] .dot,
  [data-connection='connecting'] .dot,
  [data-connection='registering'] .dot {
    background: #d99a1b;
    animation: pulse 1.4s ease-in-out infinite;
  }

  [data-connection='unavailable'] .dot {
    background: var(--danger);
  }

  .sync {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    margin-top: 0.15rem;
  }

  .bar {
    width: 100%;
    height: 0.4rem;
    accent-color: var(--accent);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none !important;
    }
  }
</style>
