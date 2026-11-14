<script lang="ts">
  /**
   * « Nouvelle note depuis un PDF ou une image » (SPEC §4.4, P2): the file is chosen first (the
   * native dialog opens with this dialog), then a PDF gets one note per chosen page (all pages by
   * default) at the size of its pages, an image one note A4-sized in its proportions; each note
   * has the page or the image as its background, ready to be written on. The notes go into the
   * binder shown; one note is opened right away.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../../lib/api';
  import {
    formatPageList,
    pageMillimetres,
    parsePageList,
    type BackgroundSource,
    type NotesProgress,
  } from '../../lib/background';
  import { pickBackgroundFile } from '../../lib/dialogs';
  import { t, tn } from '../../lib/i18n.svelte';
  import type { NoteSummary } from '../../lib/note';
  import { beginWork } from '../../lib/work';
  import PdfPagePicker from '../editor/PdfPagePicker.svelte';
  import Modal from '../Modal.svelte';

  let {
    binder,
    onclose,
    oncreated,
  }: {
    /** Where the notes go (relative, `/`-separated; empty for the root). */
    binder: string;
    onclose: () => void;
    /** The notes made, in page order. */
    oncreated: (notes: NoteSummary[]) => void;
  } = $props();

  let path = $state<string | null>(null);
  let source = $state.raw<BackgroundSource | null>(null);
  let which = $state<'all' | 'some'>('all');
  let pageList = $state('');
  /** The pages chosen on the thumbnails, kept in step with `pageList` (the field is the other way to choose). */
  let pagesPicked = $state<number[]>([]);
  let running = $state(false);
  $effect(() => {
    if (running) return beginWork();
  });
  /** The notes are being made (as opposed to the file being read): this can be cancelled. */
  let creating = $state(false);
  /** « Cancel » was pressed: waiting for the page being rendered to finish. */
  let cancelling = $state(false);
  /** The last creation was cancelled: nothing was kept. */
  let cancelled = $state(false);
  let error = $state<string | null>(null);
  /** How far the creation is, while notes are made. */
  let progress = $state.raw<NotesProgress | null>(null);

  const count = $derived(source?.pages.length ?? 0);
  const chosen = $derived(which === 'all' || !source ? null : parsePageList(pageList, count));
  const firstPage = $derived(source?.pages[0] ? pageMillimetres(source.pages[0]) : null);
  /** The pages of the PDF, from 1: what « Toutes » picks. */
  const everyPage = $derived(Array.from({ length: count }, (_, i) => i + 1));
  const ready = $derived(!!source && !running && (which === 'all' || source.kind === 'image' || chosen !== null));

  async function choose() {
    error = null;
    const picked = await pickBackgroundFile(t('background.library.pickTitle'), t('background.fileFilter'));
    if (!picked) {
      // Nothing chosen the first time: there is nothing to show.
      if (!source) onclose();
      return;
    }
    running = true;
    try {
      const inspected = await api.inspectBackgroundFile(picked);
      path = picked;
      source = inspected;
      which = 'all';
      pageList = inspected.kind === 'pdf' && inspected.pages.length > 0 ? `1-${inspected.pages.length}` : '';
      pagesPicked = Array.from({ length: inspected.kind === 'pdf' ? inspected.pages.length : 0 }, (_, i) => i + 1);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      running = false;
    }
  }

  /** « Toutes » and the first choice: every page, in the field and on the thumbnails. */
  function pickEvery() {
    which = 'all';
    pagesPicked = everyPage;
    pageList = count > 0 ? `1-${count}` : '';
  }

  /** A click on the thumbnails: those pages (never « all » any more), written in the field. */
  function pickedOnThumbnails(pages: number[]) {
    which = 'some';
    pageList = formatPageList(pages);
  }

  /** Typing in the field: a valid list is shown on the thumbnails (an invalid one leaves them as they were). */
  function typedList() {
    const pages = parsePageList(pageList, count);
    if (pages) pagesPicked = [...pages].sort((a, b) => a - b);
  }

  async function create() {
    if (!ready || !path || !source) return;
    running = true;
    creating = true;
    cancelling = false;
    cancelled = false;
    error = null;
    progress = null;
    try {
      const pages = source.kind === 'pdf' && which === 'some' && chosen ? chosen : undefined;
      const made = await api.createNotesFromFile(binder, path, pages, (next) => (progress = next));
      oncreated(made);
    } catch (e) {
      if ((e as { code?: unknown } | null)?.code === 'cancelled') cancelled = true;
      else error = errorMessage(e);
      running = false;
      progress = null;
    } finally {
      creating = false;
      cancelling = false;
    }
  }

  /** Stops after the page being rendered; the notes already made are removed again. */
  async function cancel() {
    if (!creating || cancelling) return;
    cancelling = true;
    await api.cancelCreateNotes().catch(() => {});
  }

  onMount(() => {
    void choose();
  });
</script>

<Modal title={t('background.library.title')} onclose={() => (creating ? void cancel() : onclose())} dismissible={!running || creating}>
  <form
    aria-busy={running}
    onsubmit={(event) => {
      event.preventDefault();
      void create();
    }}
  >
    <p class="muted">{t('background.library.intro')}</p>

    {#if source}
      <p data-slot="new-from-file-name"><strong>{source.name}</strong></p>
      {#if source.kind === 'pdf'}
        <p class="muted small">
          {tn('background.library.pdfInfo', count, { width: firstPage?.width ?? 0, height: firstPage?.height ?? 0 })}
        </p>
        {#if count > 1}
          <fieldset class="export-fieldset" disabled={running}>
            <legend>{t('background.library.pages')}</legend>
            <label class="choice">
              <input type="radio" name="new-from-file-pages" value="all" bind:group={which} onchange={pickEvery} />
              {t('background.library.allPages', { count })}
            </label>
            <label class="choice">
              <input type="radio" name="new-from-file-pages" value="some" bind:group={which} />
              {t('background.library.somePages')}
            </label>
            {#if which === 'some'}
              <input
                class="text-input"
                type="text"
                aria-label={t('background.library.pageList')}
                placeholder="1-3, 5"
                bind:value={pageList}
                oninput={typedList}
              />
              <p class="muted small export-hint" class:error={chosen === null}>
                {chosen === null
                  ? pageList.trim() === ''
                    ? t('background.library.noPageChosen')
                    : t('background.library.pageListInvalid', { count })
                  : t('background.library.pageListCount', { count: chosen.length })}
              </p>
            {/if}
            <!-- The thumbnails of the pages: click to choose (Maj + clic for a run), as many as the PDF has, drawn as they
                 come into view. They are shown with « all » too: a click on one makes it « some pages ». -->
            {#if path}
              <p class="muted small" id="new-from-file-thumbs-hint">{t('background.library.thumbnailsHint')}</p>
              <PdfPagePicker
                {path}
                name={source.name}
                pages={source.pages}
                multiple
                bind:picked={pagesPicked}
                onpick={pickedOnThumbnails}
                disabled={running}
              />
              <div class="actions thumbs-actions">
                <button type="button" class="button small-button" data-action="pages-all" disabled={running} onclick={pickEvery}>
                  {t('background.library.pagesAll')}
                </button>
                <button
                  type="button"
                  class="button small-button"
                  data-action="pages-none"
                  disabled={running}
                  onclick={() => {
                    pagesPicked = [];
                    pickedOnThumbnails([]);
                  }}
                >
                  {t('background.library.pagesNone')}
                </button>
              </div>
            {/if}
          </fieldset>
        {/if}
      {:else}
        <p class="muted small">{t('background.library.imageInfo')}</p>
      {/if}
    {/if}

    {#if running}
      <p class="muted small" role="status">
        {!source
          ? t('background.library.reading')
          : progress && progress.total > 1
            ? t('background.library.progress', { done: progress.done, total: progress.total })
            : t('background.library.running')}
      </p>
      {#if progress && progress.total > 1}
        <progress max={progress.total} value={progress.done} aria-label={t('background.library.running')}></progress>
      {/if}
    {/if}
    {#if cancelled}
      <p class="muted small" role="status" data-slot="new-from-file-cancelled">{t('background.library.cancelled')}</p>
    {/if}
    {#if error}
      <p class="banner error inline" role="alert">{t('background.library.error', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="submit" class="button primary" data-autofocus disabled={!ready}>
        {source?.kind === 'pdf' && (which === 'all' ? count : (chosen?.length ?? 0)) > 1
          ? t('background.library.createMany', { count: which === 'all' ? count : (chosen?.length ?? 0) })
          : t('background.library.createOne')}
      </button>
      <button type="button" class="button" disabled={running} onclick={() => void choose()}>
        {t('background.library.otherFile')}
      </button>
      <button
        type="button"
        class="button"
        data-action="cancel-create"
        disabled={cancelling || (running && !creating)}
        onclick={() => (creating ? void cancel() : onclose())}
      >
        {cancelling ? t('background.library.cancelling') : t('library.cancel')}
      </button>
    </div>
  </form>
</Modal>
