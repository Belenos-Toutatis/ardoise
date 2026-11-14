<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { genericError, type AppState } from '../lib/api';
  import { bridgeScreens } from '../lib/app-bridge.svelte';
  import { closeKey } from '../lib/close-keys';
  import { defaultWindowClose, type WindowClose } from '../lib/editor/window-close';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import { inkspaceOffer } from '../lib/inkspace-offer.svelte';
  import { displayTitle, type SortOrder, type SummaryChoices } from '../lib/library-logic';
  import { LibraryState } from '../lib/library-state.svelte';
  import { isPrintKey } from '../lib/print-key';
  import { share } from '../lib/share-state.svelte';
  import { isBusy, tablet } from '../lib/tablet-state.svelte';
  import { workInProgress } from '../lib/work';
  import type { BookmarkRef, NoteSummary } from '../lib/note';
  import Diagnostic from './Diagnostic.svelte';
  import LiveView from './live/LiveView.svelte';
  import TabletPanel from './tablet/TabletPanel.svelte';
  import ExportDialog from './ExportDialog.svelte';
  import PrintDialog from './PrintDialog.svelte';
  import Editor from './editor/Editor.svelte';
  import ActivityBanner from './library/ActivityBanner.svelte';
  import AssembleDialog from './library/AssembleDialog.svelte';
  import DateDialog from './library/DateDialog.svelte';
  import DraftsBanner from './library/DraftsBanner.svelte';
  import InkspaceImportDialog from './library/InkspaceImportDialog.svelte';
  import NewFromFileDialog from './library/NewFromFileDialog.svelte';
  import NoteGrid from './library/NoteGrid.svelte';
  import Sidebar from './library/Sidebar.svelte';
  import SummaryDialog from './library/SummaryDialog.svelte';
  import TrashView from './library/TrashView.svelte';
  import Logo from './Logo.svelte';
  import Modal, { modalOpen } from './Modal.svelte';
  import ShareIndicator from './share/ShareIndicator.svelte';
  import SharePanel from './share/SharePanel.svelte';
  import Settings from './Settings.svelte';

  let {
    app,
    onchange,
    windowClose = defaultWindowClose(),
  }: { app: AppState; onchange: (next: AppState) => void; windowClose?: WindowClose } = $props();

  const lib = new LibraryState();
  /** The window was asked to close while work is under way in the library. */
  let quitAsked = $state(false);

  // One side panel at a time.
  let panel = $state<'settings' | 'diagnostic' | 'tablet' | null>(null);
  /** Live mode (and the classroom's control window) takes the whole window, like the editor. */
  let liveOpen = $state(false);
  /** Path of the note being edited; the editor then takes the whole window. */
  let openPath = $state<string | null>(null);
  /** The bookmark the editor shows when it opens the note (the binder's summary): its id. */
  let openFocus = $state<{ id: string } | null>(null);
  /** Live mode resumes this note (M12) instead of starting on a blank page; null for a new session. */
  let resumePath = $state<string | null>(null);
  /** Live mode was asked from the tablet panel's « Live mode » button: where the focus goes back if the user gives up. */
  let liveFromPanel = false;
  let searchInput = $state<HTMLInputElement | null>(null);
  let shell = $state<HTMLElement | null>(null);
  /** The editor or live mode shows over the library, which stays mounted (hidden) meanwhile. */
  const away = $derived(liveOpen || openPath !== null);
  /** Where the library stood when a note or live mode took the window, to give it back. */
  let returnTo: { path: string | null; element: HTMLElement | null; scroll: number; moved?: boolean } | null = null;

  const inTrash = $derived(lib.view.kind === 'trash');

  /** What the export and print dialogs of a binder need: its name, and its notes with or without the sub-binders. */
  function binderMode(binder: string | undefined) {
    if (binder === undefined) return undefined;
    return {
      name: binder.split('/').pop() || binder,
      hasSub: lib.hasSubBinders(binder),
      sourcesFor: (includeSub: boolean, withDates: boolean) => lib.binderSources(binder, includeSub, withDates),
      summaryRows: (includeSub: boolean, choices: SummaryChoices) => lib.binderSummaryRows(binder, includeSub, choices),
      hasBookmarks: (includeSub: boolean) => lib.binderHasBookmarks(binder, includeSub),
    };
  }
  /** The notes as the library shows them now (binder, tag, search, sort): the editor's page buttons walk through them. */
  const pagePaths = $derived(lib.visible.map((n) => n.path));

  // What the share on the network shows is what the main window shows: the library, the settings
  // and the panels are the waiting screen; the editor and live mode say what they show themselves.
  $effect(() => {
    if (!away) share.showWaiting();
  });

  onMount(() => {
    share.follow();
    // Started with `--diagnostic`: the tablet test is shown right away.
    if (app.openDiagnostic) panel = 'diagnostic';
    // The wizard offered to import Inkspace's notes and the user accepted: the dialog opens once.
    if (inkspaceOffer.pending) {
      inkspaceOffer.pending = false;
      inkspaceOpen = true;
    }
    const stopLibrary = lib.start();
    let stopGuard: (() => void) | null = null;
    let gone = false;
    windowClose.subscribe(holdClose).then(
      (stop) => {
        if (gone) stop();
        else stopGuard = stop;
      },
      () => {
        // No close guard (e.g. not allowed): closing then works as before.
      },
    );
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
      gone = true;
      stopGuard?.();
      unregister();
      stopLibrary();
    };
  });

  /**
   * The window is asked to close with the library on display and work under way: true holds it back,
   * and the user is asked. The editor and live mode have their own rules.
   */
  function holdClose(): boolean {
    if (away) return false;
    const connection = tablet.status?.connection;
    if (!workInProgress() && !lib.activity.busy && !(connection && isBusy(connection))) return false;
    if (windowClose.destroy) quitAsked = true;
    return true;
  }

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
    // The editor went on to other notes by its page buttons: the one it shows may be far from where the library was.
    if (back.moved) card?.scrollIntoView?.({ block: 'nearest' });
  }

  function open(note: NoteSummary) {
    leaveLibrary(note.path);
    openFocus = null;
    openPath = note.path;
  }

  /** A bookmark of the binder's summary is chosen: the note opens at it. */
  function openAtBookmark(note: NoteSummary, bookmark?: BookmarkRef) {
    lib.closeSummary();
    leaveLibrary(note.path);
    openFocus = bookmark ? { id: bookmark.id } : null;
    openPath = note.path;
  }

  /** A draft's note is opened from the banner. */
  function openDraft(path: string) {
    leaveLibrary(path);
    openPath = path;
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
    liveFromPanel = panel === 'tablet';
    panel = null;
    leaveLibrary(null);
    resumePath = null;
    liveOpen = true;
  }

  /**
   * « Reprendre en mode direct » (M12), from a card's menu or from the editor (which has saved or
   * let go of its changes first): live mode takes the window on that note.
   */
  function resumeLive(path: string) {
    liveFromPanel = false;
    panel = null;
    if (openPath !== null && returnTo) returnTo.path = path;
    else leaveLibrary(path);
    openPath = null;
    resumePath = path;
    liveOpen = true;
  }

  /**
   * `notice`: what live mode has to say on the way out (a recording that could not be kept). `declined`: the user gave up
   * the question of the pages the Xiaomi tablet still holds; from the tablet panel's button, the panel and that button
   * are given back (a card or the editor gets its card's focus, as always).
   */
  function closeLive(notice?: unknown, declined?: boolean) {
    const toPanel = declined === true && liveFromPanel;
    liveOpen = false;
    resumePath = null;
    void lib.refresh();
    if (typeof notice === 'string') lib.notice = notice;
    if (toPanel) panel = 'tablet';
    void backToLibrary().then(async () => {
      if (!toPanel) return;
      await tick();
      shell?.querySelector<HTMLElement>('[data-action="live"]')?.focus({ preventScroll: true });
    });
  }

  function closeEditor() {
    if (returnTo && openPath) returnTo.path = openPath;
    openPath = null;
    openFocus = null;
    void lib.refresh();
    void backToLibrary();
  }

  /** Escape, Ctrl + W and Ctrl + F4 close the side panel; a tablet test under way only stops by its button. */
  function closesPanel(event: KeyboardEvent): boolean {
    if (!panel || away || event.defaultPrevented || modalOpen()) return false;
    if (event.key !== 'Escape' && closeKey(event) !== 'close') return false;
    return !(panel === 'diagnostic' && workInProgress());
  }

  /** Whether the key is typed in a field: Ctrl+A and Escape are the field's own there. */
  function typing(event: KeyboardEvent): boolean {
    const target = event.target;
    return (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement ||
      (target instanceof HTMLElement && target.isContentEditable)
    );
  }

  /**
   * The selection's keys wherever the focus is in the library (a card handles them itself when it has the focus):
   * Ctrl+A selects every note shown instead of the window's text, Escape drops the selection.
   */
  function selectionKey(event: KeyboardEvent): boolean {
    if (away || lib.dialog || fromFile || share.panelOpen || modalOpen() || event.defaultPrevented || typing(event)) return false;
    if (event.key === 'Escape' && !event.ctrlKey && !event.metaKey && !event.altKey && lib.selected.length > 0) {
      lib.clearSelection();
      return true;
    }
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'a') {
      lib.selectAll();
      return true;
    }
    return false;
  }

  /**
   * Ctrl + P in the library: « Imprimer… » of what is in front: the selected notes (as the selection bar's button), else the
   * binder shown (as its menu's « Imprimer le classeur… »). Notes shown without a selection are not printed on a guess (a
   * thousand notes by an inadvertent key): the notice says what to do. Nothing under a dialog, the editor or live mode.
   */
  function printKey() {
    const busy =
      away || lib.dialog || lib.summaryBinder !== null || lib.dateNote || fromFile || inkspaceOpen || quitAsked || share.panelOpen || modalOpen();
    if (busy) return;
    if (lib.selectedDisplayed.length > 0) return lib.openDialog('print');
    const view = lib.view;
    if (view.kind === 'binder') {
      if (lib.binderNotes(view.binder, true).length > 0) return lib.openBinderDialog('print', view.binder);
      lib.notice = t('library.binder.emptyHint');
      return;
    }
    lib.notice = t('library.printNone');
  }

  function shortcuts(event: KeyboardEvent) {
    // Ardoise's printing, never the web view's (the guard of `lib/browser-guard.ts` keeps that off in every window).
    if (isPrintKey(event)) {
      event.preventDefault();
      if (!event.repeat && !away) printKey();
      return;
    }
    if (selectionKey(event)) {
      event.preventDefault();
      return;
    }
    if (closesPanel(event)) {
      event.preventDefault();
      panel = null;
      return;
    }
    // Not under the editor, live mode or a dialog of the library (« Depuis un PDF » would be unmounted).
    if (away || lib.dialog || fromFile || share.panelOpen || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
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
  <LiveView {app} {onchange} resume={resumePath} onclose={closeLive} />
{:else if openPath}
  <Editor
    path={openPath}
    {app}
    {onchange}
    onclose={closeEditor}
    focus={openFocus}
    onsaved={(path) => {
      // A rename moves the file: keep following the note under its new name, unless the
      // editor was closed meanwhile (a late result must not reopen it).
      if (openPath !== null) openPath = path;
      void lib.refresh();
    }}
    onresume={resumeLive}
    pages={pagePaths}
    onpage={(path) => {
      // The editor went on to the next note; `closeEditor` gives the library's focus to this one.
      if (openPath === null) return;
      openPath = path;
      if (returnTo) returnTo.moved = true;
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
      value={lib.effectiveOrder}
      onchange={(event) => (lib.order = event.currentTarget.value as SortOrder)}
    >
      {#each lib.sortChoices as order (order)}
        <option value={order}>{t(`library.sortOptions.${order}`)}</option>
      {/each}
    </select>
    {#if lib.view.kind === 'binder'}
      <button
        type="button"
        class="button"
        data-action="binder-summary"
        title={t('library.summary.hint')}
        onclick={() => lib.view.kind === 'binder' && lib.openSummary(lib.view.binder)}
      >
        {t('library.summary.button')}
      </button>
    {/if}
    <button type="button" class="button primary" onclick={newNote}>{t('library.newNote')}</button>
    <button type="button" class="button" data-action="new-from-file" title={t('background.library.openHint')} onclick={() => (fromFile = true)}>
      {t('background.library.open')}
    </button>
    <ShareIndicator />
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
        <DraftsBanner {lib} onopen={openDraft} />
        {#if inTrash}
          <TrashView {lib} />
        {:else}
          <NoteGrid {lib} onopen={open} onnew={newNote} onresume={(note) => resumeLive(note.path)} />
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
  {@const binder = binderMode(lib.dialog.binder)}
  <ExportDialog
    sources={lib.dialog.notes.map((n) => ({ path: n.path }))}
    {binder}
    subject={binder
      ? t('export.subjectBinder', { name: binder.name })
      : lib.dialog.notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(lib.dialog.notes[0], currentLocale()) })
        : t('export.subjectMany', { count: lib.dialog.notes.length })}
    onclose={() => lib.closeDialog()}
  />
{:else if lib.dialog?.kind === 'print'}
  {@const binder = binderMode(lib.dialog.binder)}
  <PrintDialog
    sources={lib.dialog.notes.map((n) => ({ path: n.path }))}
    {binder}
    subject={binder
      ? t('export.subjectBinder', { name: binder.name })
      : lib.dialog.notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(lib.dialog.notes[0], currentLocale()) })
        : t('export.subjectMany', { count: lib.dialog.notes.length })}
    onclose={() => lib.closeDialog()}
  />
{:else if lib.dialog?.kind === 'assemble'}
  <AssembleDialog {lib} notes={lib.dialog.notes} onclose={() => lib.closeDialog()} />
{/if}
{#if lib.summaryBinder !== null}
  <SummaryDialog {lib} binder={lib.summaryBinder} onopen={openAtBookmark} onclose={() => lib.closeSummary()} />
{/if}
{#if lib.dateNote}
  <DateDialog {lib} note={lib.dateNote} onclose={() => lib.closeDate()} />
{/if}
{#if quitAsked}
  <Modal title={t('library.quitBusy.title')} role="alertdialog" onclose={() => (quitAsked = false)}>
    <p>{t('library.quitBusy.message')}</p>
    <div class="actions">
      <button type="button" class="button primary" data-autofocus data-action="quit-wait" onclick={() => (quitAsked = false)}>
        {t('library.quitBusy.wait')}
      </button>
      <button
        type="button"
        class="button danger"
        data-action="quit-anyway"
        onclick={() => {
          quitAsked = false;
          void windowClose.destroy?.();
        }}
      >
        {t('library.quitBusy.quit')}
      </button>
    </div>
  </Modal>
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
{#if share.panelOpen}
  <SharePanel />
{/if}
