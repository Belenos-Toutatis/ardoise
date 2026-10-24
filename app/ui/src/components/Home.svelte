<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppState } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { SORT_ORDERS, type SortOrder } from '../lib/library-logic';
  import { LibraryState } from '../lib/library-state.svelte';
  import type { NoteSummary } from '../lib/note';
  import Diagnostic from './Diagnostic.svelte';
  import Editor from './editor/Editor.svelte';
  import NoteGrid from './library/NoteGrid.svelte';
  import Sidebar from './library/Sidebar.svelte';
  import TrashView from './library/TrashView.svelte';
  import Logo from './Logo.svelte';
  import Settings from './Settings.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  const lib = new LibraryState();

  // One side panel at a time.
  let panel = $state<'settings' | 'diagnostic' | null>(null);
  /** Path of the note being edited; the editor then takes the whole window. */
  let openPath = $state<string | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);

  const inTrash = $derived(lib.view.kind === 'trash');

  onMount(() => {
    // Started with `--diagnostic`: the tablet test is shown right away.
    if (app.openDiagnostic) panel = 'diagnostic';
    return lib.start();
  });

  function toggle(name: 'settings' | 'diagnostic') {
    panel = panel === name ? null : name;
  }

  function open(note: NoteSummary) {
    openPath = note.path;
  }

  async function newNote() {
    const note = await lib.createNote();
    if (note) openPath = note.path;
  }

  function closeEditor() {
    openPath = null;
    void lib.refresh();
  }

  function shortcuts(event: KeyboardEvent) {
    if (openPath || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === 'f') {
      event.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    } else if (key === 'n') {
      event.preventDefault();
      void newNote();
    }
  }

  function searchKey(event: KeyboardEvent) {
    if (event.key === 'Escape' && lib.query) {
      event.preventDefault();
      lib.query = '';
    }
  }
</script>

<svelte:window onkeydown={shortcuts} />

{#if openPath}
  <Editor
    path={openPath}
    {app}
    onclose={closeEditor}
    onsaved={(path) => {
      // A rename moves the file: keep following the note under its new name.
      openPath = path;
      void lib.refresh();
    }}
  />
{:else}
  <div class="shell">
    <header class="topbar">
      <Logo size={32} />
      <strong class="brand" title={app.library ?? ''}>{t('app.name')}</strong>
      <input
        bind:this={searchInput}
        bind:value={lib.query}
        class="search"
        type="search"
        disabled={inTrash}
        aria-label={t('library.search')}
        placeholder={t('library.searchPlaceholder')}
        onkeydown={searchKey}
      />
      <select
        class="sort"
        aria-label={t('library.sort')}
        disabled={inTrash}
        value={lib.order}
        onchange={(event) => (lib.order = event.currentTarget.value as SortOrder)}
      >
        {#each SORT_ORDERS as order (order)}
          <option value={order}>{t(`library.sortOptions.${order}`)}</option>
        {/each}
      </select>
      <button type="button" class="button primary" onclick={newNote}>{t('library.newNote')}</button>
      <button
        type="button"
        class="button ghost"
        aria-expanded={panel === 'settings'}
        onclick={() => toggle('settings')}
      >
        {t('settings.title')}
      </button>
    </header>

    <div class="library-body">
      <Sidebar {lib} testOpen={panel === 'diagnostic'} ontest={() => toggle('diagnostic')} />

      <main class="content library-main">
        <div class="library-scroll">
          {#if lib.error}
            <div class="banner error inline-banner" role="alert">
              <span>{t('errors.generic', { message: lib.error })}</span>
              <button type="button" class="button ghost" onclick={() => lib.dismissError()}>
                {t('library.dismiss')}
              </button>
            </div>
          {/if}
          {#if inTrash}
            <TrashView {lib} />
          {:else}
            <NoteGrid {lib} onopen={open} onnew={newNote} />
          {/if}
        </div>
        {#if panel === 'settings'}
          <Settings {app} {onchange} onclose={() => (panel = null)} ontest={() => (panel = 'diagnostic')} />
        {:else if panel === 'diagnostic'}
          <Diagnostic onclose={() => (panel = null)} />
        {/if}
      </main>
    </div>
  </div>
{/if}
