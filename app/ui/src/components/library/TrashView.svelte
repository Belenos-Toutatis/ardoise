<script lang="ts">
  /** The trash: restore a note, delete it for good, or empty the trash (both ask to confirm). */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate, originalBinder } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { TrashEntry } from '../../lib/note';

  let { lib }: { lib: LibraryState } = $props();

  /** Id of the entry whose deletion awaits confirmation, or 'all'. */
  let confirming = $state<string | null>(null);
  /** The entries shown when "empty the trash" was asked: only those are deleted. */
  let confirmed = $state<string[]>([]);

  const locale = $derived(currentLocale());
  const entries = $derived(lib.trash);

  function titleOf(entry: TrashEntry): string {
    return entry.title?.trim() || t('library.trashView.untitled');
  }

  function placeOf(entry: TrashEntry): string {
    return originalBinder(entry.original) || t('library.root');
  }

  async function purge(id: string) {
    confirming = null;
    await lib.purge(id);
  }

  function askEmpty() {
    confirmed = entries.map((entry) => entry.id);
    confirming = 'all';
  }

  async function emptyAll() {
    confirming = null;
    await lib.emptyTrash(confirmed);
  }
</script>

<section class="library-section" aria-labelledby="trash-heading">
  <header class="section-header">
    <h2 id="trash-heading">{t('library.trashView.title')}</h2>
    <span class="muted count">{entries.length}</span>
    {#if entries.length > 0 && confirming !== 'all'}
      <button type="button" class="button danger push" onclick={askEmpty}>
        {t('library.trashView.emptyAll')}
      </button>
    {/if}
  </header>

  <p class="muted small">{t('library.trashView.intro')}</p>

  {#if confirming === 'all'}
    <div class="confirm" role="alertdialog" aria-label={t('library.trashView.emptyAll')}>
      <p>{t('library.trashView.confirmEmpty', { count: confirmed.length })}</p>
      <div class="actions">
        <button type="button" class="button primary danger-fill" onclick={emptyAll}>
          {t('library.trashView.confirm')}
        </button>
        <button type="button" class="button" onclick={() => (confirming = null)}>{t('library.cancel')}</button>
      </div>
    </div>
  {/if}

  {#if entries.length === 0}
    <div class="empty-state"><p class="lead">{t('library.trashView.empty')}</p></div>
  {:else}
    <table class="trash-table" aria-label={t('library.trashView.list')}>
      <thead>
        <tr>
          <th scope="col">{t('library.trashView.name')}</th>
          <th scope="col">{t('library.trashView.from')}</th>
          <th scope="col">{t('library.trashView.deleted')}</th>
          <th scope="col"><span class="visually-hidden">{t('library.trashView.actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each entries as entry (entry.id)}
          <tr data-id={entry.id}>
            <th scope="row">{titleOf(entry)}</th>
            <td>{placeOf(entry)}</td>
            <td>{formatDate(entry.deleted, locale)}</td>
            <td class="row-actions">
              {#if confirming === entry.id}
                <span class="muted small">{t('library.trashView.confirmPurge', { title: titleOf(entry) })}</span>
                <button type="button" class="button danger" onclick={() => purge(entry.id)}>
                  {t('library.trashView.confirm')}
                </button>
                <button type="button" class="button ghost" onclick={() => (confirming = null)}>
                  {t('library.cancel')}
                </button>
              {:else}
                <button type="button" class="button" onclick={() => lib.restore(entry.id)}>
                  {t('library.trashView.restore')}
                </button>
                <button type="button" class="button danger" onclick={() => (confirming = entry.id)}>
                  {t('library.trashView.purge')}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>
