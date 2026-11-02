<script lang="ts">
  /**
   * Shown in the library's top bar while a share runs, with the number of screens connected:
   * the teacher always sees that the port is open. It reopens the share panel.
   */
  import { t, tn } from '../../lib/i18n.svelte';
  import { share } from '../../lib/share-state.svelte';
</script>

{#if share.status.active}
  <button
    type="button"
    class="share-indicator"
    class:paused={share.status.paused}
    data-viewers={share.status.viewers}
    aria-haspopup="dialog"
    aria-label={tn('share.indicatorHint', share.status.viewers)}
    title={tn('share.indicatorHint', share.status.viewers)}
    onclick={() => share.openPanel()}
  >
    <span class="dot" aria-hidden="true"></span>
    <span aria-hidden="true">{t('share.indicator')}</span>
    <span class="count" aria-hidden="true">{share.status.viewers}</span>
  </button>
{/if}

<style>
  .share-indicator {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--accent);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
    cursor: pointer;
  }

  .share-indicator:hover {
    background: var(--surface-2);
  }

  .share-indicator:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: #1f9d55;
  }

  .paused .dot {
    background: var(--muted);
  }

  .count {
    min-width: 1.3rem;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 700;
    text-align: center;
  }
</style>
