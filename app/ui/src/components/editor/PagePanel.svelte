<script lang="ts">
  /** Paper, template, texture, quarter-turn rotation and crop. */
  import { t } from '../../lib/i18n.svelte';
  import type { Background, Paper, Template } from '../../lib/note';

  let {
    background,
    cropping,
    disabled = false,
    onbackground,
    onrotate,
    onautocrop,
    onmanualcrop,
  }: {
    background: Background;
    cropping: boolean;
    /** The split panel is open: the page cannot change meanwhile. */
    disabled?: boolean;
    onbackground: (background: Background) => void;
    onrotate: (direction: 'cw' | 'ccw') => void;
    onautocrop: () => void;
    onmanualcrop: () => void;
  } = $props();

  const PAPERS = ['white', 'cream', 'kraft', 'slate'] as const;
  const TEMPLATES: Template[] = ['blank', 'lined', 'seyes', 'grid5mm', 'millimetric', 'dotted', 'music'];

  const paperValue = $derived(typeof background.paper === 'string' ? background.paper : 'custom');

  function setPaper(value: string) {
    if (value === 'custom') return;
    onbackground({ ...background, paper: value as Paper });
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
  <div class="buttons">
    <button type="button" class="button small-button" {disabled} onclick={() => onrotate('ccw')} title={t('editor.page.rotateCcw')}>
      <span aria-hidden="true">⟲</span>
      {t('editor.page.rotateCcwShort')}
    </button>
    <button type="button" class="button small-button" {disabled} onclick={() => onrotate('cw')} title={t('editor.page.rotateCw')}>
      <span aria-hidden="true">⟳</span>
      {t('editor.page.rotateCwShort')}
    </button>
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
</section>
