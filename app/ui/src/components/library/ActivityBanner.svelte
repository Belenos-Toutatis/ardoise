<script lang="ts">
  /**
   * The library's longer operations (`lib/library-activity.svelte.ts`): progress with « Arrêter »,
   * the notes that failed with their reasons, and the notice with « Annuler » shown for a few
   * seconds (it stays while the pointer or the focus is on it). Stuck to the bottom of the library
   * and placed after its content, so that it never pushes the cards away under the pointer.
   */
  import { onDestroy } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { LibraryActivity } from '../../lib/library-activity.svelte';

  let { activity }: { activity: LibraryActivity } = $props();

  const progress = $derived(activity.progress);

  onDestroy(() => activity.dismissOffer());
</script>

{#if progress || activity.failures || activity.offer}
  <div class="activity">
    {#if progress}
      <div class="banner inline-banner notice activity-progress" role="status">
        <label class="activity-label">
          <span>{t(progress.label, { done: progress.done, total: progress.total })}</span>
          <progress max={progress.total} value={progress.done}></progress>
        </label>
        <button type="button" class="button" disabled={progress.stopping} onclick={() => activity.stop()}>
          {progress.stopping ? t('library.activity.stopping') : t('library.activity.stop')}
        </button>
      </div>
    {/if}

    {#if activity.failures}
      <div class="banner error inline-banner activity-failures" role="alert">
        <div>
          <p class="activity-heading">{activity.failures.heading}</p>
          <ul class="activity-list">
            {#each activity.failures.items as item (item.path)}
              <li>{t('library.activity.failure', { title: item.title, message: item.message })}</li>
            {/each}
          </ul>
        </div>
        <button type="button" class="button ghost" onclick={() => activity.dismissFailures()}>{t('library.dismiss')}</button>
      </div>
    {/if}

    {#if activity.offer}
      <div
        class="banner inline-banner notice activity-offer"
        role="status"
        onmouseenter={() => activity.hold()}
        onmouseleave={() => activity.release()}
        onfocusin={() => activity.hold()}
        onfocusout={() => activity.release()}
      >
        <span>{activity.offer.message}</span>
        <span class="activity-actions">
          {#if activity.offer.undoable}
            <button type="button" class="button primary" data-action="undo" onclick={() => activity.undo()}>
              {t('library.activity.undo')}
            </button>
          {/if}
          <button type="button" class="button ghost" onclick={() => activity.dismissOffer()}>{t('library.dismiss')}</button>
        </span>
      </div>
    {/if}
  </div>
{/if}

<style>
  .activity {
    position: sticky;
    bottom: 0;
    z-index: 4;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .activity > :global(.banner) {
    margin: 0;
    box-shadow: var(--shadow);
  }

  .activity-label {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }

  .activity-label progress {
    flex: 1;
    min-width: 8rem;
  }

  .activity-heading {
    margin: 0 0 0.3rem;
    font-weight: 600;
  }

  .activity-list {
    margin: 0;
    padding-left: 1.2rem;
  }

  .activity-actions {
    display: flex;
    flex: none;
    gap: 0.5rem;
  }
</style>
