<script lang="ts">
  /**
   * « Partage sur le réseau »: the block of both windows' Projection tab (milestone M16; docs/design/side-panel.md,
   * §2 "Help, share block"), around the unchanged `ShareButton`. It says what the share does, and, while one runs,
   * how many screens are connected (and that it is paused). No props: the status is the window's share
   * (`share-state.svelte.ts`). Package 0's skeleton: the content and the hooks, no style (package A).
   */
  import { t, tn } from '../../lib/i18n.svelte';
  import { share } from '../../lib/share-state.svelte';
  import ShareButton from './ShareButton.svelte';

  const status = $derived(share.status);
</script>

<section class="pane-block" data-section="share" aria-labelledby="side-share-title">
  <h3 class="pane-title" id="side-share-title">{t('share.block.title')}</h3>
  <p class="pane-help" data-slot="share-status">
    {status.active ? tn('share.block.active', status.viewers) : t('share.block.idle')}
    {#if status.active && status.paused}{' '}{t('share.paused')}{/if}
  </p>
  <ShareButton />
</section>
