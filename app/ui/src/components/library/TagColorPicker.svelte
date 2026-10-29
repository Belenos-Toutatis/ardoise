<script lang="ts">
  /**
   * The colour of one tag (milestone M10): a small button showing it, which opens the palette
   * (each colour, or none). The colour belongs to the library (`_ardoise/tags.json`), so it
   * changes on every note carrying the tag at once. Escape or a click elsewhere closes it.
   */
  import { tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { TAG_COLORS, type TagColor } from '../../lib/tag-colors';
  import type { TagColorState } from '../../lib/tag-colors.svelte';

  let { tag, colors }: { tag: string; colors: TagColorState } = $props();

  let open = $state(false);
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);

  const current = $derived(colors.of(tag));
  const label = $derived(
    t('library.tagColor.button', { tag, color: current ? t(`library.tagColor.names.${current}`) : t('library.tagColor.none') }),
  );

  async function toggle() {
    open = !open;
    if (open) {
      await tick();
      root?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
    }
  }

  async function choose(color: TagColor | null) {
    open = false;
    trigger?.focus();
    if (color !== current) await colors.set(tag, color);
  }

  function onkeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      open = false;
      trigger?.focus();
    }
  }

  function outside(event: MouseEvent) {
    if (open && root && !root.contains(event.target as Node)) open = false;
  }
</script>

<svelte:window onclick={outside} />

<!-- The keydown handler only handles Escape bubbling up from the buttons inside. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="tag-color-picker" bind:this={root} {onkeydown}>
  <button
    bind:this={trigger}
    type="button"
    class="icon-button tag-color-trigger"
    class:has-color={current !== null}
    data-tag-color={current}
    aria-label={label}
    title={label}
    aria-haspopup="true"
    aria-expanded={open}
    onclick={toggle}
  >
    <span class="tag-dot" aria-hidden="true"></span>
  </button>
  {#if open}
    <span class="tag-color-palette" role="radiogroup" aria-label={t('library.tagColor.palette', { tag })}>
      {#each TAG_COLORS as color (color)}
        <button
          type="button"
          role="radio"
          class="swatch"
          data-tag-color={color}
          aria-checked={current === color}
          aria-label={t(`library.tagColor.names.${color}`)}
          title={t(`library.tagColor.names.${color}`)}
          onclick={() => choose(color)}
        >
          <span class="tag-dot" aria-hidden="true"></span>
        </button>
      {/each}
      <button
        type="button"
        role="radio"
        class="swatch none"
        aria-checked={current === null}
        aria-label={t('library.tagColor.none')}
        title={t('library.tagColor.none')}
        onclick={() => choose(null)}
      >
        <span aria-hidden="true">∅</span>
      </button>
    </span>
  {/if}
</span>

<style>
  .tag-color-picker {
    position: relative;
    display: inline-flex;
    flex: none;
  }

  .tag-color-trigger {
    width: 1.5rem;
    height: 1.5rem;
  }

  .tag-color-trigger:not(.has-color) .tag-dot {
    background: transparent;
  }

  .tag-color-palette {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 20;
    display: grid;
    grid-template-columns: repeat(5, 1.9rem);
    gap: 0.3rem;
    padding: 0.45rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .swatch {
    display: grid;
    place-items: center;
    width: 1.9rem;
    height: 1.9rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
  }

  .swatch .tag-dot {
    width: 1rem;
    height: 1rem;
  }

  .swatch[aria-checked='true'] {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
</style>
