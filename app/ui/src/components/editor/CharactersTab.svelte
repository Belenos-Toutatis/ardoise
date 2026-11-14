<script lang="ts">
  /**
   * The editor's « Caractères » tab of the side panel (the user's choice of 2026-11-15, after trying the pop-up palette
   * that covered the frame being typed): the Greek letters and symbols to insert in a text frame, in the pane, never over
   * the page. The small letters (ς included), the capitals, the variants ϵ ϑ ϕ ϖ ϱ, the symbols of physics and chemistry
   * and the superscript and subscript digits (`lib/editor/greek.ts`), each group titled, in keys sized for a pen or a
   * finger that wrap to the pane's width (no horizontal scroll), drawn in the font and weight of the frame being typed
   * (the tool's otherwise).
   *
   * A press on a key inserts its character at the caret of the frame being typed, in place of its selection
   * (`textInput.target`, `TextField.svelte`), as part of the same typing (one undo step of the editor with it); the press
   * takes the focus from nobody, so the caret stays in the frame. Keyboard: Tab goes into the tab (the typing goes on),
   * the arrows move from key to key, Enter or Space inserts and gives the focus back to the frame. Leaving the tab for
   * anything else than the frame ends the typing, as leaving the frame does. With no frame being typed the keys are
   * disabled and the tab says to touch the page with the Text tool, with a button that takes that tool when another is
   * chosen. The « αβγ » button of the Text tool opens this tab (`Editor.svelte`, `toggleCharacters`).
   */
  import { cssFamilies } from '../../lib/text';
  import { PALETTE, paletteName, type PaletteChar } from '../../lib/editor/greek';
  import { textInput } from '../../lib/editor/text-input.svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { FontFamily } from '../../lib/note';

  let {
    font,
    bold,
    textTool,
    ontexttool,
    disabled = false,
  }: {
    /** The family and weight the keys are drawn in: the frame's while one is typed (the tool shows it), else the tool's. */
    font: FontFamily;
    bold: boolean;
    /** The Text tool is the one chosen (no « Outil Texte » button then). */
    textTool: boolean;
    /** « Outil Texte »: choose the Text tool. */
    ontexttool: () => void;
    /** Changes wait (the split panel is open). */
    disabled?: boolean;
  } = $props();

  let root = $state<HTMLDivElement>();
  const active = $derived(textInput.target !== null && !disabled);

  /** A press on a key keeps the focus where it is: in the frame being typed. */
  function keepFocus(e: PointerEvent) {
    if (e.button === 0) e.preventDefault();
  }

  /** A key pressed: its character at the frame's caret, the focus in the frame (a pointer never took it from there). */
  function insert(c: PaletteChar) {
    textInput.target?.insert(c.char, true);
  }

  /** The arrows move from key to key: left and right in reading order, up and down to the nearest key of the row there. */
  function navigate(e: KeyboardEvent) {
    if (!root || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
    const keys = Array.from(root.querySelectorAll<HTMLButtonElement>('button.key'));
    const from = keys.indexOf(document.activeElement as HTMLButtonElement);
    if (from < 0) return;
    e.preventDefault();
    let to = from;
    if (e.key === 'ArrowRight') to = Math.min(keys.length - 1, from + 1);
    else if (e.key === 'ArrowLeft') to = Math.max(0, from - 1);
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = keys.length - 1;
    else {
      const down = e.key === 'ArrowDown';
      const here = keys[from].getBoundingClientRect();
      const rows = keys.map((k) => k.getBoundingClientRect());
      // Without a layout (tests), a row is taken as five keys.
      if (rows.every((r) => r.top === 0 && r.left === 0)) to = Math.max(0, Math.min(keys.length - 1, from + (down ? 5 : -5)));
      else {
        const tops = rows.map((r) => r.top).filter((top) => (down ? top > here.top + 1 : top < here.top - 1));
        if (tops.length > 0) {
          const row = down ? Math.min(...tops) : Math.max(...tops);
          const centre = here.left + here.width / 2;
          let best = Infinity;
          rows.forEach((r, k) => {
            if (Math.abs(r.top - row) > 1) return;
            const distance = Math.abs(r.left + r.width / 2 - centre);
            if (distance < best) {
              best = distance;
              to = k;
            }
          });
        }
      }
    }
    keys[to].focus();
  }

  /** The focus leaves the tab for something else than the frame's field: the typing ends there, as if the field were left. */
  function focusout(e: FocusEvent) {
    const next = e.relatedTarget;
    if (next instanceof Element && (root?.contains(next) || next.closest('[data-slot="text-field"]'))) return;
    textInput.target?.end();
  }
</script>

<!-- The arrows move between the keys of the whole tab (a composite widget): the handler is on their common parent. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div bind:this={root} class="characters" data-slot="greek-palette" onkeydown={navigate} onfocusout={focusout}>
  {#if !active}
    <section class="pane-block" data-slot="greek-inactive">
      <p class="pane-help" data-slot="greek-hint">{t('editor.text.greek.inactive')}</p>
      {#if !textTool && !disabled}
        <div class="pane-actions">
          <button type="button" class="button" data-action="characters-text-tool" onclick={ontexttool}>
            {t('editor.text.greek.textTool')}
          </button>
        </div>
      {/if}
    </section>
  {/if}
  {#each PALETTE as group (group.id)}
    <section class="pane-block" data-group={group.id} aria-labelledby="characters-{group.id}">
      <h3 class="pane-title" id="characters-{group.id}">{t(`editor.text.greek.groups.${group.id}`)}</h3>
      <div class="keys" role="group" aria-labelledby="characters-{group.id}">
        {#each group.chars as c (c.char + c.form)}
          {@const name = paletteName(c)}
          <button
            type="button"
            class="key"
            aria-label={name}
            title={name}
            data-char={c.char}
            disabled={!active}
            style:font-family={cssFamilies(font)}
            style:font-weight={bold ? 700 : 400}
            onpointerdown={keepFocus}
            onclick={() => insert(c)}>{c.char}</button
          >
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  /* The keys fill the pane's width: six a row in the 290 px pane, four at 240 px (a window up to 900 px wide, the scroll bar taking its room); never a horizontal
     scroll, the pane's body scrolls down as the other tabs do. */
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(2.4rem, 1fr));
    gap: 0.3rem;
  }

  .key {
    height: 2.6rem;
    min-width: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font-size: 1.3rem;
    line-height: 1;
    cursor: pointer;
    touch-action: manipulation;
  }

  .key:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .key:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .key:disabled {
    cursor: default;
    opacity: 0.45;
  }
</style>
