<script lang="ts">
  /**
   * The field where a bookmark's label is typed (SPEC §4.4 « Signets », `docs/design/page-objects.md` §7): a one-line
   * text field just under the bookmark on the page (above it near the bottom of the editor), in the interface's own
   * font and size, whatever the zoom. The bookmark drawn on the page follows what is typed.
   *
   * The keys typed here are the field's: the editor's shortcuts ignore them. Enter ends the editing, the label kept
   * (`onenter`); Escape is the editor's (`lib/editor/escape.ts`: it ends the editing, the label kept, an empty label
   * removes the bookmark); leaving the field ends it too (`onblur`). A label is cut at `BOOKMARK.maxLabel` characters.
   */
  import { onMount } from 'svelte';
  import { BOOKMARK } from '../../lib/editor/objects';
  import type { Rect } from '../../lib/editor/geometry';
  import { pageToScreen, type View, type Viewport } from '../../lib/editor/view';
  import { t } from '../../lib/i18n.svelte';

  let {
    label,
    box,
    view,
    viewport,
    oninput,
    onenter,
    onblur,
  }: {
    /** What the bookmark says now. */
    label: string;
    /** The bookmark's box on the page. */
    box: Rect;
    view: View;
    viewport: Viewport;
    oninput: (label: string) => void;
    onenter: () => void;
    onblur: () => void;
  } = $props();

  /** The field's size on screen, pixels (the interface's rem is 16 px). */
  const WIDTH = 288;
  const HEIGHT = 36;
  const GAP = 8;

  let field = $state<HTMLInputElement>();

  const corner = $derived(pageToScreen(view, { x: box.x, y: box.y }));
  const bottom = $derived(pageToScreen(view, { x: box.x, y: box.y + box.height }).y);
  const left = $derived(Math.max(GAP, Math.min(corner.x, viewport.width - WIDTH - GAP)));
  // Under the bookmark; above it when the field would leave the editor.
  const top = $derived(bottom + GAP + HEIGHT > viewport.height ? Math.max(GAP, corner.y - GAP - HEIGHT) : bottom + GAP);

  onMount(() => {
    const el = field;
    if (!el) return;
    el.value = label;
    el.focus({ preventScroll: true });
    el.setSelectionRange(el.value.length, el.value.length);
  });
</script>

<input
  bind:this={field}
  class="bookmark-field"
  data-slot="bookmark-field"
  type="text"
  aria-label={t('editor.bookmark.field')}
  placeholder={t('editor.bookmark.placeholder')}
  maxlength={BOOKMARK.maxLabel}
  spellcheck="false"
  autocomplete="off"
  style="left: {left}px; top: {top}px; width: {WIDTH}px; height: {HEIGHT}px;"
  oninput={(e) => oninput(e.currentTarget.value)}
  onkeydown={(e) => {
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      onenter();
    }
  }}
  onblur={() => {
    // The window itself lost the focus (another window, Alt + Tab): the field gets it back when the window does.
    if (typeof document !== 'undefined' && typeof document.hasFocus === 'function' && !document.hasFocus()) return;
    onblur();
  }}
/>

<style>
  .bookmark-field {
    position: absolute;
    z-index: 3;
    box-sizing: border-box;
    padding: 0 0.6rem;
    border: 1.5px solid var(--accent);
    border-radius: 8px;
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
    font: inherit;
    font-size: 0.95rem;
  }

  .bookmark-field:focus {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
</style>
