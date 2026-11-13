<script lang="ts">
  /**
   * The question of the « pages first » refusal (docs/design/blackboard.md §9.5): the Xiaomi tablet still holds stored
   * pages, so they are imported and deleted from it before live mode starts. Two buttons (import and delete, cancel);
   * Escape cancels; the focus goes to the first button and comes back to where it was (`Modal`). While the import runs it
   * shows the sync's progress, the import cannot be asked twice, and Cancel stops the sync between two pages. The state and
   * the flow are the controller's (`live-pages-first.svelte.ts`), which this follows on the window's tablet status.
   */
  import { t } from '../../lib/i18n.svelte';
  import { pagesFirstText } from '../../lib/live-pages-first';
  import type { PagesFirst } from '../../lib/live-pages-first.svelte';
  import { tablet } from '../../lib/tablet-state.svelte';
  import Modal from '../Modal.svelte';

  let { controller }: { controller: PagesFirst } = $props();

  const importing = $derived(controller.phase === 'importing');
  const sync = $derived(tablet.status?.sync ?? null);

  // The tablet's status says how the import goes: syncing, then a new last sync (or a failure, or a lost link).
  $effect(() => {
    void controller.phase;
    controller.follow(tablet.status);
  });
</script>

{#if controller.open}
  <Modal title={t('live.pagesFirst.title')} role="alertdialog" onclose={() => controller.cancel()}>
    <div data-slot="pages-first" data-pages={controller.pages} data-phase={controller.phase}>
      <p data-slot="pages-first-text">{pagesFirstText(controller.pages)}</p>
      {#if importing}
        <div data-slot="pages-first-progress">
          <p class="small" role="status">
            {t('live.pagesFirst.importing')}
            {#if sync}
              {t('tablet.sync.progress', { current: sync.current, total: sync.total })}
            {/if}
          </p>
          <progress
            class="bar"
            max={Math.max(sync?.total ?? 1, 1)}
            value={sync?.current ?? 0}
            aria-label={t('tablet.sync.progressLabel')}
          ></progress>
        </div>
      {/if}
      {#if controller.error}
        <p class="banner error inline" role="alert" data-slot="pages-first-error">
          {t('live.pagesFirst.failed', { reason: controller.error })}
        </p>
      {/if}
    </div>
    <div class="actions">
      <button
        type="button"
        class="button primary"
        data-autofocus
        data-action="pages-first-import"
        disabled={importing}
        onclick={() => void controller.importPages()}
      >
        {t('live.pagesFirst.confirm')}
      </button>
      <button type="button" class="button" data-action="pages-first-cancel" onclick={() => controller.cancel()}>
        {t('settings.cancel')}
      </button>
    </div>
  </Modal>
{/if}

<style>
  .bar {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
