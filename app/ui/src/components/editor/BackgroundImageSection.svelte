<script lang="ts">
  /**
   * « Image ou PDF de fond » (SPEC §4.4, P2), in the page panel: puts an image or a page of a PDF
   * under the writing (to annotate a worksheet, a document, a photo), places it, sets its opacity,
   * removes it. The file is imported by Rust (`stage_background_image`), which keeps the pixels in
   * memory until the note is saved; each change is one undoable step of the editor.
   */
  import { api, errorMessage } from '../../lib/api';
  import { IMAGE_FITS, currentFit, opacityOf, type BackgroundSource, type ImageFit, type PixelsRef } from '../../lib/background';
  import { pickBackgroundFile } from '../../lib/dialogs';
  import { importedImage, pixelsOf } from '../../lib/editor/image';
  import { t } from '../../lib/i18n.svelte';
  import type { BackgroundImage, PageSize } from '../../lib/note';

  let {
    page,
    image,
    pixels,
    disabled = false,
    onset,
    onfit,
    onopacity,
    onremove,
  }: {
    page: PageSize;
    image: BackgroundImage | undefined;
    /** The pixels of `image`; absent when they cannot be read (the image is then not drawn). */
    pixels: PixelsRef | undefined;
    disabled?: boolean;
    onset: (image: BackgroundImage, pixels: PixelsRef) => void;
    onfit: (fit: ImageFit) => void;
    onopacity: (opacity: number) => void;
    onremove: () => void;
  } = $props();

  /** A PDF of several pages waits for its page to be chosen. */
  let pending = $state<{ path: string; source: BackgroundSource; page: number } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** The slider's position while it is dragged (the change is made once, when it is let go). */
  let draft = $state<number | null>(null);

  const fit = $derived(image && pixels ? currentFit(image, page, pixels) : null);
  const percent = $derived(draft ?? Math.round((image ? opacityOf(image) : 1) * 100));

  async function choose() {
    error = null;
    const path = await pickBackgroundFile(t('background.editor.pickTitle'), t('background.fileFilter'));
    if (!path) return;
    busy = true;
    try {
      const source = await api.inspectBackgroundFile(path);
      if (source.kind === 'pdf' && source.pages.length > 1) {
        pending = { path, source, page: 1 };
        return;
      }
      await place(path, source.kind === 'pdf' ? 1 : undefined);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function place(path: string, pdfPage?: number) {
    error = null;
    busy = true;
    try {
      const staged = await api.stageBackgroundImage(path, pdfPage);
      pending = null;
      onset(importedImage(page, staged), pixelsOf(staged));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function commitOpacity(value: number) {
    draft = null;
    onopacity(value / 100);
  }
</script>

<div class="background-image" data-section="background-image">
  <h4 id="editor-background-image-heading">{t('background.editor.title')}</h4>
  <p class="muted small">{t('background.editor.hint')}</p>

  {#if pending}
    {@const chosen = pending}
    <p class="small" data-slot="background-pdf">
      {t('background.editor.pdfPages', { name: chosen.source.name, count: chosen.source.pages.length })}
    </p>
    <label class="row">
      <span>{t('background.editor.page')}</span>
      <select bind:value={chosen.page} disabled={busy}>
        {#each chosen.source.pages as _, i (i)}
          <option value={i + 1}>{i + 1}</option>
        {/each}
      </select>
    </label>
    <div class="buttons">
      <button type="button" class="button small-button primary" disabled={busy} onclick={() => void place(chosen.path, chosen.page)}>
        {t('background.editor.place')}
      </button>
      <button type="button" class="button small-button" disabled={busy} onclick={() => (pending = null)}>
        {t('background.editor.cancel')}
      </button>
    </div>
  {:else if image}
    <p class="small" data-slot="background-image-name">
      {image.pdfPage === undefined ? image.name : t('background.editor.pdfPageName', { name: image.name, page: image.pdfPage })}
    </p>
    {#if !pixels}
      <p class="small error" role="alert">{t('background.editor.unreadable')}</p>
    {/if}
    <label class="row">
      <span>{t('background.editor.fit')}</span>
      <select
        value={fit ?? 'custom'}
        disabled={disabled || busy || !pixels}
        onchange={(e) => {
          const value = e.currentTarget.value;
          if (value !== 'custom') onfit(value as ImageFit);
        }}
      >
        {#each IMAGE_FITS as choice (choice)}
          <option value={choice}>{t(`background.fits.${choice}`)}</option>
        {/each}
        {#if fit === null}
          <option value="custom" disabled>{t('background.fits.custom')}</option>
        {/if}
      </select>
    </label>
    <label class="row opacity">
      <span>{t('background.editor.opacity')}</span>
      <input
        type="range"
        min="0"
        max="100"
        step="5"
        value={percent}
        disabled={disabled || busy}
        aria-valuetext={`${percent} %`}
        oninput={(e) => (draft = Number(e.currentTarget.value))}
        onchange={(e) => commitOpacity(Number(e.currentTarget.value))}
      />
      <span class="value">{percent} %</span>
    </label>
    <div class="buttons">
      <button type="button" class="button small-button" disabled={disabled || busy} onclick={() => void choose()}>
        {t('background.editor.replace')}
      </button>
      <button type="button" class="button small-button danger" disabled={disabled || busy} onclick={onremove}>
        {t('background.editor.remove')}
      </button>
    </div>
  {:else}
    <div class="buttons">
      <button type="button" class="button small-button" data-action="background-choose" disabled={disabled || busy} onclick={() => void choose()}>
        {t('background.editor.choose')}
      </button>
    </div>
  {/if}

  {#if busy}
    <p class="small muted" role="status">{t('background.editor.importing')}</p>
  {/if}
  {#if error}
    <p class="small error" role="alert">{t('background.editor.failed', { message: error })}</p>
  {/if}
</div>

<style>
  .background-image {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.35rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
  }

  h4 {
    margin: 0;
    font-size: 0.9rem;
  }

  .opacity input {
    flex: 1;
    min-width: 0;
  }

  .opacity .value {
    width: 3.2rem;
    text-align: right;
  }
</style>
