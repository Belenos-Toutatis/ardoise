<script lang="ts">
  /**
   * The Greek palette of the text tool (the user's request: « un bouton pour afficher l'alphabet grec et cliquer sur une
   * lettre à insérer, minuscules et majuscules »): the button « αβγ » among the text options opens a panel under it with
   * the small letters (ς included), the capitals, the variants ϵ ϑ ϕ ϖ ϱ, then the symbols of physics and chemistry and
   * the superscript and subscript digits (`lib/editor/greek.ts`). A press on a letter inserts it at the caret of the frame
   * being typed, in place of its selection (`textInput.target`, `TextField.svelte`), as part of the same typing (one undo
   * step of the editor with it); the press takes the focus from nobody, so the caret stays in the frame. The panel stays
   * open for several letters and closes with its button, Escape (before the typing ends: `lib/editor/escape.ts`) or a
   * change of tool (`Editor.svelte`). With no frame being typed, the letters are disabled and the panel says to touch the
   * page first. The letters are drawn in the frame's family and weight. Keyboard: Tab goes into the panel (the typing goes
   * on), the arrows move from key to key, Enter or Space inserts.
   */
  import { cssFamilies } from '../../lib/text';
  import { PALETTE, paletteName, type PaletteChar } from '../../lib/editor/greek';
  import { textInput } from '../../lib/editor/text-input.svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { FontFamily } from '../../lib/note';

  let {
    font,
    bold,
    disabled = false,
  }: {
    /** The family and weight the letters are drawn in: the frame's (the tool shows them while a frame is typed). */
    font: FontFamily;
    bold: boolean;
    disabled?: boolean;
  } = $props();

  let panel = $state<HTMLDivElement>();
  const active = $derived(textInput.target !== null);

  // The palette belongs to the text tool's options: when they go (another tool, another note), it closes.
  $effect(() => () => {
    textInput.paletteOpen = false;
  });

  /** A press on the button or a letter keeps the focus where it is: in the frame being typed. */
  function keepFocus(e: PointerEvent) {
    if (e.button === 0) e.preventDefault();
  }

  function toggle() {
    textInput.paletteOpen = !textInput.paletteOpen;
  }

  /** A letter pressed with the pointer: the caret stays in the frame; from the keyboard, the focus stays on the key. */
  function insert(c: PaletteChar, e: MouseEvent) {
    const target = textInput.target;
    if (!target) return;
    target.insert(c.char, e.detail > 0);
  }

  /** The arrows move from key to key: left and right in reading order, up and down to the nearest key of the row there. */
  function navigate(e: KeyboardEvent) {
    if (!panel || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
    const keys = Array.from(panel.querySelectorAll<HTMLButtonElement>('button.key'));
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
      // Without a layout (tests), a row is taken as eight keys.
      if (rows.every((r) => r.top === 0 && r.left === 0)) to = Math.max(0, Math.min(keys.length - 1, from + (down ? 8 : -8)));
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

  /** The focus leaves the panel for something else than the frame's field: the typing ends there, as if the field were left. */
  function focusout(e: FocusEvent) {
    const next = e.relatedTarget;
    if (next instanceof Element && (panel?.contains(next) || next.closest('[data-slot="text-field"]'))) return;
    textInput.target?.end();
  }
</script>

<div class="greek">
  <button
    type="button"
    class="button small-button toggle"
    class:active={textInput.paletteOpen}
    aria-pressed={textInput.paletteOpen}
    aria-expanded={textInput.paletteOpen}
    aria-controls="greek-palette"
    aria-label={t('editor.text.greek.button')}
    title={t('editor.text.greek.buttonHint')}
    data-action="text-greek"
    {disabled}
    style:font-family={cssFamilies(font)}
    onpointerdown={keepFocus}
    onclick={toggle}>αβγ</button
  >
  {#if textInput.paletteOpen && !disabled}
    <div
      bind:this={panel}
      id="greek-palette"
      class="palette"
      role="toolbar"
      tabindex="-1"
      aria-label={t('editor.text.greek.label')}
      data-slot="greek-palette"
      onkeydown={navigate}
      onfocusout={focusout}
    >
      {#if !active}
        <p class="hint" data-slot="greek-hint">{t('editor.text.greek.inactive')}</p>
      {/if}
      {#each PALETTE as group (group.id)}
        <div class="row" role="group" aria-label={t(`editor.text.greek.groups.${group.id}`)} data-group={group.id}>
          <span class="row-label" aria-hidden="true">{t(`editor.text.greek.groups.${group.id}`)}</span>
          <div class="keys">
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
                onclick={(e) => insert(c, e)}>{c.char}</button
              >
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .greek {
    position: relative;
    display: inline-flex;
  }

  .greek > .button {
    min-width: 2.75rem;
  }

  /* Over the editor's own `.button.active`. */
  .greek > .toggle.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .palette {
    position: absolute;
    top: calc(100% + 0.35rem);
    left: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    box-sizing: border-box;
    /* About thirteen keys a row: the small letters and the capitals in two rows each. */
    width: min(36rem, calc(100vw - 2rem));
    max-height: min(36rem, calc(100vh - 9rem));
    overflow-y: auto;
    padding: 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius, 8px);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .hint {
    margin: 0;
    padding: 0.35rem 0.5rem;
    border-radius: 6px;
    background: var(--warn-bg);
    color: var(--text);
    font-size: 0.85rem;
  }

  .row {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .row-label {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, 2.4rem);
    gap: 0.25rem;
  }

  .key {
    width: 2.4rem;
    height: 2.4rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font-size: 1.25rem;
    line-height: 1;
    cursor: pointer;
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

  @media (max-width: 600px) {
    .palette {
      position: fixed;
      top: auto;
      bottom: 0.5rem;
      left: 0.5rem;
      right: 0.5rem;
      width: auto;
      max-width: none;
    }
  }
</style>
