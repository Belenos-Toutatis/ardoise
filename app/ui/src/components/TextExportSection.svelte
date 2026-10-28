<script lang="ts">
  /**
   * The export dialog's "recognised text" part: the handwriting of the note (or of the selected
   * notes) read by Windows' recognition, saved as a UTF-8 `.txt` file (one note after the other,
   * each with its title), as Markdown or as a Word document (with the structure drawn from the
   * ink: paragraphs, list items, drawings), or copied to the clipboard, with a preview. The dialog
   * serves the editor (the note as it is in memory) and the library selection alike.
   */
  import { api, errorMessage } from '../lib/api';
  import { pickExportFile } from '../lib/dialogs';
  import { exportStem } from '../lib/export-options';
  import { textFileName, type NoteText, type StructuredFormat, type TextLabels } from '../lib/handwriting';
  import { t } from '../lib/i18n.svelte';
  import type { ExportSource } from '../lib/note';

  let {
    sources,
    disabled = false,
  }: {
    sources: ExportSource[];
    /** The image export is running. */
    disabled?: boolean;
  } = $props();

  let phase = $state<'idle' | 'saving' | 'copying'>('idle');
  let done = $state<{ kind: 'saved'; path: string; images: number } | { kind: 'copied' } | null>(null);
  /** The layout of the last structured export was guessed from the text (no Windows analysis). */
  let guessed = $state(false);
  let error = $state<string | null>(null);
  let preview = $state.raw<NoteText[] | null>(null);
  let previewLoading = $state(false);
  let previewError = $state<string | null>(null);

  const busy = $derived(disabled || phase !== 'idle');

  /** Fresh copies: the editor's note is reactive state. */
  function requestSources(): ExportSource[] {
    return sources.map((source) => ({ ...source }));
  }

  function stem(): string {
    return sources.length === 1 ? exportStem(sources[0].path) : t('handwriting.export.defaultName');
  }

  function defaultName(format: 'txt' | StructuredFormat): string {
    return format === 'txt' ? textFileName(stem()) : `${stem()}.${format}`;
  }

  /** The words the Markdown and Word files contain, in the interface language. */
  function labels(): TextLabels {
    return {
      date: t('handwriting.export.labels.date'),
      tags: t('handwriting.export.labels.tags'),
      drawing: t('handwriting.export.labels.drawing'),
    };
  }

  async function save(format: 'txt' | StructuredFormat = 'txt') {
    error = null;
    done = null;
    guessed = false;
    const title = { txt: 'pickFile', md: 'pickMd', docx: 'pickDocx' }[format];
    let chosen: string | null;
    try {
      chosen = await pickExportFile(t(`handwriting.export.${title}`), defaultName(format), format);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    if (!chosen) return;
    phase = 'saving';
    try {
      if (format === 'txt') {
        done = { kind: 'saved', path: await api.exportHandwritingText(requestSources(), chosen), images: 0 };
      } else {
        const result = await api.exportStructuredText(requestSources(), format, chosen, labels());
        done = { kind: 'saved', path: result.paths[0] ?? chosen, images: Math.max(0, result.paths.length - 1) };
        guessed = !result.analyzed;
      }
    } catch (e) {
      error = errorMessage(e);
    } finally {
      phase = 'idle';
    }
  }

  async function copy() {
    error = null;
    done = null;
    phase = 'copying';
    try {
      await api.copyHandwritingText(requestSources());
      done = { kind: 'copied' };
    } catch (e) {
      error = errorMessage(e);
    } finally {
      phase = 'idle';
    }
  }

  async function togglePreview(event: Event) {
    if (!(event.currentTarget as HTMLDetailsElement).open || preview || previewLoading) return;
    previewLoading = true;
    previewError = null;
    try {
      preview = await api.handwritingText(requestSources());
    } catch (e) {
      previewError = errorMessage(e);
    } finally {
      previewLoading = false;
    }
  }
</script>

<section class="text-export" aria-labelledby="text-export-title" aria-busy={phase !== 'idle'}>
  <h3 id="text-export-title" class="text-export-title">{t('handwriting.export.title')}</h3>
  <p class="muted small">{t('handwriting.export.hint')}</p>
  <div class="text-export-actions">
    <button type="button" class="button" data-action="text-save" disabled={busy} onclick={() => save('txt')}>
      {phase === 'saving' ? t('handwriting.export.working') : t('handwriting.export.save')}
    </button>
    <button type="button" class="button" data-action="text-md" disabled={busy} onclick={() => save('md')}>
      {t('handwriting.export.saveMd')}
    </button>
    <button type="button" class="button" data-action="text-docx" disabled={busy} onclick={() => save('docx')}>
      {t('handwriting.export.saveDocx')}
    </button>
    <button type="button" class="button" data-action="text-copy" disabled={busy} onclick={copy}>
      {phase === 'copying' ? t('handwriting.export.working') : t('handwriting.export.copy')}
    </button>
  </div>
  {#if done?.kind === 'saved'}
    <div class="export-result" role="status">
      <p class="export-status">{t('handwriting.export.saved')}</p>
      <code>{done.path}</code>
      {#if done.images > 0}
        <p class="muted small">{t('handwriting.export.savedImages', { count: done.images })}</p>
      {/if}
    </div>
    {#if guessed}
      <p class="muted small" role="status">{t('handwriting.export.guessed')}</p>
    {/if}
  {:else if done?.kind === 'copied'}
    <p class="export-status" role="status">{t('handwriting.export.copied')}</p>
  {/if}
  {#if error}
    <p class="banner error inline" role="alert">{t('handwriting.export.failed', { message: error })}</p>
  {/if}
  <details class="text-preview" ontoggle={togglePreview}>
    <summary>{t('handwriting.export.preview')}</summary>
    {#if previewLoading}
      <p class="muted small">{t('handwriting.export.previewLoading')}</p>
    {:else if previewError}
      <p class="error small" role="alert">{previewError}</p>
    {:else if preview}
      {#each preview as note (note.path)}
        <h4 class="text-preview-title">{note.title}</h4>
        {#if note.text}
          <!-- A scrollable text must be reachable and scrollable with the keyboard. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre tabindex="0">{note.text}</pre>
        {:else}
          <p class="muted small">{t('handwriting.export.empty')}</p>
        {/if}
      {/each}
    {/if}
  </details>
</section>

<style>
  .text-export {
    margin-top: 1.1rem;
    padding-top: 0.9rem;
    border-top: 1px solid var(--border);
  }

  .text-export-title {
    margin: 0 0 0.3rem;
    font-size: 0.95rem;
  }

  .text-export-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin: 0.4rem 0;
  }

  .text-preview {
    margin-top: 0.5rem;
  }

  .text-preview-title {
    margin: 0.6rem 0 0.2rem;
    font-size: 0.9rem;
  }

  .text-preview pre {
    max-height: 10rem;
    overflow: auto;
    margin: 0;
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    white-space: pre-wrap;
    font-family: inherit;
  }
</style>
