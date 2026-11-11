<script lang="ts">
  /**
   * « Partager sur le réseau »: the button of the « Partage sur le réseau » block (`ShareBlock.svelte`) in the
   * Projection tab of the editor's and of the live control window's side panel (milestone M16; before that, of their
   * toolbars). It opens the share panel, which starts the share if none runs. While a share
   * runs the button is highlighted and carries the number of screens connected
   * (`.share-indicator`), so that the teacher always sees that the port is open.
   */
  import { t, tn } from '../../lib/i18n.svelte';
  import { share } from '../../lib/share-state.svelte';

  const active = $derived(share.status.active);
  const viewers = $derived(share.status.viewers);
</script>

<button
  type="button"
  class="button share-button"
  class:active
  data-action="share"
  aria-haspopup="dialog"
  aria-label={active ? tn('share.buttonActive', viewers) : undefined}
  title={t('share.buttonHint')}
  onclick={() => share.openPanel()}
>
  <span aria-hidden="true">⇄</span>
  {t('share.button')}
  {#if active}
    <span class="share-indicator" data-viewers={viewers} aria-hidden="true">{viewers}</span>
  {/if}
</button>

<style>
  .share-button {
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  /* With `.button` too: the editor's `.editor .button.active` (a light background for its tool toggles) weighs as much
     as `.share-button.active` alone and came later, which left white text on a light background. */
  .button.share-button.active,
  .button.share-button.active:hover {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .share-indicator {
    display: inline-block;
    min-width: 1.3rem;
    margin-left: 0.2rem;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: var(--accent-text);
    color: var(--accent);
    font-size: 0.8rem;
    font-weight: 700;
    text-align: center;
  }
</style>
