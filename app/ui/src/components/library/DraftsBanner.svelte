<script lang="ts">
  /**
   * The drafts left by an earlier run of Ardoise, closed before their notes were saved (M10,
   * `lib/editor/drafts.ts`): each can be opened (the editor then offers it), recovered into a new
   * note when its note is gone, or discarded (asked once more). Nothing shows when there is none.
   */
  import { api, errorMessage } from '../../lib/api';
  import type { DraftSummary } from '../../lib/editor/drafts';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { NoteSummary } from '../../lib/note';

  let { lib, onopen }: { lib: LibraryState; onopen: (path: string) => void } = $props();

  let drafts = $state.raw<DraftSummary[]>([]);
  let confirming = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** Bumped by each load: only the latest answer counts. */
  let request = 0;

  async function load() {
    const mine = ++request;
    try {
      const found = await api.draftList();
      if (mine === request) drafts = found;
    } catch {
      // No library, or nothing readable: nothing to offer.
      if (mine === request) drafts = [];
    }
  }

  // Again whenever the library changes (a draft restored and saved goes away).
  $effect(() => {
    void lib.listing;
    void load();
  });

  /** The note a draft belongs to: the one at its path, else the one with its id (renamed elsewhere). */
  function noteOf(draft: DraftSummary): NoteSummary | null {
    const notes = lib.notes;
    return notes.find((n) => n.id === draft.id && n.path === draft.path) ?? notes.find((n) => n.id === draft.id) ?? null;
  }

  function when(draft: DraftSummary): string {
    return new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'long', timeStyle: 'short' }).format(new Date(draft.savedAt));
  }

  function titleOf(draft: DraftSummary): string {
    return draft.title.trim() || t('editor.untitled');
  }

  async function restoreCopy(draft: DraftSummary) {
    busy = true;
    error = null;
    try {
      const created = await api.draftRestoreCopy(draft.id, t('drafts.copyTitle', { title: titleOf(draft) }));
      await lib.refresh();
      onopen(created.path);
    } catch (problem) {
      error = errorMessage(problem);
      void load();
    } finally {
      busy = false;
    }
  }

  async function discard(draft: DraftSummary) {
    busy = true;
    error = null;
    try {
      await api.draftDiscard(draft.id);
    } catch (problem) {
      error = errorMessage(problem);
    } finally {
      confirming = null;
      busy = false;
      void load();
    }
  }
</script>

{#if drafts.length > 0}
  <div class="banner notice drafts-banner" role="status" data-slot="drafts-banner">
    <strong>{t('drafts.library.title', { count: drafts.length })}</strong>
    {#if error}
      <p class="error" role="alert">{t('drafts.library.failed', { message: error })}</p>
    {/if}
    <ul>
      {#each drafts as draft (draft.id)}
        {@const note = noteOf(draft)}
        <li data-draft={draft.id}>
          <span class="draft-name">
            {titleOf(draft)}
            <span class="muted small">— {note ? t('drafts.library.saved', { date: when(draft) }) : t('drafts.library.missing')}</span>
          </span>
          {#if confirming === draft.id}
            <span class="muted small">{t('drafts.library.discardConfirm', { title: titleOf(draft) })}</span>
            <button type="button" class="button danger" data-action="drafts-discard-yes" disabled={busy} onclick={() => discard(draft)}>
              {t('drafts.library.discardYes')}
            </button>
            <button type="button" class="button" disabled={busy} onclick={() => (confirming = null)}>{t('drafts.library.discardNo')}</button>
          {:else}
            {#if note}
              <button type="button" class="button primary" data-action="drafts-open" disabled={busy} onclick={() => onopen(note.path)}>
                {t('drafts.library.open')}
              </button>
            {:else}
              <button type="button" class="button primary" data-action="drafts-copy" disabled={busy} onclick={() => restoreCopy(draft)}>
                {t('drafts.library.restoreCopy')}
              </button>
            {/if}
            <button type="button" class="button ghost" data-action="drafts-discard" disabled={busy} onclick={() => (confirming = draft.id)}>
              {t('drafts.library.discard')}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .drafts-banner {
    margin: 0 0 1rem;
  }

  ul {
    list-style: none;
    margin: 0.5rem 0 0;
    padding: 0;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0;
  }

  .draft-name {
    flex: 1 1 12rem;
  }
</style>
