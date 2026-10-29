<script lang="ts">
  /**
   * Left column: views (all notes, favourites), the tree of binders (drop targets for dragged
   * notes), tags, trash, the Inkspace import, then the tablet block with the entry that opens the
   * tablet test.
   */
  import { tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { t } from '../../lib/i18n.svelte';
  import { ancestorsOf, sameView, type BinderNode, type LibraryView } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import TabletStatus from './TabletStatus.svelte';

  let {
    lib,
    testOpen,
    ontest,
    tabletOpen = false,
    ontablet = () => {},
    oninkspace = () => {},
  }: {
    lib: LibraryState;
    testOpen: boolean;
    ontest: () => void;
    /** The tablet panel is shown. */
    tabletOpen?: boolean;
    /** Opens or closes the tablet panel. */
    ontablet?: () => void;
    /** Opens « Importer depuis Inkspace ». */
    oninkspace?: () => void;
  } = $props();

  const expanded = new SvelteSet<string>();
  /** Where a new binder is being named: the parent's path (`''` for the root), or null. */
  let creatingIn = $state<string | null>(null);
  let newName = $state('');
  /** Drop target under the pointer: a binder path, or null. */
  let dropTarget = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

  // The tree opens down to the binder being viewed.
  $effect(() => {
    const view = lib.view;
    if (view.kind === 'binder') for (const path of ancestorsOf(view.binder)) expanded.add(path);
  });

  const notes = $derived(lib.notes);
  const favoriteCount = $derived(notes.filter((n) => n.favorite).length);
  const loaded = $derived(lib.listing !== null);

  function current(view: LibraryView): 'true' | undefined {
    return sameView(view, lib.view) ? 'true' : undefined;
  }

  function toggleExpanded(path: string) {
    if (expanded.has(path)) expanded.delete(path);
    else expanded.add(path);
  }

  async function startCreating(parent: string) {
    creatingIn = parent;
    newName = '';
    if (parent) expanded.add(parent);
    await tick();
    nameInput?.focus();
  }

  async function submitBinder(event: SubmitEvent) {
    event.preventDefault();
    const parent = creatingIn ?? '';
    if (await lib.createBinder(parent, newName)) {
      creatingIn = null;
      if (parent) expanded.add(parent);
    }
  }

  function cancelCreating(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      creatingIn = null;
    }
  }

  function dragover(event: DragEvent, binder: string) {
    if (!lib.dragging) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    dropTarget = binder;
  }

  function drop(event: DragEvent, binder: string) {
    if (!lib.dragging) return;
    event.preventDefault();
    dropTarget = null;
    void lib.dropOn(binder);
  }

  function dragleave(binder: string) {
    if (dropTarget === binder) dropTarget = null;
  }
</script>

{#snippet creator(parent: string)}
  <form class="inline-form binder-form" onsubmit={submitBinder}>
    <input
      bind:this={nameInput}
      bind:value={newName}
      type="text"
      aria-label={t('library.binderName')}
      placeholder={t('library.binderName')}
      onkeydown={cancelCreating}
    />
    <button type="submit" class="button primary" disabled={!newName.trim()}>{t('library.create')}</button>
    <button type="button" class="button ghost" onclick={() => (creatingIn = null)} data-parent={parent}>
      {t('library.cancel')}
    </button>
  </form>
{/snippet}

{#snippet branch(node: BinderNode)}
  {@const view = { kind: 'binder', binder: node.path } as LibraryView}
  {@const open = expanded.has(node.path)}
  <li>
    <div class="nav-row" style:padding-left="{node.depth * 0.9}rem">
      {#if node.children.length > 0}
        <button
          type="button"
          class="twisty"
          aria-expanded={open}
          aria-label={open ? t('library.collapse', { name: node.name }) : t('library.expand', { name: node.name })}
          onclick={() => toggleExpanded(node.path)}
        >
          <span aria-hidden="true">{open ? '▾' : '▸'}</span>
        </button>
      {:else}
        <span class="twisty-space"></span>
      {/if}
      <button
        type="button"
        class="nav-item"
        class:drop={dropTarget === node.path}
        data-binder={node.path}
        aria-current={current(view)}
        onclick={() => lib.setView(view)}
        ondragover={(event) => dragover(event, node.path)}
        ondragenter={(event) => dragover(event, node.path)}
        ondragleave={() => dragleave(node.path)}
        ondrop={(event) => drop(event, node.path)}
      >
        <span class="nav-label">{node.name}</span>
        <span class="count">{node.count}</span>
      </button>
      <button
        type="button"
        class="icon-button add-sub"
        aria-label={t('library.newSubBinder', { name: node.name })}
        onclick={() => startCreating(node.path)}
      >
        <span aria-hidden="true">+</span>
      </button>
    </div>
    {#if creatingIn === node.path}
      {@render creator(node.path)}
    {/if}
    {#if open && node.children.length > 0}
      <ul class="nav-list">
        {#each node.children as child (child.path)}
          {@render branch(child)}
        {/each}
      </ul>
    {/if}
  </li>
{/snippet}

<nav class="sidebar" aria-label={t('library.sidebarLabel')}>
  <ul class="nav-list">
    <li>
      <button
        type="button"
        class="nav-item"
        class:drop={dropTarget === ''}
        data-view="all"
        aria-current={current({ kind: 'all' })}
        onclick={() => lib.setView({ kind: 'all' })}
        ondragover={(event) => dragover(event, '')}
        ondragenter={(event) => dragover(event, '')}
        ondragleave={() => dragleave('')}
        ondrop={(event) => drop(event, '')}
      >
        <span class="nav-label">{t('library.all')}</span>
        <span class="count">{notes.length}</span>
      </button>
    </li>
    <li>
      <button
        type="button"
        class="nav-item"
        data-view="favorites"
        aria-current={current({ kind: 'favorites' })}
        onclick={() => lib.setView({ kind: 'favorites' })}
      >
        <span class="nav-label">{t('library.favorites')}</span>
        <span class="count">{favoriteCount}</span>
      </button>
    </li>
  </ul>

  <section class="nav-section" aria-labelledby="nav-binders">
    <div class="nav-heading">
      <h3 id="nav-binders">{t('library.binders')}</h3>
      <button type="button" class="icon-button" aria-label={t('library.newBinder')} onclick={() => startCreating('')}>
        <span aria-hidden="true">+</span>
      </button>
    </div>
    {#if creatingIn === ''}
      {@render creator('')}
    {/if}
    {#if lib.tree.length > 0}
      <ul class="nav-list">
        {#each lib.tree as node (node.path)}
          {@render branch(node)}
        {/each}
      </ul>
    {:else if loaded}
      <p class="muted small nav-empty">{t('library.noBinders')}</p>
    {/if}
  </section>

  <section class="nav-section" aria-labelledby="nav-tags">
    <div class="nav-heading"><h3 id="nav-tags">{t('library.tags')}</h3></div>
    {#if (lib.listing?.tags.length ?? 0) > 0}
      <ul class="nav-list">
        {#each lib.listing?.tags ?? [] as entry (entry.tag)}
          <li>
            <button
              type="button"
              class="nav-item"
              data-tag={entry.tag}
              aria-current={current({ kind: 'tag', tag: entry.tag })}
              onclick={() => lib.setView({ kind: 'tag', tag: entry.tag })}
            >
              <span class="nav-label"># {entry.tag}</span>
              <span class="count">{entry.count}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if loaded}
      <p class="muted small nav-empty">{t('library.noTags')}</p>
    {/if}
  </section>

  <ul class="nav-list nav-trash">
    <li>
      <button
        type="button"
        class="nav-item"
        data-view="trash"
        aria-current={current({ kind: 'trash' })}
        onclick={() => lib.setView({ kind: 'trash' })}
      >
        <span class="nav-label">{t('library.trash')}</span>
        <span class="count">{lib.trash.length}</span>
      </button>
    </li>
  </ul>

  <ul class="nav-list nav-import">
    <li>
      <button type="button" class="nav-item" data-action="import-inkspace" onclick={oninkspace}>
        <span class="nav-label">{t('inkspace.entry')}</span>
      </button>
    </li>
  </ul>

  <div class="sidebar-foot">
    <TabletStatus open={tabletOpen} onopen={ontablet} />
    <button type="button" class="button" aria-expanded={testOpen} onclick={ontest}>{t('diagnostic.open')}</button>
  </div>
</nav>
