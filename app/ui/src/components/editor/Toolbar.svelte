<script lang="ts">
  /** Tools and style: mode, ink tool, colour (Inkspace palette + custom colours), width, eraser. */
  import { INKSPACE_COLOR_NAMES, sameColor, toHex, fromHex } from '../../lib/editor/colors';
  import type { EraserMode, Mode, ToolSettings } from '../../lib/editor/state';
  import { t } from '../../lib/i18n.svelte';
  import { TOOLS, type Tool } from '../../lib/ink';
  import { INKSPACE_COLORS, cssColor, type Rgba } from '../../lib/note';
  import ShareButton from '../share/ShareButton.svelte';

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
  } = $props();

  const MODES: { mode: Mode; icon: string }[] = [
    { mode: 'select', icon: '⬚' },
    { mode: 'pen', icon: '✎' },
    { mode: 'eraser', icon: '⌫' },
    { mode: 'pan', icon: '✋' },
  ];

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
        <span aria-hidden="true">{icon}</span>
        <span class="tool-label">{t(`editor.modes.${mode}`)}</span>
      </button>
    {/each}
  </div>

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
    <label class="group">
      <span class="label">{t('editor.ink.label')}</span>
      <select value={tools.tool} disabled={locked} onchange={(e) => ontool(e.currentTarget.value as Tool)}>
        {#each TOOLS as tool}
          <option value={tool}>{t(`editor.ink.tools.${tool}`)}</option>
        {/each}
      </select>
    </label>

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
          onclick={() => oncolor(color)}
        ></button>
      {/each}
      <label class="custom" title={t('editor.color.custom')}>
        <span class="visually-hidden">{t('editor.color.custom')}</span>
        <input type="color" value={toHex(tools.color)} disabled={locked} onchange={pickCustom} />
      </label>
    </div>

    <label class="group slider">
      <span>{t('editor.width.label')}</span>
      <input
        type="range"
        min="10"
        max="1000"
        step="5"
        value={tools.width}
        disabled={locked}
        oninput={(e) => onwidth(Number(e.currentTarget.value), false)}
        onchange={(e) => onwidth(Number(e.currentTarget.value), true)}
      />
      <span class="value">{t('editor.width.value', { mm: mm(tools.width) })}</span>
    </label>

    <div class="group apply" role="group" aria-label={t('editor.applyAll.label')}>
      <span class="label">{hasSelection ? t('editor.applyAll.selectionHint') : t('editor.applyAll.label')}</span>
      {#if !hasSelection}
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('color')}>{t('editor.applyAll.color')}</button>
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('tool')}>{t('editor.applyAll.tool')}</button>
        <button type="button" class="button small-button" disabled={locked} onclick={() => onapplyall('width')}>{t('editor.applyAll.width')}</button>
      {/if}
    </div>
    {#if tools.mode === 'select'}
      <div class="group">
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
    <ShareButton />
  </div>
</div>

<style>
  .toolbar {
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

  .toolbar .tool.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
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
