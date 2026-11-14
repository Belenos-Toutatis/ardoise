<script lang="ts">
  /**
   * « Sommaire du classeur » (SPEC §4.3): a binder's summary in the binder's order (its free order when it has one; the
   * notes of its sub-binders after its own, each in its own order), as the three choices of « Dans le sommaire » make it:
   * the names of the sub-binders as headings, a line per note, the bookmarks of the notes. A click, or Enter, on a note
   * opens it, on a bookmark opens the note at that bookmark (`onopen`). The same choices shape the outline and the summary
   * page of the binder's PDF. The field narrows the list to the lines that have the words typed (and what is above or
   * below them); the arrow keys walk through the lines as well as Tab does.
   */
  import { effectiveChoices, loadSummaryChoices, saveSummaryChoices } from '../../lib/binder-options';
  import { t, tn } from '../../lib/i18n.svelte';
  import { filterRows, type SummaryChoices } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { BookmarkRef, NoteSummary } from '../../lib/note';
  import Modal from '../Modal.svelte';
  import SummaryChoicesGroup from '../SummaryChoicesGroup.svelte';

  let {
    lib,
    binder,
    onopen,
    onclose,
  }: {
    lib: LibraryState;
    /** The binder, `/`-separated. */
    binder: string;
    /** A note line opens the note (no bookmark), a bookmark opens it at the bookmark. */
    onopen: (note: NoteSummary, bookmark?: BookmarkRef) => void;
    onclose: () => void;
  } = $props();

  let query = $state('');
  /** What the user chose, remembered on this computer; null until they choose (the defaults then depend on the binder). */
  let stored = $state<SummaryChoices | null>(loadSummaryChoices());
  let list = $state<HTMLElement>();

  const name = $derived(binder.split('/').pop() || binder);
  const hasBookmarks = $derived(lib.binderHasBookmarks(binder, true));
  const choices = $derived(effectiveChoices(stored, hasBookmarks));
  /** The notes are listed only because the binder has no bookmark and nothing was chosen. */
  const fallback = $derived(stored === null && !hasBookmarks);
  const rows = $derived(lib.binderSummaryRows(binder, true, choices));
  const shown = $derived(filterRows(rows, query));
  /** Lines that open something. */
  const opens = $derived(rows.filter((row) => row.kind !== 'binder').length);
  const hasNotes = $derived(lib.binderNotes(binder, true).length > 0);

  function choose(next: SummaryChoices) {
    stored = next;
    saveSummaryChoices(next);
  }

  /** Why the summary is empty, in a sentence: no note, no bookmark in the binder, or choices that leave nothing. */
  const emptyKey = $derived(
    !hasNotes
      ? 'export.binderEmpty'
      : choices.notes === 'none' && choices.bookmarks && !hasBookmarks
        ? 'library.summary.empty'
        : 'library.summary.emptyChoices',
  );

  /** Up and Down move from one line to the next (Tab does too). */
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
    <SummaryChoicesGroup {choices} binders={lib.hasSubBinders(binder)} {fallback} onchange={choose} />
    {#if rows.length === 0}
      <p class="muted" role="status" data-slot="summary-empty">{t(emptyKey)}</p>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus onclick={onclose}>{t('library.summary.close')}</button>
      </div>
    {:else}
      <p class="muted small" data-slot="summary-intro">{tn('library.summary.intro', opens)}</p>
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
        {#each shown as row, index (index)}
          {#if row.kind === 'binder'}
            <h3 class="summary-binder" style:--level={row.level} data-row="binder">{row.label}</h3>
          {:else if row.kind === 'note' && row.note}
            {@const note = row.note}
            <button
              type="button"
              class="summary-item summary-note"
              style:--level={row.level}
              data-row="note"
              data-note={note.path}
              aria-label={t('library.summary.openNote', { note: row.label })}
              onclick={() => onopen(note)}
            >
              <span class="summary-label">{row.label}</span>
            </button>
          {:else if row.note && row.bookmark}
            {@const note = row.note}
            {@const bookmark = row.bookmark}
            <button
              type="button"
              class="summary-item summary-bookmark"
              style:--level={row.level}
              data-row="bookmark"
              data-bookmark-id={bookmark.id}
              aria-label={t('library.summary.open', { label: bookmark.label, note: row.noteName ?? note.title })}
              onclick={() => onopen(note, bookmark)}
            >
              <span class="summary-flag" aria-hidden="true">⚑</span>
              <span class="summary-label">{row.label}</span>
            </button>
          {/if}
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
    max-height: 45vh;
    overflow-y: auto;
    padding-right: 0.25rem;
  }

  /* A sub-binder's name: a heading, indented by its level. */
  .summary-binder {
    margin: 0.7rem 0 0.2rem;
    padding-left: calc(var(--level, 0) * 1rem + 0.6rem);
    font-size: 1rem;
  }

  .summary-binder:first-child {
    margin-top: 0;
  }

  .summary-item {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    width: 100%;
    padding: 0.4rem 0.6rem;
    padding-left: calc(var(--level, 0) * 1rem + 0.6rem);
    border: 1px solid transparent;
    border-radius: 8px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .summary-note {
    font-weight: 600;
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
