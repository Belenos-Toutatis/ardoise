<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { genericError, type AppState } from '../lib/api';
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
  import ActivityBanner from './library/ActivityBanner.svelte';
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
  let shell = $state<HTMLElement | null>(null);
  /** The editor or live mode shows over the library, which stays mounted (hidden) meanwhile. */
  const away = $derived(liveOpen || openPath !== null);
  /** Where the library stood when a note or live mode took the window, to give it back. */
  let returnTo: { path: string | null; element: HTMLElement | null; scroll: number } | null = null;

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
        // The panel (a test, the settings being edited) stays as it is under the editor.
        // Through the library first, so that the editor loads the new note afresh.
        const comingFrom = openPath === null && !liveOpen;
        if (comingFrom) leaveLibrary(null);
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

  /** Notes the library's place before the editor or live mode takes the window. */
  function leaveLibrary(path: string | null) {
    const active = document.activeElement;
    returnTo = {
      path,
      element: active instanceof HTMLElement && shell?.contains(active) ? active : null,
      scroll: shell?.querySelector('.library-scroll')?.scrollTop ?? 0,
    };
  }

  /** The library shows again: same scroll, and the focus on the card that was opened. */
  async function backToLibrary() {
    await tick();
    const back = returnTo;
    returnTo = null;
    if (!back) return;
    const scroller = shell?.querySelector('.library-scroll');
    if (scroller) scroller.scrollTop = back.scroll;
    const card = back.path
      ? [...(shell?.querySelectorAll<HTMLElement>('.note-card') ?? [])]
          .find((el) => el.dataset.path === back.path)
          ?.querySelector<HTMLElement>('.card-open')
      : null;
    const target = card ?? (back.element?.isConnected ? back.element : null);
    target?.focus({ preventScroll: true });
  }

  function open(note: NoteSummary) {
    leaveLibrary(note.path);
    openPath = note.path;
  }

  async function newNote() {
    const note = await lib.createNote();
    if (!note || away) return;
    leaveLibrary(note.path);
    openPath = note.path;
  }

  /** « Nouvelle note depuis un PDF ou une image » is open. */
  let fromFile = $state(false);

  /** One note made from a file is opened; several are announced. */
  function createdFromFile(notes: NoteSummary[]) {
    // A late answer (the dialog went away, another note is being edited) must not replace what is on screen.
    const current = fromFile && !away;
    fromFile = false;
    void lib.refresh();
    if (notes.length === 1 && current) {
      leaveLibrary(notes[0].path);
      openPath = notes[0].path;
    } else if (notes.length >= 1) lib.notice = t('background.library.done', { count: notes.length });
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
    leaveLibrary(null);
    liveOpen = true;
  }

  function closeLive() {
    liveOpen = false;
    void lib.refresh();
    void backToLibrary();
  }

  function closeEditor() {
    if (returnTo && openPath) returnTo.path = openPath;
    openPath = null;
    void lib.refresh();
    void backToLibrary();
  }

  function shortcuts(event: KeyboardEvent) {
    // Not under the editor, live mode or a dialog of the library (« Depuis un PDF » would be unmounted).
    if (away || lib.dialog || fromFile || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
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

<!--
  The library stays mounted, hidden, under the editor and live mode: a tablet test in progress, the
  registration wizard, the settings being edited, the scroll position and the binders opened are
  still there when the user comes back (and a library of 1,000 notes is not built again).
-->
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
{/if}

<div class="shell" class:shell-hidden={away} bind:this={shell}>
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
            <span>{genericError(lib.error)}</span>
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
        <ActivityBanner activity={lib.activity} />
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
