<script lang="ts">
  /**
   * Left column: views (all notes, favourites), the tree of binders (drop targets for dragged
   * notes; each one can be renamed or deleted from its menu), tags (each with its colour), trash,
   * the Inkspace import, then the tablet block with the entry that opens the tablet test.
   */
  import { onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { t } from '../../lib/i18n.svelte';
  import { ancestorsOf, isInBinder, sameView, type BinderNode, type LibraryView } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import BinderEditor from './BinderEditor.svelte';
  import Menu from './Menu.svelte';
  import TabletStatus from './TabletStatus.svelte';
  import TagColorPicker from './TagColorPicker.svelte';

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
  /** The menu of each binder row, to open it with the right click. */
  const menus: Record<string, { show: () => void } | null | undefined> = {};
  /** The binder being renamed or whose deletion awaits confirmation. */
  let editing = $state<{ path: string; mode: 'rename' | 'delete' } | null>(null);

  // The tree opens down to the binder being viewed.
  $effect(() => {
    const view = lib.view;
    if (view.kind === 'binder') for (const path of ancestorsOf(view.binder)) expanded.add(path);
  });

  // The tag colours follow the library while the sidebar shows them.
  onMount(() => lib.tagColors.start());

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

  let nav = $state<HTMLElement | null>(null);

  /** Gives the focus back to the sidebar's entry of `binder`, or to « Toutes les notes ». */
  async function focusEntry(binder: string | null) {
    await tick();
    const items = [...(nav?.querySelectorAll<HTMLElement>('.nav-item') ?? [])];
    const found = binder === null ? undefined : items.find((item) => item.dataset.binder === binder);
    (found ?? items.find((item) => item.dataset.view === 'all'))?.focus();
  }

  /** The binder was renamed: what was open under its old path stays open. */
  function renamed(from: string, to: string) {
    editing = null;
    for (const path of [...expanded]) {
      if (!isInBinder(path, from)) continue;
      expanded.delete(path);
      expanded.add(to + path.slice(from.length));
    }
    void focusEntry(to);
  }

  function closeEditing(path: string) {
    editing = null;
    void focusEntry(path);
  }

  /**
   * Where dragged notes can be dropped in this column: a binder (they move there), « Favoris » (they become
   * favourites) or a tag (they get it). The last two were asked by the user on 2026-11-13.
   */
  type DropPlace = { kind: 'binder'; path: string } | { kind: 'favorites' } | { kind: 'tag'; tag: string };
  const placeKey = (place: DropPlace) => (place.kind === 'binder' ? `b:${place.path}` : place.kind === 'tag' ? `t:${place.tag}` : 'f');
  const binderPlace = (path: string): DropPlace => ({ kind: 'binder', path });

  function dragover(event: DragEvent, place: DropPlace) {
    if (!lib.dragging) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    dropTarget = placeKey(place);
  }

  function drop(event: DragEvent, place: DropPlace) {
    if (!lib.dragging) return;
    event.preventDefault();
    dropTarget = null;
    if (place.kind === 'binder') void lib.dropOn(place.path);
    else if (place.kind === 'favorites') void lib.dropOnFavorites();
    else void lib.dropOnTag(place.tag);
  }

  function dragleave(place: DropPlace) {
    if (dropTarget === placeKey(place)) dropTarget = null;
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
        class:drop={dropTarget === placeKey(binderPlace(node.path))}
        data-binder={node.path}
        aria-current={current(view)}
        onclick={() => lib.setView(view)}
        ondragover={(event) => dragover(event, binderPlace(node.path))}
        ondragenter={(event) => dragover(event, binderPlace(node.path))}
        ondragleave={() => dragleave(binderPlace(node.path))}
        ondrop={(event) => drop(event, binderPlace(node.path))}
        oncontextmenu={(event) => {
          event.preventDefault();
          menus[node.path]?.show();
        }}
      >
        <span class="nav-label">{node.name}</span>
        <span class="count">{node.count}</span>
      </button>
      <span class="row-tool binder-menu">
        <Menu
          bind:this={menus[node.path]}
          label={t('library.binder.menu', { name: node.name })}
          items={[
            { id: 'rename', label: t('library.binder.rename'), onselect: () => (editing = { path: node.path, mode: 'rename' }) },
            { id: 'summary', label: t('library.binder.summary'), onselect: () => lib.openSummary(node.path) },
            {
              id: 'export-pdf',
              label: t('library.binder.exportPdf'),
              disabled: node.count === 0,
              hint: t('library.binder.emptyHint'),
              onselect: () => lib.openBinderDialog('export', node.path),
            },
            {
              id: 'print',
              label: t('library.binder.print'),
              disabled: node.count === 0,
              hint: t('library.binder.emptyHint'),
              onselect: () => lib.openBinderDialog('print', node.path),
            },
            { id: 'delete', label: t('library.binder.delete'), danger: true, onselect: () => (editing = { path: node.path, mode: 'delete' }) },
          ]}
        />
      </span>
      <button
        type="button"
        class="icon-button add-sub"
        aria-label={t('library.newSubBinder', { name: node.name })}
        onclick={() => startCreating(node.path)}
      >
        <span aria-hidden="true">+</span>
      </button>
    </div>
    {#if editing?.path === node.path}
      {#key editing.mode}
        <BinderEditor
          {lib}
          {node}
          mode={editing.mode}
          onrenamed={renamed}
          ondeleted={() => {
            editing = null;
            void focusEntry(null);
          }}
          onclose={() => closeEditing(node.path)}
        />
      {/key}
    {/if}
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

<nav class="sidebar" aria-label={t('library.sidebarLabel')} bind:this={nav}>
  <ul class="nav-list">
    <li>
      <button
        type="button"
        class="nav-item"
        class:drop={dropTarget === placeKey(binderPlace(''))}
        data-view="all"
        aria-current={current({ kind: 'all' })}
        onclick={() => lib.setView({ kind: 'all' })}
        ondragover={(event) => dragover(event, binderPlace(''))}
        ondragenter={(event) => dragover(event, binderPlace(''))}
        ondragleave={() => dragleave(binderPlace(''))}
        ondrop={(event) => drop(event, binderPlace(''))}
      >
        <span class="nav-label">{t('library.all')}</span>
        <span class="count">{notes.length}</span>
      </button>
    </li>
    <li>
      <button
        type="button"
        class="nav-item"
        class:drop={dropTarget === 'f'}
        data-view="favorites"
        ondragover={(event) => dragover(event, { kind: 'favorites' })}
        ondragenter={(event) => dragover(event, { kind: 'favorites' })}
        ondragleave={() => dragleave({ kind: 'favorites' })}
        ondrop={(event) => drop(event, { kind: 'favorites' })}
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
          {@const color = lib.tagColors.of(entry.tag)}
          <li>
            <div class="nav-row">
              <button
                type="button"
                class="nav-item"
                class:drop={dropTarget === placeKey({ kind: 'tag', tag: entry.tag })}
                data-tag={entry.tag}
                ondragover={(event) => dragover(event, { kind: 'tag', tag: entry.tag })}
                ondragenter={(event) => dragover(event, { kind: 'tag', tag: entry.tag })}
                ondragleave={() => dragleave({ kind: 'tag', tag: entry.tag })}
                ondrop={(event) => drop(event, { kind: 'tag', tag: entry.tag })}
                data-tag-color={color}
                aria-current={current({ kind: 'tag', tag: entry.tag })}
                onclick={() => lib.setView({ kind: 'tag', tag: entry.tag })}
              >
                <span class="nav-label">
                  {#if color}<span class="tag-dot" aria-hidden="true"></span>{:else}#{/if}
                  {entry.tag}
                </span>
                <span class="count">{entry.count}</span>
              </button>
              <span class="row-tool"><TagColorPicker tag={entry.tag} colors={lib.tagColors} /></span>
            </div>
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
