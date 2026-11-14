<script lang="ts">
  /**
   * Tools and style: mode (select, pen, the shape tools, eraser, move the page), ink tool, colour (Inkspace palette +
   * custom colours), width, eraser. With a shape tool, three toggles (`shapePrefs`): regular, from the centre, snap to
   * the page's template (docs/design/shape-tools.md). With the text tool (or text frames selected), the family, the size
   * and bold (docs/design/page-objects.md §5); their buttons and the colours do not take the focus from the frame being
   * typed (`keepFocus`).
   */
  import { INKSPACE_COLOR_NAMES, sameColor, toHex, fromHex } from '../../lib/editor/colors';
  import WidthControl from '../WidthControl.svelte';
  import { shapePrefs, type ShapeToggle } from '../../lib/editor/shape-prefs.svelte';
  import { isShapeKind } from '../../lib/editor/shape-tools';
  import { textSettings, type EraserMode, type Mode, type TextSettings, type ToolSettings } from '../../lib/editor/state';
  import { CSS_FAMILY, TEXT_SIZES, TEXT_SIZE_NAMES } from '../../lib/text';
  import { FONT_FAMILIES } from '../../lib/note';
  import { t } from '../../lib/i18n.svelte';
  import { TOOLS, type Tool } from '../../lib/ink';
  import { INKSPACE_COLORS, cssColor, type Rgba, type ShapeSettings } from '../../lib/note';
  import StraightenPanel from './StraightenPanel.svelte';

  let {
    tools,
    customColors,
    hasSelection,
    onmode,
    ontool,
    oncolor,
    oncustomcolor,
    onwidth,
    oneraser,
    ontext,
    textTargets = 0,
    onapplyall,
    splitting = false,
    locked = false,
    copying = false,
    replaying = false,
    replayDisabled = false,
    resumeDisabled = false,
    resumeHint = null,
    onsplit,
    oncopy,
    onexport,
    onprint,
    onreplay,
    onvideo,
    onresume,
    onstraighten,
    shapes,
    shapesOpen = false,
    shapesSelected = 0,
    shapesFound = 0,
    snapAvailable = false,
    onshapesopen,
    onshapes,
    onshapesdefaults,
  }: {
    tools: ToolSettings;
    customColors: Rgba[];
    hasSelection: boolean;
    onmode: (mode: Mode) => void;
    ontool: (tool: Tool) => void;
    oncolor: (color: Rgba) => void;
    oncustomcolor: (color: Rgba) => void;
    /** `commit` is false while a slider moves, true when it is released. */
    onwidth: (width: number, commit: boolean) => void;
    oneraser: (change: { mode?: EraserMode; radius?: number }) => void;
    /** The text tool's family, size or bold changed (the frame typed in, or the selected frames, take it too). */
    ontext?: (change: Partial<TextSettings>) => void;
    /** How many text frames the text options act on now (the one typed in, or the selected ones). */
    textTargets?: number;
    onapplyall: (what: 'color' | 'tool' | 'width') => void;
    /** The split panel is open. */
    splitting?: boolean;
    /** Style and tool changes wait (the split panel is open: edits would be lost when it ends). */
    locked?: boolean;
    /** The image is being copied to the clipboard. */
    copying?: boolean;
    /** The replay mode is on: the page is read-only. */
    replaying?: boolean;
    /** The replay cannot be switched now (the split panel is open, or a narration is being recorded). */
    replayDisabled?: boolean;
    onsplit: () => void;
    oncopy: () => void;
    onexport: () => void;
    onprint: () => void;
    onreplay: () => void;
    onvideo: () => void;
    /** « Reprendre en mode direct » (M12): absent, no such button. */
    onresume?: () => void;
    /** Live mode cannot resume the note now. */
    resumeDisabled?: boolean;
    /** Why, when it cannot (no tablet registered, the tablet is busy). */
    resumeHint?: string | null;
    /** "Redresser les formes" on the selection. */
    onstraighten?: () => void;
    /** The setting of "Redresser les formes" (M15); the panel next to its button changes it. */
    shapes: ShapeSettings;
    /** The setting's panel is open. */
    shapesOpen?: boolean;
    /** How many strokes are selected, and how many shapes they give with `shapes` (the panel says it). */
    shapesSelected?: number;
    shapesFound?: number;
    /** The page has a template with lines to snap the shapes to (« Aimanter au quadrillage » is enabled). */
    snapAvailable?: boolean;
    /** The panel opens or closes (the small button, or a click outside it). */
    onshapesopen?: (open: boolean) => void;
    onshapes?: (next: ShapeSettings) => void;
    onshapesdefaults?: () => void;
  } = $props();

  let split = $state<HTMLDivElement>();
  let shapesButton = $state<HTMLButtonElement>();

  /** Puts the focus back on the small button (the panel closed with Escape). */
  export function focusShapesButton() {
    shapesButton?.focus();
  }

  // A press anywhere outside the button and its panel closes the panel.
  $effect(() => {
    if (!shapesOpen) return;
    const away = (e: Event) => {
      if (split && e.target instanceof Node && !split.contains(e.target)) onshapesopen?.(false);
    };
    window.addEventListener('pointerdown', away, true);
    return () => window.removeEventListener('pointerdown', away, true);
  });

  /** `icon`: a glyph; the shape tools have a drawn one (`ShapeIcon`). */
  const MODES: { mode: Mode; icon?: string }[] = [
    { mode: 'select', icon: '⬚' },
    { mode: 'pen', icon: '✎' },
    { mode: 'line' },
    { mode: 'rectangle' },
    { mode: 'ellipse' },
    { mode: 'text', icon: 'T' },
    { mode: 'bookmark', icon: '⚑' },
    { mode: 'eraser', icon: '⌫' },
    { mode: 'pan', icon: '✋' },
  ];

  const text = $derived(textSettings(tools));
  /** The text options show with the text tool, or when text frames are selected. */
  const textOptions = $derived(tools.mode === 'text' || textTargets > 0);

  /** A press on a text option or a colour keeps the focus where it is: in the frame being typed. */
  function keepFocus(e: PointerEvent) {
    if (e.button === 0) e.preventDefault();
  }

  const SHAPE_TOGGLES: ShapeToggle[] = ['regular', 'centre', 'snap'];

  /** A toggle is pressed; the snap one is not while the page has nothing to snap to. */
  const pressed = (toggle: ShapeToggle) => shapePrefs[toggle] && (toggle !== 'snap' || snapAvailable);

  const mm = (units: number) => (units / 100).toFixed(units < 100 ? 2 : 1);

  function pickCustom(e: Event) {
    const color = fromHex((e.currentTarget as HTMLInputElement).value);
    if (color) oncustomcolor(color);
  }
</script>

<div class="toolbar" role="toolbar" aria-label={t('editor.toolbar')}>
  <div class="group" role="group" aria-label={t('editor.modes.label')}>
    {#each MODES as { mode, icon }}
      <button
        type="button"
        class="button tool"
        class:active={tools.mode === mode}
        aria-pressed={tools.mode === mode}
        disabled={locked}
        title={t(`editor.modes.${mode}`)}
        onclick={() => onmode(mode)}
      >
        {#if icon}
          <span aria-hidden="true">{icon}</span>
        {:else}
          <svg class="shape-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
            {#if mode === 'line'}
              <path d="M5 19 19 5" />
            {:else if mode === 'rectangle'}
              <rect x="4" y="6" width="16" height="12" rx="1" />
            {:else}
              <ellipse cx="12" cy="12" rx="8" ry="6" />
            {/if}
          </svg>
        {/if}
        <span class="tool-label">{t(`editor.modes.${mode}`)}</span>
      </button>
    {/each}
  </div>

  {#if isShapeKind(tools.mode)}
    <div class="group shape-options" role="group" aria-label={t('editor.shapeTools.label')} data-slot="shape-options">
      {#each SHAPE_TOGGLES as toggle}
        {@const off = toggle === 'snap' && !snapAvailable}
        <button
          type="button"
          class="button small-button toggle"
          class:active={pressed(toggle)}
          aria-pressed={pressed(toggle)}
          disabled={locked || off}
          data-action="shape-{toggle}"
          title={off ? t('editor.shapeTools.snapDisabled') : t(`editor.shapeTools.${toggle}Hint`)}
          onclick={() => shapePrefs.toggle(toggle)}
        >
          <svg class="shape-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
            {#if toggle === 'regular'}
              <rect x="3" y="3" width="10" height="10" />
              <circle cx="16.5" cy="16.5" r="4.5" />
            {:else if toggle === 'centre'}
              <circle cx="12" cy="12" r="2" />
              <path d="M12 3v4M12 17v4M3 12h4M17 12h4" />
            {:else}
              <path d="M5 5h14M5 12h14M5 19h14M5 5v14M12 5v14M19 5v14" />
              <circle cx="12" cy="12" r="2.6" fill="currentColor" />
            {/if}
          </svg>
          {t(`editor.shapeTools.${toggle}`)}
        </button>
      {/each}
      {#if !snapAvailable}
        <span class="label" data-slot="shape-snap-hint">{t('editor.shapeTools.snapNone')}</span>
      {/if}
    </div>
  {/if}

  {#if textOptions && tools.mode !== 'eraser'}
    <div class="group text-options" role="group" aria-label={t('editor.text.label')} data-slot="text-options">
      <div class="segmented" role="group" aria-label={t('editor.text.family')}>
        {#each FONT_FAMILIES as font}
          <button
            type="button"
            class="button small-button toggle"
            class:active={text.font === font}
            aria-pressed={text.font === font}
            disabled={locked}
            data-action="text-font-{font}"
            style:font-family={`"${CSS_FAMILY[font]}", sans-serif`}
            onpointerdown={keepFocus}
            onclick={() => ontext?.({ font })}
          >
            {t(`editor.text.fonts.${font}`)}
          </button>
        {/each}
      </div>
      <div class="segmented" role="group" aria-label={t('editor.text.size')}>
        {#each TEXT_SIZE_NAMES as size}
          <button
            type="button"
            class="button small-button toggle"
            class:active={text.size === TEXT_SIZES[size]}
            aria-pressed={text.size === TEXT_SIZES[size]}
            disabled={locked}
            data-action="text-size-{size}"
            title={t(`editor.text.sizes.${size}Hint`)}
            onpointerdown={keepFocus}
            onclick={() => ontext?.({ size: TEXT_SIZES[size] })}
          >
            {t(`editor.text.sizes.${size}`)}
          </button>
        {/each}
      </div>
      <button
        type="button"
        class="button small-button toggle bold"
        class:active={text.bold}
        aria-pressed={text.bold}
        disabled={locked}
        data-action="text-bold"
        onpointerdown={keepFocus}
        onclick={() => ontext?.({ bold: !text.bold })}
      >
        {t('editor.text.bold')}
      </button>
      {#if tools.mode === 'text'}
        <span class="label text-help" data-slot="text-help">{t('editor.text.help')}</span>
      {/if}
    </div>
  {/if}

  {#if tools.mode === 'bookmark'}
    <div class="group" data-slot="bookmark-options">
      <span class="label" data-slot="bookmark-help">{t('editor.bookmark.help')}</span>
    </div>
  {/if}

  {#if tools.mode === 'eraser'}
    <div class="group" role="radiogroup" aria-label={t('editor.eraser.label')}>
      {#each ['whole', 'partial'] as const as mode}
        <label class="choice">
          <input
            type="radio"
            name="eraser-mode"
            checked={tools.eraser === mode}
            disabled={locked}
            onchange={() => oneraser({ mode })}
          />
          {t(`editor.eraser.${mode}`)}
        </label>
      {/each}
    </div>
    <label class="group slider">
      <span>{t('editor.eraser.size')}</span>
      <input
        type="range"
        min="50"
        max="2000"
        step="10"
        value={tools.eraserRadius}
        disabled={locked}
        oninput={(e) => oneraser({ radius: Number(e.currentTarget.value) })}
      />
      <span class="value">{t('editor.width.value', { mm: mm(tools.eraserRadius * 2) })}</span>
    </label>
  {:else}
    {#if tools.mode !== 'text'}
    <label class="group">
      <span class="label">{t('editor.ink.label')}</span>
      <select value={tools.tool} disabled={locked} onchange={(e) => ontool(e.currentTarget.value as Tool)}>
        {#each TOOLS as tool}
          <option value={tool}>{t(`editor.ink.tools.${tool}`)}</option>
        {/each}
      </select>
    </label>
    {/if}

    <div class="group swatches" role="group" aria-label={t('editor.color.label')}>
      {#each INKSPACE_COLORS as color, i}
        <button
          type="button"
          class="swatch"
          class:active={sameColor(color, tools.color)}
          style:background={cssColor(color)}
          aria-pressed={sameColor(color, tools.color)}
          disabled={locked}
          aria-label={t(`editor.colors.${INKSPACE_COLOR_NAMES[i]}`)}
          title={t(`editor.colors.${INKSPACE_COLOR_NAMES[i]}`)}
          onpointerdown={keepFocus}
          onclick={() => oncolor(color)}
        ></button>
      {/each}
      {#each customColors as color}
        <button
          type="button"
          class="swatch"
          class:active={sameColor(color, tools.color)}
          style:background={cssColor(color)}
          aria-pressed={sameColor(color, tools.color)}
          disabled={locked}
          aria-label={t('editor.color.customSwatch', { hex: toHex(color) })}
          title={t('editor.color.customSwatch', { hex: toHex(color) })}
          onpointerdown={keepFocus}
          onclick={() => oncolor(color)}
        ></button>
      {/each}
      <label class="custom" title={t('editor.color.custom')}>
        <span class="visually-hidden">{t('editor.color.custom')}</span>
        <input type="color" value={toHex(tools.color)} disabled={locked} onchange={pickCustom} />
      </label>
    </div>

    {#if tools.mode !== 'text'}
    <div class="group slider" role="group" aria-label={t('editor.width.label')}>
      <span>{t('editor.width.label')}</span>
      <WidthControl
        value={tools.width}
        disabled={locked}
        label={t('editor.width.label')}
        oninput={(width) => onwidth(width, false)}
        onchange={(width) => onwidth(width, true)}
      />
    </div>

    <div class="group apply" role="group" aria-label={t('editor.applyAll.label')}>
      <span class="label">{hasSelection ? t('editor.applyAll.selectionHint') : t('editor.applyAll.label')}</span>
      {#if !hasSelection}
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('color')}>{t('editor.applyAll.color')}</button>
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('tool')}>{t('editor.applyAll.tool')}</button>
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('width')}>{t('editor.applyAll.width')}</button>
      {/if}
    </div>
    {/if}
    {#if tools.mode === 'select'}
      <div class="group">
        <!-- The split button: straighten on the left, its setting on the right (M15). -->
        <div class="split" bind:this={split}>
          <button
            type="button"
            class="button small-button"
            data-action="straighten"
            disabled={locked || !hasSelection}
            title={hasSelection ? t('editor.straighten.hint') : t('editor.straighten.disabledHint')}
            onclick={() => onstraighten?.()}
          >
            <span aria-hidden="true">△</span>
            {t('editor.straighten.button')}
          </button>
          <button
            bind:this={shapesButton}
            type="button"
            class="button small-button more"
            data-action="straighten-settings"
            aria-expanded={shapesOpen}
            aria-controls="straighten-panel"
            aria-haspopup="dialog"
            disabled={locked}
            title={t('editor.straighten.settings.button')}
            aria-label={t('editor.straighten.settings.button')}
            onclick={() => onshapesopen?.(!shapesOpen)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
              <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
              <circle cx="16" cy="7" r="2" />
              <circle cx="10" cy="17" r="2" />
            </svg>
          </button>
          {#if shapesOpen && onshapes}
            <StraightenPanel
              id="straighten-panel"
              settings={shapes}
              selected={shapesSelected}
              found={shapesFound}
              onchange={onshapes}
              ondefaults={() => onshapesdefaults?.()}
              onapply={() => onstraighten?.()}
            />
          {/if}
        </div>
      </div>
      <p class="group label select-help">{t('editor.selectHelp')}</p>
    {/if}
  {/if}

  <div class="group share" role="group" aria-label={t('editor.share.label')}>
    <button
      type="button"
      class="button small-button"
      data-action="replay-toggle"
      aria-pressed={replaying}
      class:active={replaying}
      disabled={replayDisabled}
      title={t('editor.replay.hint')}
      onclick={onreplay}
    >
      <span aria-hidden="true">▶</span>
      {t('editor.replay.button')}
    </button>
    <button type="button" class="button small-button" aria-pressed={splitting} class:active={splitting} disabled={replaying} title={t('editor.split.hint')} onclick={onsplit}>
      <span aria-hidden="true">✂</span>
      {t('editor.split.button')}
    </button>
    <button type="button" class="button small-button" disabled={copying} title={t('editor.copyImage.hint')} onclick={oncopy}>
      <span aria-hidden="true">⧉</span>
      {t('editor.copyImage.button')}
    </button>
    <button type="button" class="button small-button" title={t('editor.export.hint')} onclick={onexport}>
      <span aria-hidden="true">⇪</span>
      {t('editor.export.button')}
    </button>
    <button type="button" class="button small-button" data-action="print" title={t('editor.print.hint')} onclick={onprint}>
      <span aria-hidden="true">⎙</span>
      {t('editor.print.button')}
    </button>
    <button type="button" class="button small-button" data-action="video-export" title={t('editor.video.hint')} onclick={onvideo}>
      <span aria-hidden="true">▣</span>
      {t('editor.video.button')}
    </button>
    {#if onresume}
      <button
        type="button"
        class="button small-button"
        data-action="resume-live"
        disabled={resumeDisabled}
        title={resumeHint ?? t('live.resume.hint')}
        onclick={onresume}
      >
        <span aria-hidden="true">◉</span>
        {t('live.resume.action')}
      </button>
    {/if}
  </div>
</div>

<style>
  .toolbar {
    /* Above the page: the setting's panel hangs under the toolbar. */
    position: relative;
    z-index: 3;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .group {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    /* As tall as a button, so that swapping buttons for a hint does not move the page. */
    min-height: 2.25rem;
  }

  .label,
  .slider > span:first-child {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .select-help {
    margin: 0;
    font-size: 0.8rem;
  }

  .tool {
    padding: 0.35rem 0.6rem;
  }

  .toolbar .tool.active,
  .toolbar .toggle.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .shape-icon {
    width: 1.05rem;
    height: 1.05rem;
    vertical-align: -0.2em;
  }

  .shape-options,
  .text-options {
    flex-wrap: wrap;
  }

  .segmented {
    display: inline-flex;
  }

  .segmented > .button:not(:first-child) {
    border-left: 0;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }

  .segmented > .button:not(:last-child) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }

  .bold {
    font-weight: 700;
  }

  .text-help {
    font-size: 0.8rem;
  }

  .tool-label {
    font-size: 0.875rem;
  }

  .swatch {
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
    border: 2px solid var(--border);
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch.active {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .swatch:focus-visible,
  .custom input:focus-visible,
  .slider input:focus-visible,
  .choice input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .custom input {
    width: 2rem;
    height: 1.7rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    cursor: pointer;
  }

  .slider input {
    width: 8rem;
    accent-color: var(--accent);
  }

  .value {
    min-width: 4rem;
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.875rem;
    cursor: pointer;
  }

  .small-button {
    padding: 0.25rem 0.6rem;
    font-size: 0.875rem;
  }

  .share {
    margin-left: auto;
  }

  .split {
    position: relative;
    display: inline-flex;
  }

  .split > .button:first-child {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }

  .split .more {
    padding: 0.25rem 0.45rem;
    border-left: 0;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }

  .split .more[aria-expanded='true'] {
    background: var(--surface-2);
    color: var(--accent);
  }

  .split .more svg {
    width: 1rem;
    height: 1rem;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  @media (max-width: 900px) {
    .tool-label {
      display: none;
    }
  }
</style>
