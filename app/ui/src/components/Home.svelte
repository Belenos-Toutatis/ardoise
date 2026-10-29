<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { AppState } from '../lib/api';
  import { bridgeScreens } from '../lib/app-bridge.svelte';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import { inkspaceOffer } from '../lib/inkspace-offer.svelte';
  import { SORT_ORDERS, displayTitle, type SortOrder } from '../lib/library-logic';
  import { LibraryState } from '../lib/library-state.svelte';
  import type { NoteSummary } from '../lib/note';
  import Diagnostic from './Diagnostic.svelte';
  import LiveView from './live/LiveView.svelte';
  import TabletPanel from './tablet/TabletPanel.svelte';
  import ExportDialog from './ExportDialog.svelte';
  import PrintDialog from './PrintDialog.svelte';
  import Editor from './editor/Editor.svelte';
  import AssembleDialog from './library/AssembleDialog.svelte';
  import InkspaceImportDialog from './library/InkspaceImportDialog.svelte';
  import NewFromFileDialog from './library/NewFromFileDialog.svelte';
  import NoteGrid from './library/NoteGrid.svelte';
  import Sidebar from './library/Sidebar.svelte';
  import TrashView from './library/TrashView.svelte';
  import Logo from './Logo.svelte';
  import Settings from './Settings.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  const lib = new LibraryState();

  // One side panel at a time.
  let panel = $state<'settings' | 'diagnostic' | 'tablet' | null>(null);
  /** Live mode (and the classroom's control window) takes the whole window, like the editor. */
  let liveOpen = $state(false);
  /** Path of the note being edited; the editor then takes the whole window. */
  let openPath = $state<string | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);

  const inTrash = $derived(lib.view.kind === 'trash');

  onMount(() => {
    // Started with `--diagnostic`: the tablet test is shown right away.
    if (app.openDiagnostic) panel = 'diagnostic';
    // The wizard offered to import Inkspace's notes and the user accepted: the dialog opens once.
    if (inkspaceOffer.pending) {
      inkspaceOffer.pending = false;
      inkspaceOpen = true;
    }
    const stopLibrary = lib.start();
    // The assistant's bridge opens notes as a click on a card does (`lib/app-bridge.svelte.ts`).
    const unregister = bridgeScreens.register('home', {
      busy: () =>
        lib.dialog || fromFile || inkspaceOpen
          ? 'a dialog is open in Ardoise'
          : panel === 'diagnostic'
            ? 'the tablet test is open in Ardoise'
            : null,
      openNote: async (path) => {
        panel = null;
        // Through the library first, so that the editor loads the new note afresh.
        openPath = null;
        await tick();
        openPath = path;
        await tick();
      },
    });
    return () => {
      unregister();
      stopLibrary();
    };
  });

  function toggle(name: 'settings' | 'diagnostic' | 'tablet') {
    panel = panel === name ? null : name;
  }

  function open(note: NoteSummary) {
    openPath = note.path;
  }

  async function newNote() {
    const note = await lib.createNote();
    if (note) openPath = note.path;
  }

  /** « Nouvelle note depuis un PDF ou une image » is open. */
  let fromFile = $state(false);

  /** One note made from a file is opened; several are announced. */
  function createdFromFile(notes: NoteSummary[]) {
    fromFile = false;
    void lib.refresh();
    if (notes.length === 1) openPath = notes[0].path;
    else if (notes.length > 1) lib.notice = t('background.library.done', { count: notes.length });
  }

  /** « Importer depuis Inkspace » is open. */
  let inkspaceOpen = $state(false);

  function closeInkspace() {
    inkspaceOpen = false;
    void lib.refresh();
  }

  /** Shows the notes an import has just made. */
  async function showImported(binder: string) {
    inkspaceOpen = false;
    // The binder is new to the listing until it is read again (a refresh drops unknown binders).
    await lib.refresh();
    lib.setView(binder ? { kind: 'binder', binder } : { kind: 'all' });
  }

  function openLive() {
    panel = null;
    liveOpen = true;
  }

  function closeLive() {
    liveOpen = false;
    void lib.refresh();
  }

  function closeEditor() {
    openPath = null;
    void lib.refresh();
  }

  function shortcuts(event: KeyboardEvent) {
    if (openPath || liveOpen || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
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

{#if liveOpen}
  <LiveView {app} {onchange} onclose={closeLive} />
{:else if openPath}
  <Editor
    path={openPath}
    {app}
    {onchange}
    onclose={closeEditor}
    onsaved={(path) => {
      // A rename moves the file: keep following the note under its new name, unless the
      // editor was closed meanwhile (a late result must not reopen it).
      if (openPath !== null) openPath = path;
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
      <button type="button" class="button" data-action="new-from-file" title={t('background.library.openHint')} onclick={() => (fromFile = true)}>
        {t('background.library.open')}
      </button>
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
      <Sidebar
        {lib}
        testOpen={panel === 'diagnostic'}
        ontest={() => toggle('diagnostic')}
        tabletOpen={panel === 'tablet'}
        ontablet={() => toggle('tablet')}
        oninkspace={() => (inkspaceOpen = true)}
      />

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
          {#if lib.notice}
            <div class="banner inline-banner notice" role="status">
              <span>{lib.notice}</span>
              <button type="button" class="button ghost" onclick={() => lib.dismissNotice()}>
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
        {:else if panel === 'tablet'}
          <TabletPanel onclose={() => (panel = null)} onlive={openLive} ontest={() => (panel = 'diagnostic')} />
        {/if}
      </main>
    </div>
  </div>

  {#if lib.dialog?.kind === 'export'}
    <ExportDialog
      sources={lib.dialog.notes.map((n) => ({ path: n.path }))}
      subject={lib.dialog.notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(lib.dialog.notes[0], currentLocale()) })
        : t('export.subjectMany', { count: lib.dialog.notes.length })}
      onclose={() => lib.closeDialog()}
    />
  {:else if lib.dialog?.kind === 'print'}
    <PrintDialog
      sources={lib.dialog.notes.map((n) => ({ path: n.path }))}
      subject={lib.dialog.notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(lib.dialog.notes[0], currentLocale()) })
        : t('export.subjectMany', { count: lib.dialog.notes.length })}
      onclose={() => lib.closeDialog()}
    />
  {:else if lib.dialog?.kind === 'assemble'}
    <AssembleDialog {lib} notes={lib.dialog.notes} onclose={() => lib.closeDialog()} />
  {/if}
  {#if fromFile}
    <NewFromFileDialog binder={lib.currentBinder} onclose={() => (fromFile = false)} oncreated={createdFromFile} />
  {/if}
  {#if inkspaceOpen}
    <InkspaceImportDialog
      binders={lib.listing?.binders ?? []}
      onclose={closeInkspace}
      onshow={(binder) => void showImported(binder)}
      ondone={() => void lib.refresh()}
    />
  {/if}
{/if}
