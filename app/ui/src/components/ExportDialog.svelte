<script lang="ts">
  /**
   * Exports notes to PNG, JPG, SVG or PDF. Used by the editor (the note as it is in memory) and by
   * the library (the selected notes). The destination is asked last, with the native dialog: a
   * file for one note or for a PDF (one page per note), a folder for several images.
   */
  import { api, errorMessage } from '../lib/api';
  import { pickExportFile, pickExportFolder } from '../lib/dialogs';
  import { DPI_CHOICES, FORMATS, exportStem, loadChoice, saveChoice } from '../lib/export-options';
  import { t } from '../lib/i18n.svelte';
  import type { ExportFormat, ExportOptions, ExportSource } from '../lib/note';
  import Modal from './Modal.svelte';

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

  const initial = loadChoice();
  let format = $state<ExportFormat>(initial.format);
  let options = $state<ExportOptions>({ ...initial.options });
  let phase = $state<'idle' | 'running' | 'done'>('idle');
  let error = $state<string | null>(null);
  let written = $state<string[]>([]);
  let destination = $state('');

  const many = $derived(sources.length > 1);
  const intoFolder = $derived(many && format !== 'pdf');
  const running = $derived(phase === 'running');
  const raster = $derived(format === 'png' || format === 'jpg');

  /** The destination dialog's proposed file name. */
  function defaultName(): string {
    const stem = many ? t('export.defaultPdfName') : exportStem(sources[0].path);
    return `${stem}.${format}`;
  }

  async function run() {
    error = null;
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
    destination = chosen;
    try {
      const request = { ...options };
      written = await api.exportNotes(
        sources.map((source) => ({ ...source })),
        format,
        request,
        chosen,
      );
      saveChoice({ format, options: request });
      phase = 'done';
    } catch (e) {
      error = errorMessage(e);
      phase = 'idle';
    }
  }
</script>

<Modal title={t('export.title')} {onclose} dismissible={!running}>
  <div aria-busy={running}>
    <p class="muted">{subject}</p>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('export.format')}</legend>
      <div class="export-choices" role="radiogroup" aria-label={t('export.format')}>
        {#each FORMATS as value (value)}
          <label class="choice">
            <input type="radio" name="export-format" {value} bind:group={format} />
            {t(`export.formats.${value}`)}
          </label>
        {/each}
      </div>
      <p class="muted small">{t(`export.formatHints.${format}`)}</p>
    </fieldset>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('export.options')}</legend>
      {#if raster}
        <label class="export-row">
          <span>{t('export.resolution')}</span>
          <select bind:value={options.dpi}>
            {#each DPI_CHOICES as dpi (dpi)}
              <option value={dpi}>{t(`export.dpi.${dpi}`)}</option>
            {/each}
          </select>
        </label>
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
      <p class="export-status" role="status">{t('export.running')}</p>
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
    {#if error}
      <p class="banner error inline" role="alert">{t('export.failed', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" data-autofocus disabled={running} onclick={run}>
        {phase === 'done' ? t('export.again') : running ? t('export.running') : t('export.run')}
      </button>
      <button type="button" class="button" disabled={running} onclick={onclose}>
        {phase === 'done' ? t('common.close') : t('library.cancel')}
      </button>
    </div>
  </div>
</Modal>
