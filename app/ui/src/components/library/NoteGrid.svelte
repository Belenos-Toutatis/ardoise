<script lang="ts">
  /** The note grid of the current view, with its heading, selection bar, empty states and problem list. */
  import { onMount, tick } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import { tablet } from '../../lib/tablet-state.svelte';
  import type { NoteSummary } from '../../lib/note';
  import { LIBRARY_LISTED_MARK, markWhenDrawn } from '../../lib/perf';
  import HandwritingResults from './HandwritingResults.svelte';
  import NoteCard from './NoteCard.svelte';
  import SelectionBar from './SelectionBar.svelte';

  let {
    lib,
    onopen,
    onnew,
    onresume,
  }: {
    lib: LibraryState;
    onopen: (note: NoteSummary) => void;
    onnew: () => void;
    /** « Reprendre en mode direct » in a card's menu (M12). */
    onresume?: (note: NoteSummary) => void;
  } = $props();

  // The menu of a card offers « Reprendre en mode direct » only with a registered tablet.
  onMount(() => tablet.start());

  let grid = $state<HTMLElement | null>(null);
  let activeIndex = $state(0);

  const notes = $derived(lib.visible);
  const searching = $derived(lib.query.trim() !== '');
  const heading = $derived.by(() => {
    const view = lib.view;
    switch (view.kind) {
      case 'favorites':
        return t('library.favorites');
      case 'binder':
        return view.binder.split('/').pop() ?? view.binder;
      case 'tag':
        return `# ${view.tag}`;
      default:
        return t('library.all');
    }
  });
  const emptyKey = $derived.by(() => {
    const kind = lib.view.kind;
    return kind === 'favorites' ? 'library.empty.favorites' : kind === 'binder' ? 'library.empty.binder' : 'library.empty.tag';
  });
  const problems = $derived(lib.listing?.problems ?? []);
  // Keeps the Tab stop inside the grid when notes disappear.
  const tabStop = $derived(Math.min(activeIndex, Math.max(notes.length - 1, 0)));

  // The end of the start-up measure of `cargo xtask perf`: the cards are on screen.
  $effect(() => {
    if (grid && notes.length > 0) markWhenDrawn(LIBRARY_LISTED_MARK);
  });

  function cards(): HTMLElement[] {
    return [...(grid?.querySelectorAll<HTMLElement>('.card-open[data-index]') ?? [])];
  }

  function navigate(event: KeyboardEvent, index: number) {
    const elements = cards();
    const count = notes.length;
    if (count === 0) return;
    const first = elements[0]?.offsetTop ?? 0;
    const columns = Math.max(1, elements.filter((el) => el.offsetTop === first).length);
    let next = index;
    switch (event.key) {
      case 'ArrowLeft':
        next = index - 1;
        break;
      case 'ArrowRight':
        next = index + 1;
        break;
      case 'ArrowUp':
        next = index - columns;
        break;
      case 'ArrowDown':
        next = index + columns;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = count - 1;
        break;
    }
    if (next < 0 || next >= count) next = event.key === 'ArrowUp' || event.key === 'ArrowDown' ? index : Math.min(Math.max(next, 0), count - 1);
    event.preventDefault();
    if (next === index) return;
    if (event.shiftKey) {
      if (!lib.hasAnchor) lib.toggle(notes[index].path);
      lib.selectRange(notes[next].path);
    }
    activeIndex = next;
    elements[next]?.focus();
  }

  async function focusPath(path: string) {
    await tick();
    const found = [...(grid?.querySelectorAll<HTMLElement>('.note-card') ?? [])].find((el) => el.dataset.path === path);
    found?.querySelector<HTMLElement>('.card-open')?.focus();
  }
</script>

<section class="library-section" aria-labelledby="library-heading">
  <header class="section-header">
    <h2 id="library-heading">{heading}</h2>
    <span class="muted count">{notes.length}</span>
  </header>

  {#if problems.length > 0}
    <!-- Unreadable files stay on the disk untouched; the reason is the library error's sentence
         (a note from a newer Ardoise says to update it). -->
    <div class="banner error library-problems" role="status" data-slot="problems">
      <p>
        <strong>{t('library.problems', { count: problems.length })}</strong>
        {t('library.problemsHint')}
      </p>
      <ul>
        {#each problems as problem (problem.path)}
          <li>
            <code>{problem.path}</code>
            <span>{errorMessage({ code: problem.code, message: problem.message })}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if lib.selected.length > 0}
    <SelectionBar {lib} />
  {/if}

  {#if searching}
    <HandwritingResults {lib} {onopen} />
  {/if}

  {#if lib.listing === null}
    <p class="loading">{t('common.loading')}</p>
  {:else if notes.length === 0}
    <div class="empty-state">
      {#if searching}
        <p class="lead">{t('library.empty.search', { query: lib.query.trim() })}</p>
      {:else if lib.view.kind === 'all'}
        <p class="lead">{t('home.empty')}</p>
        <p class="muted">{t('home.emptyHint')}</p>
        <button type="button" class="button primary" onclick={onnew}>{t('library.newNote')}</button>
      {:else}
        <p class="lead">{t(emptyKey)}</p>
      {/if}
    </div>
  {:else}
    {#if lib.canReorder}
      <p class="muted small order-help" data-slot="order-help">{t('library.order.help')}</p>
    {:else if lib.effectiveOrder === 'binder' && searching}
      <p class="muted small order-help" data-slot="order-help">{t('library.order.searching')}</p>
    {/if}
    <ul
      class="note-grid"
      class:selecting={lib.selected.length > 0}
      class:reordering={lib.canReorder}
      bind:this={grid}
      aria-label={t('library.notes')}
    >
      {#each notes as note, index (note.path)}
        <NoteCard
          {note}
          {lib}
          {index}
          active={index === tabStop}
          {onopen}
          onnavigate={navigate}
          onfocuscard={(i) => (activeIndex = i)}
          onrenamed={focusPath}
          onmoved={focusPath}
          {onresume}
        />
      {/each}
    </ul>
  {/if}

  <!-- What the keyboard's moves say to a screen reader. -->
  <p class="visually-hidden" role="status" aria-live="polite" data-slot="order-announcement">{lib.announcement}</p>
</section>
