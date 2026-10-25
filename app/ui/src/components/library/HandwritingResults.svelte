<script lang="ts">
  /**
   * Under the search field, while searching: the notes the handwriting search found, each with a
   * short excerpt of its recognised text that shows why it matched, and whether the handwriting
   * is searched at all (indexing under way, no recognizer on this PC).
   */
  import { searchNotice } from '../../lib/handwriting';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { displayTitle, handwritingMatches } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { NoteSummary } from '../../lib/note';

  let { lib, onopen }: { lib: LibraryState; onopen: (note: NoteSummary) => void } = $props();

  const matches = $derived(handwritingMatches(lib.visible, lib.hits));
  const notice = $derived(searchNotice(lib.handwriting));
</script>

{#if notice || matches.length > 0}
  <section class="handwriting-results" aria-label={t('handwriting.search.label')}>
    {#if notice}
      <p class="muted small handwriting-notice" role="status">{t(notice.key, notice.values)}</p>
    {/if}
    {#if matches.length > 0}
      <p class="small handwriting-found">{t('handwriting.search.found', { count: matches.length })}</p>
      <ul class="handwriting-excerpts">
        {#each matches as { note, hit } (note.path)}
          <li>
            <button
              type="button"
              class="handwriting-excerpt"
              data-path={note.path}
              title={t('handwriting.search.open', { title: displayTitle(note, currentLocale()) })}
              onclick={() => onopen(note)}
            >
              <span class="excerpt-title">{displayTitle(note, currentLocale())}</span>
              <span class="excerpt-text">{hit.excerpt.before}<mark>{hit.excerpt.word}</mark>{hit.excerpt.after}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .handwriting-results {
    margin: 0 0 0.9rem;
  }

  .handwriting-notice,
  .handwriting-found {
    margin: 0 0 0.35rem;
  }

  .handwriting-excerpts {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 9.5rem;
    overflow-y: auto;
    display: grid;
    gap: 0.2rem;
  }

  .handwriting-excerpt {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    width: 100%;
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .handwriting-excerpt:hover,
  .handwriting-excerpt:focus-visible {
    border-color: var(--accent);
  }

  .excerpt-title {
    font-weight: 600;
    white-space: nowrap;
  }

  .excerpt-text {
    font-style: italic;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    font-style: normal;
    font-weight: 600;
    padding: 0 0.1em;
    border-radius: 3px;
  }
</style>
