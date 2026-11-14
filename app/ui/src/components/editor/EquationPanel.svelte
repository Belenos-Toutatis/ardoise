<script lang="ts">
  /**
   * The equation tool's panel (SPEC §4.4 « Équation », `docs/design/equations.md`): a field for LaTeX code in math mode
   * (chemistry with `\ce{…}`), the live preview of what will be put on the page, and MathJax's error when the code is
   * not valid (then nothing can be put down). The preview is drawn from our own path data (`lib/equation.ts`), never
   * from MathJax's markup. The conversion waits for the typing to pause (`DEBOUNCE_MS`); the first one loads MathJax.
   *
   * Keys: Enter puts the equation down (once its drawing is ready), Shift + Enter starts a new line; Escape is the
   * editor's (`lib/editor/escape.ts`: it drops the equation). The keys typed in the field never reach the editor's
   * shortcuts (the editor ignores keys typed in a field). Its buttons are large enough for a pen or a finger.
   */
  import { onMount } from 'svelte';
  import { convertLatex, type EquationDrawing, type EquationProblem, type EquationResult, type TexConverter } from '../../lib/equation';
  import { t } from '../../lib/i18n.svelte';
  import { cssColor, type Rgba } from '../../lib/note';

  let {
    latex = '',
    editing = false,
    color,
    paper,
    left,
    top,
    width,
    convert,
    onplace,
    oncancel,
  }: {
    /** The source the field starts with (the equation's own when one is rewritten). */
    latex?: string;
    /** An equation of the page is rewritten (« Mettre à jour »), not a new one put down (« Poser »). */
    editing?: boolean;
    /** The colour the equation will have. */
    color: Rgba;
    /** The colour of the note's paper, under the preview. */
    paper: string;
    /** Where the panel goes in the canvas area, pixels. */
    left: number;
    top: number;
    width: number;
    /** LaTeX to drawing (tests); MathJax by default. */
    convert?: TexConverter;
    onplace: (latex: string, drawing: EquationDrawing) => void;
    oncancel: () => void;
  } = $props();

  /** How long the typing must pause before the preview is drawn again, milliseconds. */
  const DEBOUNCE_MS = 150;
  /** Pixels per em of the preview at most (it shrinks to fit the panel). */
  const PREVIEW_EM_PX = 36;

  let field = $state<HTMLTextAreaElement>();
  // The field starts with the given source (the panel is made anew for each equation: `{#key}` in the editor).
  // svelte-ignore state_referenced_locally
  let source = $state(latex);
  /** The result for `drawnFor`, the source it was made from. */
  let result = $state.raw<EquationResult | null>(null);
  let drawnFor = $state<string | null>(null);
  let converting = $state(false);
  /** Enter was pressed while the drawing was being made: put it down as soon as it is ready (and valid). */
  let placeWhenReady = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let request = 0;

  const ready = $derived(result !== null && result.ok && drawnFor === source && !converting);
  const drawing = $derived(result && result.ok && drawnFor === source ? result.drawing : null);
  const problem = $derived(result && !result.ok && drawnFor === source ? result.problem : null);
  const preview = $derived.by(() => {
    if (!drawing) return null;
    const naturalWidth = (drawing.width / 1000) * PREVIEW_EM_PX;
    const naturalHeight = (drawing.height / 1000) * PREVIEW_EM_PX;
    return { naturalWidth, naturalHeight, viewBox: `0 0 ${Math.max(drawing.width, 1)} ${Math.max(drawing.height, 1)}` };
  });

  async function redraw(text: string) {
    const mine = ++request;
    converting = true;
    const next = await convertLatex(text, convert);
    if (mine !== request) return;
    result = next;
    drawnFor = text;
    converting = false;
    if (placeWhenReady) {
      placeWhenReady = false;
      if (next.ok) onplace(text, next.drawing);
    }
  }

  function schedule() {
    clearTimeout(timer);
    // Typed after Enter: what is put down must be what is now in the field, so Enter must be pressed again.
    placeWhenReady = false;
    // Nothing typed: nothing to say yet.
    if (source.trim() === '') {
      request++;
      result = null;
      drawnFor = null;
      converting = false;
      return;
    }
    converting = true;
    const text = source;
    timer = setTimeout(() => void redraw(text), DEBOUNCE_MS);
  }

  function place() {
    if (ready && drawing) {
      onplace(source, drawing);
      return;
    }
    if (converting) {
      // The drawing is being made: Enter puts it down as soon as it is ready, if it is valid.
      placeWhenReady = true;
      if (timer !== undefined && drawnFor !== source) {
        clearTimeout(timer);
        timer = undefined;
        void redraw(source);
      }
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      place();
    }
  }

  /** What the panel says of a problem. */
  function describe(p: EquationProblem): string {
    switch (p.kind) {
      case 'empty':
        return t('editor.equation.problems.empty');
      case 'tooLong':
        return t('editor.equation.problems.tooLong', { max: p.max });
      case 'latex':
        return t('editor.equation.problems.latex', { message: p.message });
      case 'nothingDrawn':
        return t('editor.equation.problems.nothingDrawn');
      case 'missingGlyph':
        return t('editor.equation.problems.missingGlyph', { chars: p.chars });
      case 'unsupported':
        return p.what === 'tag' ? t('editor.equation.problems.tag') : t('editor.equation.problems.unsupported', { what: p.what });
      case 'tooLarge':
        return t('editor.equation.problems.tooLarge');
      case 'unavailable':
        return t('editor.equation.problems.unavailable', { message: p.message });
    }
  }

  onMount(() => {
    field?.focus({ preventScroll: true });
    field?.setSelectionRange(source.length, source.length);
    if (source.trim() !== '') void redraw(source);
    return () => {
      clearTimeout(timer);
      request++;
    };
  });
</script>

<div
  class="equation-panel"
  role="dialog"
  aria-modal="false"
  aria-labelledby="equation-panel-title"
  data-slot="equation-panel"
  data-state={ready ? 'ready' : converting ? 'converting' : problem ? 'error' : 'empty'}
  style="left: {left}px; top: {top}px; width: {width}px;"
>
  <h3 class="title" id="equation-panel-title">{editing ? t('editor.equation.titleEdit') : t('editor.equation.title')}</h3>
  <label class="field-label" for="equation-source">{t('editor.equation.field')}</label>
  <textarea
    bind:this={field}
    bind:value={source}
    id="equation-source"
    class="source"
    data-slot="equation-source"
    rows="3"
    spellcheck="false"
    autocomplete="off"
    autocapitalize="off"
    placeholder={t('editor.equation.placeholder')}
    aria-describedby="equation-panel-hint equation-panel-status"
    oninput={schedule}
    {onkeydown}
  ></textarea>
  <div class="preview" style:background={paper} data-slot="equation-preview">
    {#if preview && drawing}
      <svg
        role="img"
        aria-label={t('editor.equation.previewLabel', { latex: source })}
        viewBox={preview.viewBox}
        width={preview.naturalWidth}
        height={preview.naturalHeight}
        preserveAspectRatio="xMidYMid meet"
      >
        <path d={drawing.path} fill={cssColor(color)} fill-rule="nonzero" />
      </svg>
    {:else}
      <span class="preview-empty">{t('editor.equation.previewEmpty')}</span>
    {/if}
  </div>
  <p class="status" id="equation-panel-status" role="status" aria-live="polite" class:error={problem !== null} data-slot="equation-status">
    {#if problem}
      {describe(problem)}
    {:else if converting}
      {t('editor.equation.converting')}
    {/if}
  </p>
  <p class="hint" id="equation-panel-hint">{t('editor.equation.hint')}</p>
  <p class="hint examples">{t('editor.equation.examples')}</p>
  <div class="actions">
    <button type="button" class="button" data-action="equation-cancel" onclick={oncancel}>{t('editor.equation.cancel')}</button>
    <button type="button" class="button primary" data-action="equation-place" disabled={!ready && !converting} onclick={place}>
      {editing ? t('editor.equation.update') : t('editor.equation.place')}
    </button>
  </div>
</div>

<style>
  .equation-panel {
    position: absolute;
    z-index: 4;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 8px 24px rgb(0 0 0 / 22%);
  }

  .title {
    margin: 0;
    font-size: 0.95rem;
  }

  .field-label {
    font-size: 0.85rem;
    color: var(--muted);
  }

  .source {
    box-sizing: border-box;
    width: 100%;
    min-height: 3.5rem;
    padding: 0.45rem 0.55rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--text);
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 0.95rem;
    resize: vertical;
  }

  .source:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .preview {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 4.5rem;
    max-height: 10rem;
    padding: 0.5rem;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .preview svg {
    max-width: 100%;
    max-height: 9rem;
    height: auto;
  }

  .preview-empty {
    color: #6b6b6b;
    font-size: 0.85rem;
  }

  .status {
    min-height: 1.2em;
    margin: 0;
    font-size: 0.85rem;
    color: var(--muted);
  }

  .status.error {
    color: var(--danger);
  }

  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--muted);
  }

  .examples {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  .actions .button {
    min-height: 2.5rem;
    min-width: 6rem;
  }
</style>
