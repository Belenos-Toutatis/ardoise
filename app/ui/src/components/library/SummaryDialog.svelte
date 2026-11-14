<script lang="ts">
  /**
   * « Sommaire du classeur » (SPEC §4.3): the bookmarks of a binder's notes, note by note, in the binder's order (its free
   * order when it has one; the notes of its sub-binders after its own, each in its own order). A click, or Enter, opens
   * the note at that bookmark (`onopen`). Notes without a bookmark are not listed. The field narrows the list to the
   * bookmarks that have the words typed (or to the notes whose title has them); the arrow keys walk through the bookmarks
   * as well as Tab does.
   */
  import { loadShowDates, saveShowDates } from '../../lib/binder-options';
  import { t, tn } from '../../lib/i18n.svelte';
  import { filterSummary } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { BookmarkRef, NoteSummary } from '../../lib/note';
  import Modal from '../Modal.svelte';

  let {
    lib,
    binder,
    onopen,
    onclose,
  }: {
    lib: LibraryState;
    /** The binder, `/`-separated. */
    binder: string;
    onopen: (note: NoteSummary, bookmark: BookmarkRef) => void;
    onclose: () => void;
  } = $props();

  let query = $state('');
  /** The dates name the untitled notes (else « Note sans titre »): the choice of the binder's PDF too, remembered. */
  let showDates = $state(loadShowDates());
  let list = $state<HTMLElement>();

  const name = $derived(binder.split('/').pop() || binder);
  const summary = $derived(lib.binderSummary(binder));
  const shown = $derived(filterSummary(summary, query));
  const titles = $derived(lib.binderTitles(binder, showDates));
  /** A note's name in the list: its title, or its date or « Note sans titre » (`showDates`). */
  const titleOf = (note: NoteSummary) => titles.get(note.path) ?? note.title;
  const total = $derived(summary.reduce((sum, entry) => sum + entry.bookmarks.length, 0));

  /** Up and Down move from one bookmark to the next (Tab does too). */
  function walk(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const items = [...(list?.querySelectorAll<HTMLElement>('.summary-item') ?? [])];
    const at = items.indexOf(event.target as HTMLElement);
    if (at < 0) return;
    const next = items[at + (event.key === 'ArrowDown' ? 1 : -1)];
    if (!next) return;
    event.preventDefault();
    next.focus();
  }
</script>

<Modal title={t('library.summary.title', { name })} {onclose}>
  <div class="binder-summary" data-dialog="binder-summary">
    {#if total === 0}
      <p class="muted" data-slot="summary-empty">{t('library.summary.empty')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus onclick={onclose}>{t('library.summary.close')}</button>
      </div>
    {:else}
      <p class="muted small">{tn('library.summary.intro', total)}</p>
      <label class="check" data-slot="summary-dates">
        <input
          type="checkbox"
          data-option="show-dates"
          bind:checked={showDates}
          onchange={() => saveShowDates(showDates)}
        />
        {t('export.showDates')}
      </label>
      <p class="muted small">{t('export.showDatesHint')}</p>
      <input
        class="text-input"
        type="search"
        data-autofocus
        aria-label={t('library.summary.filter')}
        placeholder={t('library.summary.filterPlaceholder')}
        bind:value={query}
      />
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="summary-scroll" bind:this={list} onkeydown={walk} role="group" aria-label={t('library.summary.list')}>
        {#each shown as entry (entry.note.path)}
          <section class="summary-note" data-note={entry.note.path}>
            <h3 class="summary-note-title">
              {titleOf(entry.note)}
              {#if entry.below.length > 0}
                <span class="muted small summary-binder">{entry.below.join(' / ')}</span>
              {/if}
            </h3>
            <ul class="summary-bookmarks">
              {#each entry.bookmarks as bookmark (bookmark.id)}
                <li>
                  <button
                    type="button"
                    class="summary-item"
                    data-bookmark-id={bookmark.id}
                    aria-label={t('library.summary.open', { label: bookmark.label, note: titleOf(entry.note) })}
                    onclick={() => onopen(entry.note, bookmark)}
                  >
                    <span class="summary-flag" aria-hidden="true">⚑</span>
                    <span class="summary-label">{bookmark.label}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {:else}
          <p class="muted" data-slot="summary-none">{t('library.summary.noMatch', { query })}</p>
        {/each}
      </div>
      <div class="actions">
        <button type="button" class="button" data-action="summary-close" onclick={onclose}>{t('library.summary.close')}</button>
      </div>
    {/if}
  </div>
</Modal>

<style>
  .binder-summary {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .summary-scroll {
    max-height: 55vh;
    overflow-y: auto;
    padding-right: 0.25rem;
  }

  .summary-note + .summary-note {
    margin-top: 0.9rem;
  }

  .summary-note-title {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin: 0 0 0.25rem;
    font-size: 1rem;
  }

  .summary-bookmarks {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .summary-item {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    width: 100%;
    padding: 0.4rem 0.6rem;
    border: 1px solid transparent;
    border-radius: 8px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .summary-item:hover {
    background: var(--surface-2);
  }

  .summary-item:focus-visible {
    border-color: var(--accent);
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .summary-flag {
    color: var(--warn);
  }

  .summary-label {
    overflow-wrap: anywhere;
  }
</style>
