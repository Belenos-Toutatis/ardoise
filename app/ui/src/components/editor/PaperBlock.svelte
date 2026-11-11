<script lang="ts">
  /**
   * The paper of a page (milestone M16): its colour as round swatches, its template and its texture. Used by the
   * editor's Page tab; M17 gives it to live mode's Page tab with the live operation as `onbackground`. A paper that is
   * a custom colour shows one more swatch, pressed and disabled (nothing else can choose it here), as the select's
   * extra option did.
   */
  import { t } from '../../lib/i18n.svelte';
  import { paperRgb } from '../../lib/live-render';
  import type { Background, Paper, Template } from '../../lib/note';

  let {
    background,
    disabled = false,
    onbackground,
  }: {
    background: Background;
    /** The page cannot change now (the split panel is open, a replay runs). */
    disabled?: boolean;
    onbackground: (background: Background) => void;
  } = $props();

  const PAPERS = ['white', 'cream', 'kraft', 'slate'] as const;
  const TEMPLATES: Template[] = ['blank', 'lined', 'seyes', 'grid5mm', 'millimetric', 'dotted', 'music'];

  const custom = $derived(typeof background.paper !== 'string');
  const shown = $derived<readonly string[]>(custom ? [...PAPERS, 'custom'] : PAPERS);

  /** The paper's own colour, as the renderer paints it. */
  function colour(id: string): string {
    const [r, g, b] = paperRgb(id === 'custom' ? background.paper : (id as Paper));
    return `rgb(${r} ${g} ${b})`;
  }
</script>

<div class="pane-swatches" role="group" aria-label={t('editor.page.paper')}>
  {#each shown as id (id)}
    <button
      type="button"
      class="pane-swatch"
      data-paper={id}
      aria-pressed={id === 'custom' ? true : background.paper === id}
      disabled={disabled || id === 'custom'}
      style:--swatch={colour(id)}
      onclick={() => onbackground({ ...background, paper: id as Paper })}
    >
      <span class="paper-dot" aria-hidden="true"></span>
      <span class="paper-name">{t(`editor.papers.${id}`)}</span>
    </button>
  {/each}
</div>
<label class="pane-row">
  <span>{t('editor.page.template')}</span>
  <select
    value={background.template}
    {disabled}
    data-field="template"
    onchange={(e) => onbackground({ ...background, template: e.currentTarget.value as Template })}
  >
    {#each TEMPLATES as template (template)}
      <option value={template}>{t(`editor.templates.${template}`)}</option>
    {/each}
  </select>
</label>
<label class="pane-row">
  <span>{t('editor.page.texture')}</span>
  <input
    type="checkbox"
    role="switch"
    class="pane-switch"
    data-field="texture"
    checked={background.texture}
    {disabled}
    onchange={(e) => onbackground({ ...background, texture: e.currentTarget.checked })}
  />
</label>

<style>
  /* The swatches' look is this block's own (scoped): the paper's colour in a round pastille, its name under it. */
  .pane-swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .pane-swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    min-width: 3.4rem;
    padding: 0.25rem 0.2rem;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }

  .pane-swatch:disabled {
    cursor: default;
  }

  .pane-swatch:disabled:not([aria-pressed='true']) {
    opacity: 0.5;
  }

  .pane-swatch[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .paper-dot {
    width: 1.7rem;
    height: 1.7rem;
    border: 2px solid var(--border);
    border-radius: 50%;
    background: var(--swatch);
  }

  .pane-swatch[aria-pressed='true'] .paper-dot {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .paper-name {
    text-align: center;
  }
</style>
