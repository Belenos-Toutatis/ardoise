<script lang="ts">
  /**
   * The computer's ink in the live window's toolbar (docs/design/tablet-live-classroom.md, "M14", "The computer's
   * ink" and "Live window"): a button showing its colour and tool, which opens a small panel with the tool, the
   * colour swatches (here they only set the computer's colour, never the tablet's, never the selection's) and the
   * width. Esc, or a click outside, closes the panel. The style itself is the window's (`lib/live-computer-style.ts`,
   * remembered on this PC): every change goes up through `onchange`.
   */
  import { sameColor } from '../../lib/editor/colors';
  import { t } from '../../lib/i18n.svelte';
  import { TOOLS, type Tool } from '../../lib/ink';
  import type { LiveStyle } from '../../lib/live';
  import { MAX_LIVE_WIDTH, MIN_LIVE_WIDTH } from '../../lib/live-computer-style';
  import { cssColor, type Rgba } from '../../lib/note';

  let {
    style,
    colors,
    disabled = false,
    open = $bindable(false),
    onchange,
  }: {
    style: LiveStyle;
    /** The swatches offered, with the translation key of each name. */
    colors: readonly { color: Rgba; label: string }[];
    disabled?: boolean;
    /** The panel shows. */
    open?: boolean;
    onchange: (style: LiveStyle) => void;
  } = $props();

  let box = $state<HTMLElement>();
  const panelId = 'computer-style-panel';

  const mm = (units: number) => (units / 100).toFixed(units < 100 ? 2 : 1);

  // A closed tool (the session is over) has no panel.
  $effect(() => {
    if (disabled && open) open = false;
  });

  // A click outside closes the panel.
  $effect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => {
      if (box && e.target instanceof Node && !box.contains(e.target)) open = false;
    };
    window.addEventListener('pointerdown', outside, true);
    return () => window.removeEventListener('pointerdown', outside, true);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !open) return;
    // The window's Esc (close the projection, leave live mode) waits for the panel.
    e.preventDefault();
    e.stopPropagation();
    open = false;
    box?.querySelector<HTMLElement>('[data-action="computer-style"]')?.focus();
  }
</script>

<!-- Esc inside the panel closes it, before the window's own Esc. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="computer-style" bind:this={box} {onkeydown}>
  <button
    type="button"
    class="button small-button style-button"
    data-action="computer-style"
    aria-label={t('live.pen.styleHint')}
    title={t('live.pen.styleHint')}
    aria-expanded={open}
    aria-controls={panelId}
    {disabled}
    onclick={() => (open = !open)}
  >
    <span class="dot" style:background={cssColor(style.color)} aria-hidden="true"></span>
    <span class="tool-name">{t(`editor.ink.tools.${style.tool}`)}</span>
  </button>
  {#if open}
    <div class="style-panel" id={panelId} role="dialog" aria-label={t('live.pen.style')} data-slot="computer-style">
      <strong class="title">{t('live.pen.style')}</strong>
      <label class="field">
        <span class="label">{t('live.tool')}</span>
        <select value={style.tool} onchange={(e) => onchange({ ...style, tool: e.currentTarget.value as Tool })}>
          {#each TOOLS as tool (tool)}
            <option value={tool}>{t(`editor.ink.tools.${tool}`)}</option>
          {/each}
        </select>
      </label>
      <div class="swatches" role="group" aria-label={t('live.color')}>
        {#each colors as { color, label } (label)}
          <button
            type="button"
            class="swatch"
            class:active={sameColor(color, style.color)}
            style:background={cssColor(color)}
            aria-pressed={sameColor(color, style.color)}
            aria-label={t(label)}
            title={t(label)}
            onclick={() => onchange({ ...style, color: { ...color } })}
          ></button>
        {/each}
      </div>
      <label class="field">
        <span class="label">{t('live.width')}</span>
        <span class="slider">
          <input
            type="range"
            min={MIN_LIVE_WIDTH}
            max={MAX_LIVE_WIDTH}
            step="5"
            value={style.width}
            oninput={(e) => onchange({ ...style, width: Number(e.currentTarget.value) })}
          />
          <span class="value">{t('editor.width.value', { mm: mm(style.width) })}</span>
        </span>
      </label>
    </div>
  {/if}
</span>

<style>
  .computer-style {
    position: relative;
    display: inline-flex;
  }

  .style-button {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  .style-button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .dot {
    width: 0.9rem;
    height: 0.9rem;
    border: 1px solid var(--border);
    border-radius: 50%;
  }

  .style-panel {
    position: absolute;
    top: calc(100% + 0.35rem);
    left: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    width: max-content;
    max-width: min(22rem, 90vw);
    padding: 0.75rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .title {
    font-size: 0.9rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .label {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
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
  .slider input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .slider input {
    width: 10rem;
  }

  .value {
    min-width: 4.5rem;
    color: var(--muted);
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }
</style>
