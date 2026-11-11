<script lang="ts">
  /**
   * The editor's Page tab (milestone M16), in three blocks: the paper (`PaperBlock`: colour, template, texture); the
   * sheet's format (orientation and paper size, the ink stays); turning and cropping (a quarter-turn rotation, the ink
   * turns with the page; crop). The original and the background image are blocks of their own (`OriginalPanel`,
   * `BackgroundImageSection`).
   */
  import { t } from '../../lib/i18n.svelte';
  import {
    MAX_SIDE_MM,
    MIN_SIDE_MM,
    PAPER_SIZE_IDS,
    orientationOf,
    parseMillimetres,
    presetOf,
    presetPage,
    toMillimetres,
    type Orientation,
  } from '../../lib/editor/page-size';
  import type { Background, PageSize } from '../../lib/note';
  import PaperBlock from './PaperBlock.svelte';

  let {
    background,
    page,
    cropping,
    disabled = false,
    onbackground,
    onorientation,
    onresize,
    onrotate,
    onautocrop,
    onmanualcrop,
  }: {
    background: Background;
    page: PageSize;
    cropping: boolean;
    /** The split panel is open or a replay runs: the page cannot change meanwhile. */
    disabled?: boolean;
    onbackground: (background: Background) => void;
    /** Portrait or landscape; `scaleInk`: the writing is scaled with the page instead of staying as it is. */
    onorientation: (orientation: Orientation, scaleInk: boolean) => void;
    /** Another paper size, in the page's orientation. */
    onresize: (size: PageSize, scaleInk: boolean) => void;
    onrotate: (direction: 'cw' | 'ccw') => void;
    onautocrop: () => void;
    onmanualcrop: () => void;
  } = $props();

  const orientation = $derived(orientationOf(page));
  const preset = $derived(presetOf(page));
  /** The writing follows the page's size (otherwise it stays as it is). */
  let scaleInk = $state(false);
  /** Typed sides, millimetres; they follow the page. */
  let widthText = $state('');
  let heightText = $state('');
  $effect(() => {
    widthText = String(toMillimetres(page.width));
    heightText = String(toMillimetres(page.height));
  });
  const typedWidth = $derived(parseMillimetres(widthText));
  const typedHeight = $derived(parseMillimetres(heightText));
  const typedValid = $derived(typedWidth !== null && typedHeight !== null);

  function setPaperSize(value: string) {
    if (value === 'custom') return;
    onresize(presetPage(value as (typeof PAPER_SIZE_IDS)[number], page), scaleInk);
  }

  function applyTyped() {
    if (typedWidth !== null && typedHeight !== null) onresize({ width: typedWidth, height: typedHeight }, scaleInk);
  }
</script>

<section class="pane-block" data-section="paper" aria-labelledby="editor-page-heading">
  <h3 class="pane-title" id="editor-page-heading">{t('editor.page.paperTitle')}</h3>
  <PaperBlock {background} {disabled} {onbackground} />
</section>

<div class="pane-block" data-section="page-format">
  <h3 class="pane-title">{t('editor.page.formatLabel')}</h3>
  <div class="pane-actions" role="group" aria-label={t('editor.page.orientation')}>
    {#each ['portrait', 'landscape'] as const as choice (choice)}
      <button
        type="button"
        class="button small-button"
        class:active={orientation === choice}
        aria-pressed={orientation === choice}
        data-action={`orientation-${choice}`}
        {disabled}
        onclick={() => onorientation(choice, scaleInk)}
      >
        {t(`editor.page.${choice}`)}
      </button>
    {/each}
  </div>
  <p class="pane-help">{t('editor.page.orientationHint')}</p>
  <label class="pane-row">
    <span>{t('editor.page.size')}</span>
    <select value={preset ?? 'custom'} {disabled} data-field="paper-size" onchange={(e) => setPaperSize(e.currentTarget.value)}>
      {#each PAPER_SIZE_IDS as id (id)}
        <option value={id}>{t(`editor.paperSizes.${id}`)}</option>
      {/each}
      {#if preset === null}
        <option value="custom" disabled>{t('editor.paperSizes.custom')}</option>
      {/if}
    </select>
  </label>
  <div class="dimensions">
    <input
      class="text-input"
      type="text"
      inputmode="decimal"
      aria-label={t('editor.page.width')}
      data-field="width-mm"
      aria-invalid={typedWidth === null}
      {disabled}
      bind:value={widthText}
    />
    <span aria-hidden="true">×</span>
    <input
      class="text-input"
      type="text"
      inputmode="decimal"
      aria-label={t('editor.page.height')}
      data-field="height-mm"
      aria-invalid={typedHeight === null}
      {disabled}
      bind:value={heightText}
    />
    <span>mm</span>
    <button
      type="button"
      class="button small-button"
      data-action="apply-size"
      disabled={disabled || !typedValid}
      onclick={applyTyped}
    >
      {t('editor.page.apply')}
    </button>
  </div>
  {#if !typedValid}
    <p class="small error" role="alert">{t('editor.page.sizeInvalid', { min: MIN_SIDE_MM, max: MAX_SIDE_MM })}</p>
  {/if}
  <label class="pane-row">
    <span>{t('editor.page.scaleInk')}</span>
    <input type="checkbox" role="switch" class="pane-switch" bind:checked={scaleInk} {disabled} data-field="scale-ink" />
  </label>
  <p class="pane-help">{scaleInk ? t('editor.page.scaleInkOn') : t('editor.page.scaleInkOff')}</p>
</div>

<section class="pane-block" data-section="turn-crop" aria-labelledby="editor-turn-crop-heading">
  <h3 class="pane-title" id="editor-turn-crop-heading">{t('editor.page.turnCropTitle')}</h3>
  <p class="pane-help">{t('editor.page.rotateHint')}</p>
  <div class="pane-actions">
    <button
      type="button"
      class="button small-button"
      data-action="rotate-ccw"
      {disabled}
      onclick={() => onrotate('ccw')}
      title={t('editor.page.rotateCcw')}
    >
      <span aria-hidden="true">⟲</span>
      {t('editor.page.rotateCcwShort')}
    </button>
    <button
      type="button"
      class="button small-button"
      data-action="rotate-cw"
      {disabled}
      onclick={() => onrotate('cw')}
      title={t('editor.page.rotateCw')}
    >
      <span aria-hidden="true">⟳</span>
      {t('editor.page.rotateCwShort')}
    </button>
  </div>
  <div class="pane-actions">
    <button type="button" class="button small-button" data-action="auto-crop" {disabled} onclick={onautocrop}>
      {t('editor.page.autoCrop')}
    </button>
    <button
      type="button"
      class="button small-button"
      data-action="manual-crop"
      aria-pressed={cropping}
      class:active={cropping}
      {disabled}
      onclick={onmanualcrop}
    >
      {t('editor.page.manualCrop')}
    </button>
  </div>
  {#if cropping}
    <p class="pane-help">{t('editor.page.cropHint')}</p>
  {/if}
</section>

<style>
  .dimensions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.875rem;
  }

  .dimensions .text-input {
    width: 4.2rem;
    padding: 0.25rem 0.4rem;
  }

  .dimensions .text-input[aria-invalid='true'] {
    border-color: var(--danger);
  }
</style>
