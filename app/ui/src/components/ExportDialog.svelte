<script lang="ts">
  /**
   * Exports notes to PNG, JPG, WebP, SVG or PDF. Used by the editor (the note as it is in memory) and by
   * the library (the selected notes). The destination is asked last, with the native dialog: a
   * file for one note or for a PDF (one page per note), a folder for several images.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import { pickExportFile, pickExportFolder } from '../lib/dialogs';
  import {
    DPI_CHOICES,
    FORMATS,
    dpiFits,
    exportStem,
    hasLayers,
    isRaster,
    layerInk,
    loadChoice,
    saveChoice,
  } from '../lib/export-options';
  import { t } from '../lib/i18n.svelte';
  import type { ExportFormat, ExportOptions, ExportSource, Note } from '../lib/note';
  import { beginWork } from '../lib/work';
  import Modal from './Modal.svelte';
  import TextExportSection from './TextExportSection.svelte';

  let {
    sources,
    subject,
    onclose,
  }: {
    sources: ExportSource[];
    /** What is exported, as a sentence part: the note's title, or "3 notes". */
    subject: string;
    onclose: () => void;
  } = $props();

  /** Prefix of the element ids that tie each format to its description. */
  const uid = $props.id();
  /** Rows of the "Which format?" comparison, in `export.compare.rows`. */
  const COMPARE_ROWS = ['zoom', 'transparent', 'size', 'several', 'opens', 'bestFor'] as const;
  /** The two choices of the layers block, in `export.layers`. */
  const LAYER_CHOICES = ['separate', 'merged'] as const;

  const initial = loadChoice();
  let format = $state<ExportFormat>(initial.format);
  let options = $state<ExportOptions>({ ...initial.options });
  /** SVG and PDF: separate or merged layers, remembered per format. */
  let layers = $state({ ...initial.layers });
  let phase = $state<'idle' | 'running' | 'done'>('idle');
  let error = $state<string | null>(null);
  /** The last export was cancelled by the user: a neutral notice, not an error. */
  let cancelled = $state(false);
  /** Cancellation was asked and the export has not stopped yet. */
  let cancelling = $state(false);
  /** The exported notes, once read: their page sizes (an image over 16 384 px is refused) and their layers. */
  let notes = $state.raw<Note[] | null>(null);
  const pages = $derived(notes ? notes.map((note) => note.page) : null);
  let written = $state<string[]>([]);
  let destination = $state('');

  const many = $derived(sources.length > 1);
  const intoFolder = $derived(many && format !== 'pdf');
  const running = $derived(phase === 'running');
  $effect(() => {
    if (running) return beginWork();
  });
  const raster = $derived(isRaster(format));
  const fits = $derived(DPI_CHOICES.map((dpi) => !pages || dpiFits(pages, dpi, format)));
  const someTooLarge = $derived(fits.includes(false));
  /** Even the lowest resolution gives a too large image: only PDF and SVG are possible. */
  const nothingFits = $derived(!fits.includes(true));
  /** The format whose layers the dialog offers to keep apart; null for an image. */
  const layerFormat = $derived(hasLayers(format) ? format : null);
  // The layers block matters only for a note with two or more visible layers holding ink; the sentence about hidden
  // layers only for a note whose hidden layer holds ink. Until the notes are read, neither is shown.
  const several = $derived(notes?.some((note) => layerInk(note).visible >= 2) ?? false);
  const someHidden = $derived(notes?.some((note) => layerInk(note).hidden) ?? false);

  // The notes held by the library are read once; until then (or if it fails) every resolution is offered and the
  // desktop app refuses what is too large.
  onMount(() => {
    let alive = true;
    const read = sources.map((source) =>
      source.note ? Promise.resolve(source.note) : api.openNote(source.path).then((opened) => opened.note),
    );
    Promise.all(read)
      .then((all) => {
        if (alive) notes = all;
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  });

  // A remembered resolution that no longer fits gives way to the largest one that does.
  $effect(() => {
    if (!pages) return;
    const index = (DPI_CHOICES as readonly number[]).indexOf(options.dpi);
    if (index >= 0 && fits[index]) return;
    for (let i = DPI_CHOICES.length - 1; i >= 0; i--) {
      if (fits[i] && DPI_CHOICES[i] <= options.dpi) {
        options.dpi = DPI_CHOICES[i];
        return;
      }
    }
  });

  /** The destination dialog's proposed file name. */
  function defaultName(): string {
    const stem = many ? t('export.defaultPdfName') : exportStem(sources[0].path);
    return `${stem}.${format}`;
  }

  async function run() {
    error = null;
    cancelled = false;
    let chosen: string | null;
    try {
      chosen = intoFolder
        ? await pickExportFolder(t('export.pickFolder'))
        : await pickExportFile(t('export.pickFile'), defaultName(), format);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    if (!chosen) return;
    phase = 'running';
    cancelling = false;
    destination = chosen;
    try {
      // The request names the layers only when the dialog offered the choice (an SVG or a PDF of notes with two
      // shown layers holding ink): an image has none, and a one-layer note must not get a layer of its own in the
      // PDF (an optional content group, the reader's layers panel opened) because « Separate » was remembered.
      const request: ExportOptions = layerFormat && several ? { ...options, layers: layers[layerFormat] } : { ...options };
      written = await api.exportNotes(
        sources.map((source) => ({ ...source })),
        format,
        request,
        chosen,
      );
      saveChoice({ format, options: { ...options }, layers: { ...layers } });
      phase = 'done';
    } catch (e) {
      if (typeof e === 'object' && e !== null && 'code' in e && e.code === 'cancelled') cancelled = true;
      else error = errorMessage(e);
      phase = 'idle';
    } finally {
      cancelling = false;
    }
  }

  /** Asks the desktop app to stop the export; `run` then ends with the `cancelled` rejection. */
  async function cancel() {
    if (!running || cancelling) return;
    cancelling = true;
    try {
      await api.cancelExport();
    } catch (e) {
      cancelling = false;
      error = errorMessage(e);
    }
  }

  /** Escape and the Cancel button: stop the export under way, else close the dialog. */
  function close() {
    if (running) void cancel();
    else onclose();
  }
</script>

<Modal title={t('export.title')} onclose={close}>
  <div aria-busy={running}>
    <p class="muted">{subject}</p>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('export.format')}</legend>
      <div class="export-choices" role="radiogroup" aria-label={t('export.format')}>
        {#each FORMATS as value (value)}
          <!-- The whole card selects the format; the radio is named by the format alone and
               described by its strengths and limits. -->
          <label class="choice format-card" class:selected={format === value}>
            <span class="format-name">
              <input
                type="radio"
                name="export-format"
                {value}
                bind:group={format}
                aria-labelledby="{uid}-{value}-name"
                aria-describedby="{uid}-{value}-pros {uid}-{value}-cons"
              />
              <span class="choice-name" id="{uid}-{value}-name">{t(`export.formats.${value}`)}</span>
            </span>
            <span class="format-pros" id="{uid}-{value}-pros">
              <strong>{t('export.prosLabel')}</strong>
              {t(`export.formatPros.${value}`)}
            </span>
            <span class="format-cons" id="{uid}-{value}-cons">
              <strong>{t('export.consLabel')}</strong>
              {t(`export.formatCons.${value}`)}
            </span>
          </label>
        {/each}
      </div>
      <details class="format-compare">
        <summary>{t('export.compare.title')}</summary>
        <table>
          <thead>
            <tr>
              <th scope="col"><span class="visually-hidden">{t('export.compare.criterion')}</span></th>
              {#each FORMATS as value (value)}
                <th scope="col">{value.toUpperCase()}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each COMPARE_ROWS as row (row)}
              <tr>
                <th scope="row">{t(`export.compare.rows.${row}.label`)}</th>
                {#each FORMATS as value (value)}
                  <td>{t(`export.compare.rows.${row}.${value}`)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted small">{t('export.compare.advice')}</p>
      </details>
    </fieldset>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('export.options')}</legend>
      {#if raster}
        <label class="export-row">
          <span>{t('export.resolution')}</span>
          <select bind:value={options.dpi}>
            {#each DPI_CHOICES as dpi, i (dpi)}
              <option value={dpi} disabled={!fits[i]}>
                {fits[i] ? t(`export.dpi.${dpi}`) : t('export.dpiTooLarge', { label: t(`export.dpi.${dpi}`) })}
              </option>
            {/each}
          </select>
        </label>
        {#if nothingFits}
          <p class="muted small export-hint" role="status">{t('export.rasterTooLarge')}</p>
        {:else if someTooLarge}
          <p class="muted small export-hint">{t('export.dpiLimitHint')}</p>
        {/if}
      {/if}
      {#if format === 'jpg'}
        <label class="export-row">
          <span>{t('export.quality')}</span>
          <input type="range" min="1" max="100" step="1" bind:value={options.quality} />
          <span class="export-value">{options.quality} %</span>
        </label>
      {/if}
      {#if format === 'pdf'}
        <div class="export-row" role="radiogroup" aria-label={t('export.pdfPage')}>
          <span>{t('export.pdfPage')}</span>
          <label class="choice">
            <input type="radio" name="export-pdf-page" value="actual" bind:group={options.pdfPage} />
            {t('export.pdfActual')}
          </label>
          <label class="choice">
            <input type="radio" name="export-pdf-page" value="a4" bind:group={options.pdfPage} />
            {t('export.pdfA4')}
          </label>
        </div>
      {/if}
      <label class="check">
        <input type="checkbox" bind:checked={options.background} />
        {t('export.background')}
      </label>
      <p class="muted small">{t(`export.backgroundHints.${format}`)}</p>
    </fieldset>

    {#if several && layerFormat}
      <fieldset class="export-fieldset" data-slot="export-layers" disabled={running}>
        <legend>{t('export.layers.legend')}</legend>
        {#each LAYER_CHOICES as value (value)}
          <label class="choice layer-choice">
            <input
              type="radio"
              name="export-layers"
              {value}
              bind:group={layers[layerFormat]}
              aria-describedby="{uid}-layers-{value}"
            />
            <span class="layer-text">
              <span class="choice-name">{t(`export.layers.${value}`)}</span>
              <span class="muted small" id="{uid}-layers-{value}">
                {value === 'separate' ? t(`export.layers.separateHint.${layerFormat}`) : t('export.layers.mergedHint')}
              </span>
            </span>
          </label>
        {/each}
      </fieldset>
    {:else if several}
      <p class="muted small" data-slot="export-layers-flat">{t('export.layers.flat')}</p>
    {/if}
    {#if someHidden}
      <p class="muted small" data-slot="export-layers-hidden">{t('export.layers.hiddenNever')}</p>
    {/if}

    <p class="muted small">
      {#if format === 'pdf' && many}
        {t('export.destinationPdfMany')}
      {:else if intoFolder}
        {t('export.destinationFolder')}
      {:else}
        {t('export.destinationFile')}
      {/if}
    </p>

    {#if running}
      <p class="export-status" role="status">{cancelling ? t('export.cancelling') : t('export.running')}</p>
    {:else if phase === 'done'}
      <div class="export-result" role="status">
        <p class="export-status">{t('export.done', { count: written.length })}</p>
        {#if written.length === 1}
          <code>{written[0]}</code>
        {:else}
          <code>{destination}</code>
        {/if}
      </div>
    {/if}
    {#if cancelled}
      <p class="export-notice muted" role="status">{t('export.cancelled')}</p>
    {/if}
    {#if error}
      <p class="banner error inline" role="alert">{t('export.failed', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" data-autofocus disabled={running || (raster && nothingFits)} onclick={run}>
        {phase === 'done' ? t('export.again') : running ? t('export.running') : t('export.run')}
      </button>
      {#if running}
        <button type="button" class="button" disabled={cancelling} onclick={cancel}>
          {cancelling ? t('export.cancelling') : t('export.cancelRun')}
        </button>
      {:else}
        <button type="button" class="button" onclick={onclose}>
          {phase === 'done' ? t('common.close') : t('library.cancel')}
        </button>
      {/if}
    </div>

    <!-- The recognised handwriting, as text (milestone M7). -->
    <TextExportSection {sources} disabled={running} />
  </div>
</Modal>

<style>
  .layer-choice {
    align-items: flex-start;
    margin-top: 0.35rem;
  }

  .layer-text {
    display: flex;
    flex-direction: column;
  }
</style>
