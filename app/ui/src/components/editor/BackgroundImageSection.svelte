<script lang="ts">
  /**
   * « Image ou PDF de fond » (SPEC §4.4, P2), the editor's Fond tab (a block of its own since M16): puts an
   * image or a page of a PDF under the writing (to annotate a worksheet, a document, a photo), places it, sets its opacity,
   * removes it, turns it a quarter turn (the page and the writing stay). The file is imported by
   * Rust (`stage_background_image`), which keeps the pixels in memory until the note is saved; each
   * change is one undoable step of the editor. When the image's orientation differs from the
   * note's, the user is offered to switch the note's orientation, in the same step.
   *
   * A PDF of several pages asks which page to use, with a thumbnail of each (`PdfPagePicker`, drawn as they come into
   * view); the first page is chosen to begin with.
   *
   * M17: live mode's Fond tab uses it too (the image goes under the writing of the page on screen, `live_page_image`):
   * `offerOrientation` false (a page of live mode follows the tablet: the image is placed on the page as it is) and
   * its own `hint`; `onset` may be asynchronous, the busy state lasting until it is done.
   */
  import { api, errorMessage } from '../../lib/api';
  import {
    IMAGE_FITS,
    currentFit,
    opacityOf,
    type BackgroundSource,
    type ImageFit,
    type PixelsRef,
    type StagedBackground,
  } from '../../lib/background';
  import { pickBackgroundFile } from '../../lib/dialogs';
  import { importedImage, pixelsOf } from '../../lib/editor/image';
  import { orientationOf, swappedPage, type Orientation } from '../../lib/editor/page-size';
  import { t } from '../../lib/i18n.svelte';
  import type { BackgroundImage, PageSize } from '../../lib/note';
  import PdfPagePicker from './PdfPagePicker.svelte';

  let {
    page,
    image,
    pixels,
    disabled = false,
    offerOrientation = true,
    hint,
    onset,
    onturn,
    onfit,
    onopacity,
    onremove,
  }: {
    page: PageSize;
    image: BackgroundImage | undefined;
    /** The pixels of `image`; absent when they cannot be read (the image is then not drawn). */
    pixels: PixelsRef | undefined;
    disabled?: boolean;
    /** The image of another orientation than the page's offers to switch the page's orientation (the editor); live mode never does. */
    offerOrientation?: boolean;
    /** The sentence under the title (default: the editor's). */
    hint?: string;
    /** The image to put; `orientation` when the note's orientation is to switch in the same step (the image is placed on the switched page). */
    onset: (image: BackgroundImage, pixels: PixelsRef, orientation?: Orientation) => void | Promise<void>;
    /** The image alone turned a quarter turn. */
    onturn: (direction: 'cw' | 'ccw') => void;
    onfit: (fit: ImageFit) => void;
    onopacity: (opacity: number) => void;
    onremove: () => void;
  } = $props();

  /** A PDF of several pages waits for its page to be chosen. */
  let pending = $state<{ path: string; source: BackgroundSource; page: number } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** An imported image waits for the user to say whether the note takes its orientation. */
  let offer = $state.raw<{ staged: StagedBackground; orientation: Orientation } | null>(null);
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
      const orientation = orientationOf(staged);
      if (offerOrientation && staged.width !== staged.height && orientation !== orientationOf(page)) {
        offer = { staged, orientation };
        return;
      }
      await onset(importedImage(page, staged), pixelsOf(staged));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  /** The user's answer to the offer: the note takes the image's orientation, or keeps its own. */
  function answer(switchOrientation: boolean) {
    if (!offer) return;
    const { staged, orientation } = offer;
    offer = null;
    if (switchOrientation) onset(importedImage(swappedPage(page), staged), pixelsOf(staged), orientation);
    else onset(importedImage(page, staged), pixelsOf(staged));
  }

  function commitOpacity(value: number) {
    draft = null;
    onopacity(value / 100);
  }
</script>

<section class="pane-block" data-section="background-image" aria-labelledby="editor-background-image-heading">
  <h3 class="pane-title" id="editor-background-image-heading">{t('background.editor.title')}</h3>
  <p class="pane-help">{hint ?? t('background.editor.hint')}</p>

  {#if offer}
    {@const current = orientationOf(page)}
    <p class="small" role="alert" data-slot="background-orientation-offer">
      {t('background.editor.orientationOffer', {
        image: t(`editor.page.${offer.orientation}Lower`),
        note: t(`editor.page.${current}Lower`),
      })}
    </p>
    <div class="pane-actions">
      <button type="button" class="button small-button primary" data-action="background-switch-orientation" onclick={() => answer(true)}>
        {t('background.editor.switchOrientation', { orientation: t(`editor.page.${offer.orientation}Lower`) })}
      </button>
      <button type="button" class="button small-button" data-action="background-keep-orientation" onclick={() => answer(false)}>
        {t('background.editor.keepOrientation', { orientation: t(`editor.page.${current}Lower`) })}
      </button>
      <button type="button" class="button small-button" onclick={() => (offer = null)}>{t('background.editor.cancel')}</button>
    </div>
  {:else if pending}
    {@const chosen = pending}
    <p class="small" data-slot="background-pdf">
      {t('background.editor.pdfPages', { name: chosen.source.name, count: chosen.source.pages.length })}
    </p>
    <PdfPagePicker
      path={chosen.path}
      name={chosen.source.name}
      pages={chosen.source.pages}
      bind:selected={chosen.page}
      disabled={busy}
    />
    <div class="pane-actions">
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
    <label class="pane-row">
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
    <div class="turn">
      <span>{t('background.editor.turn')}</span>
      <div class="pane-actions">
        <button
          type="button"
          class="button small-button"
          data-action="background-turn-ccw"
          disabled={disabled || busy}
          title={t('background.editor.turnCcw')}
          onclick={() => onturn('ccw')}
        >
          <span aria-hidden="true">⟲</span>
          {t('background.editor.turnCcwShort')}
        </button>
        <button
          type="button"
          class="button small-button"
          data-action="background-turn-cw"
          disabled={disabled || busy}
          title={t('background.editor.turnCw')}
          onclick={() => onturn('cw')}
        >
          <span aria-hidden="true">⟳</span>
          {t('background.editor.turnCwShort')}
        </button>
      </div>
    </div>
    <label class="pane-row opacity">
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
      <span class="pane-value">{percent} %</span>
    </label>
    <div class="pane-actions">
      <button type="button" class="button small-button" disabled={disabled || busy} onclick={() => void choose()}>
        {t('background.editor.replace')}
      </button>
      <button type="button" class="button small-button danger" disabled={disabled || busy} onclick={onremove}>
        {t('background.editor.remove')}
      </button>
    </div>
  {:else}
    <div class="pane-actions">
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
</section>

<style>
  .turn {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.875rem;
  }

  .opacity input {
    flex: 1;
    min-width: 0;
  }

  .opacity .pane-value {
    width: 3.2rem;
    text-align: right;
  }
</style>
