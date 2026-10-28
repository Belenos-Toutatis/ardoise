<script lang="ts">
  /**
   * Paper, template, texture, orientation and paper size (the ink stays), quarter-turn rotation
   * (the ink turns with the page) and crop; then `children` (the background image).
   */
  import type { Snippet } from 'svelte';
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
  import type { Background, Paper, PageSize, Template } from '../../lib/note';

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
    children,
  }: {
    background: Background;
    page: PageSize;
    cropping: boolean;
    /** The split panel is open: the page cannot change meanwhile. */
    disabled?: boolean;
    onbackground: (background: Background) => void;
    /** Portrait or landscape; `scaleInk`: the writing is scaled with the page instead of staying as it is. */
    onorientation: (orientation: Orientation, scaleInk: boolean) => void;
    /** Another paper size, in the page's orientation. */
    onresize: (size: PageSize, scaleInk: boolean) => void;
    onrotate: (direction: 'cw' | 'ccw') => void;
    onautocrop: () => void;
    onmanualcrop: () => void;
    children?: Snippet;
  } = $props();

  const PAPERS = ['white', 'cream', 'kraft', 'slate'] as const;
  const TEMPLATES: Template[] = ['blank', 'lined', 'seyes', 'grid5mm', 'millimetric', 'dotted', 'music'];

  const paperValue = $derived(typeof background.paper === 'string' ? background.paper : 'custom');

  function setPaper(value: string) {
    if (value === 'custom') return;
    onbackground({ ...background, paper: value as Paper });
  }

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

<section class="side-section" aria-labelledby="editor-page-heading">
  <h3 id="editor-page-heading">{t('editor.page.label')}</h3>
  <label class="row">
    <span>{t('editor.page.paper')}</span>
    <select value={paperValue} {disabled} onchange={(e) => setPaper(e.currentTarget.value)}>
      {#each PAPERS as paper}
        <option value={paper}>{t(`editor.papers.${paper}`)}</option>
      {/each}
      {#if paperValue === 'custom'}
        <option value="custom">{t('editor.papers.custom')}</option>
      {/if}
    </select>
  </label>
  <label class="row">
    <span>{t('editor.page.template')}</span>
    <select
      value={background.template}
      {disabled}
      onchange={(e) => onbackground({ ...background, template: e.currentTarget.value as Template })}
    >
      {#each TEMPLATES as template}
        <option value={template}>{t(`editor.templates.${template}`)}</option>
      {/each}
    </select>
  </label>
  <label class="check">
    <input
      type="checkbox"
      checked={background.texture}
      {disabled}
      onchange={(e) => onbackground({ ...background, texture: e.currentTarget.checked })}
    />
    {t('editor.page.texture')}
  </label>
  <div class="group" data-section="page-format">
    <h4>{t('editor.page.formatLabel')}</h4>
    <div class="buttons" role="group" aria-label={t('editor.page.orientation')}>
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
    <p class="muted small">{t('editor.page.orientationHint')}</p>
    <label class="row">
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
    <label class="check">
      <input type="checkbox" bind:checked={scaleInk} {disabled} data-field="scale-ink" />
      {t('editor.page.scaleInk')}
    </label>
    <p class="muted small">{scaleInk ? t('editor.page.scaleInkOn') : t('editor.page.scaleInkOff')}</p>
  </div>
  <div class="group">
    <h4>{t('editor.page.rotateLabel')}</h4>
    <p class="muted small">{t('editor.page.rotateHint')}</p>
    <div class="buttons">
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
  </div>
  <div class="buttons">
    <button type="button" class="button small-button" {disabled} onclick={onautocrop}>{t('editor.page.autoCrop')}</button>
    <button type="button" class="button small-button" aria-pressed={cropping} class:active={cropping} {disabled} onclick={onmanualcrop}>
      {t('editor.page.manualCrop')}
    </button>
  </div>
  {#if cropping}
    <p class="muted small">{t('editor.page.cropHint')}</p>
  {/if}
  {@render children?.()}
</section>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  h4 {
    margin: 0.2rem 0 0;
    font-size: 0.9rem;
  }

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
