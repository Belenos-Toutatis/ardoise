<script lang="ts">
  /**
   * Prints notes: the editor's note (as it is in memory) or the library's selection. Windows
   * opens its own print dialog (printer, copies, pages); each note is printed on a page of its
   * own, in its orientation. Here the user only chooses whether the paper is printed and how
   * the note sits on the sheet.
   */
  import { untrack } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import { binderContent } from '../lib/binder-options';
  import { t, tn } from '../lib/i18n.svelte';
  import type { ExportSource, PrintOptions } from '../lib/note';
  import { PRINT_PER_SHEET, loadPrintChoice, savePrintChoice } from '../lib/print-options';
  import { beginWork } from '../lib/work';
  import Modal from './Modal.svelte';

  let {
    sources: given,
    binder,
    subject,
    onclose,
  }: {
    sources: ExportSource[];
    /** « Imprimer le classeur »: the notes are a binder's, in its order; `sourcesFor` gives them with or without its sub-binders. */
    binder?: {
      name: string;
      hasSub: boolean;
      sourcesFor: (includeSub: boolean, withDates: boolean) => ExportSource[];
    };
    /** What is printed, as a sentence part: the note's title, or "3 notes". */
    subject: string;
    onclose: () => void;
  } = $props();

  /** The sub-binders are printed too only when the user asks (SPEC: an option), but a binder with no note of its own starts with them. */
  const content = untrack(() => (binder ? binderContent(binder) : null));
  let includeSub = $state(content?.includeSubAtStart ?? false);
  /** The page prints no date or bookmark: whether the dates name the notes does not matter here. */
  const sources = $derived(binder ? binder.sourcesFor(includeSub, false) : given);
  /** A binder's dialog with no note to print: the button waits, and a sentence says why. */
  const noNote = $derived(sources.length === 0);

  let options = $state<PrintOptions>({ ...loadPrintChoice() });
  let phase = $state<'idle' | 'running' | 'done' | 'cancelled'>('idle');
  let pages = $state(0);
  let error = $state<string | null>(null);

  const running = $derived(phase === 'running');
  $effect(() => {
    if (running) return beginWork();
  });

  async function run() {
    error = null;
    phase = 'running';
    try {
      const request = { ...options };
      const result = await api.printNotes(
        sources.map((source) => ({ ...source })),
        request,
      );
      if (result.printed) {
        savePrintChoice(request);
        pages = result.pages;
        phase = 'done';
      } else {
        phase = 'cancelled';
      }
    } catch (e) {
      error = errorMessage(e);
      phase = 'idle';
    }
  }
</script>

<Modal title={t('print.title')} onclose={() => !running && onclose()} dismissible={!running}>
  <div aria-busy={running} data-dialog="print">
    <p class="muted">{subject}</p>
    <p class="muted small">{t('print.intro')}</p>

    {#if binder?.hasSub}
      <label class="check" data-slot="include-sub-binders">
        <input type="checkbox" data-option="include-sub-binders" bind:checked={includeSub} disabled={running} />
        {t('export.includeSubBinders')}
      </label>
      <p class="muted small">{t('export.includeSubBindersHint')}</p>
    {/if}
    {#if binder}
      {#if content && content.total === 0}
        <p class="muted" role="status" data-slot="binder-empty">{t('export.binderEmpty')}</p>
      {:else if content && content.own === 0}
        <p class="muted small" role="status" data-slot="binder-only-sub">
          {includeSub ? t('print.binderOnlySubIncluded') : t('print.binderOnlySub')}
        </p>
      {/if}
      {#if !noNote}
        <p class="muted small" data-slot="binder-print-hint">{t('print.binderHint', { count: sources.length })}</p>
      {/if}
    {/if}

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('print.options')}</legend>
      <label class="check">
        <input type="checkbox" data-option="background" bind:checked={options.background} />
        {t('print.background')}
      </label>
      <p class="muted small">{t('print.backgroundHint')}</p>
      <div class="export-row" role="radiogroup" aria-label={t('print.fit')}>
        <span>{t('print.fit')}</span>
        <label class="choice">
          <input type="radio" name="print-fit" value="actual" bind:group={options.fit} />
          {t('print.fitActual')}
        </label>
        <label class="choice">
          <input type="radio" name="print-fit" value="fit" bind:group={options.fit} />
          {t('print.fitFit')}
        </label>
      </div>
      <div class="export-row" role="radiogroup" aria-label={t('print.perSheet')} data-slot="print-per-sheet">
        <span>{t('print.perSheet')}</span>
        {#each PRINT_PER_SHEET as count (count)}
          <label class="choice">
            <input type="radio" name="print-per-sheet" value={count} bind:group={options.perSheet} />
            {t(`print.perSheetChoices.${count}`)}
          </label>
        {/each}
      </div>
      {#if options.perSheet > 1}
        <p class="muted small">{t('print.perSheetHint')}</p>
      {/if}
    </fieldset>

    {#if running}
      <p class="export-status" role="status">{t('print.running')}</p>
    {:else if phase === 'done'}
      <p class="export-status" role="status">{tn('print.done', pages)}</p>
    {:else if phase === 'cancelled'}
      <p class="export-notice muted" role="status">{t('print.cancelled')}</p>
    {/if}
    {#if error}
      <p class="banner error inline" role="alert">{t('print.failed', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" data-autofocus data-action="print-run" disabled={running || noNote} onclick={run}>
        {phase === 'done' ? t('print.again') : running ? t('print.running') : t('print.run')}
      </button>
      <button type="button" class="button" disabled={running} onclick={onclose}>
        {phase === 'done' ? t('common.close') : t('library.cancel')}
      </button>
    </div>
  </div>
</Modal>
