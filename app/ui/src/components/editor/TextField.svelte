<script lang="ts">
  /**
   * The field where a text frame is typed (SPEC §4.4, `docs/design/page-objects.md` §5): a `<textarea>` laid exactly
   * over the frame, with the frame's font (the files shipped with the app, `registerTextFonts`) and the layout rules of
   * `ardoise-text` (`textFieldStyle`: no wrapping, no kerning, no ligature), its text transparent. What shows is the
   * frame itself, drawn by the canvas from the glyphs of the exports as it is typed; the field gives the caret, the
   * selection, the keyboard and its accents, the input methods and the OS's on-screen keyboard. The browser's baseline
   * is put on the frame's (`shift`), from the font's ascent and descent as the browser measures them.
   *
   * Typing never reaches the editor's shortcuts (the editor ignores keys typed in a field); Escape is the editor's
   * (`lib/editor/escape.ts`: the first press ends the editing, the text kept). Leaving the field ends the editing too.
   */
  import { onMount } from 'svelte';
  import { textStyle } from '../../lib/editor/objects';
  import { pageToScreen, type View } from '../../lib/editor/view';
  import { t } from '../../lib/i18n.svelte';
  import { cssColor, type TextObject } from '../../lib/note';
  import { CSS_FAMILY, registerTextFonts, textFieldStyle, type TextEngine } from '../../lib/text';

  let {
    object,
    view,
    engine,
    caret = null,
    oninput,
    onblur,
  }: {
    /** The frame being typed (its text follows what is typed). */
    object: TextObject;
    view: View;
    engine: TextEngine;
    /** Where the caret starts (UTF-16 offset); null: at the end. */
    caret?: number | null;
    oninput: (text: string) => void;
    onblur: () => void;
  } = $props();

  let field = $state<HTMLTextAreaElement>();
  /** The browser's ascent and descent of the frame's font, in units of its size (null until measured). */
  let metrics = $state.raw<{ ascent: number; descent: number } | null>(null);

  const style = $derived(textStyle(object));
  const laid = $derived(engine.layout(object.text, style));
  /** Screen pixels per unit of the frame's text. */
  const px = $derived(view.scale * object.scale);
  const origin = $derived(pageToScreen(view, object));
  /**
   * How far down the browser's lines must go for its baselines to be the frame's: both put the baseline half the
   * leading below the top of the line, then the ascent; only the ascents and descents differ.
   */
  const shift = $derived.by(() => {
    if (!metrics || !(object.size > 0)) return 0;
    const ours = (laid.ascent - laid.descent) / object.size;
    const browser = metrics.ascent - metrics.descent;
    return ((ours - browser) / 2) * object.size * px;
  });
  /** One em of room after the widest line, so that the field never scrolls nor wraps. */
  const width = $derived((laid.width + object.size) * px);
  const height = $derived(Math.max(laid.height, laid.lineHeight) * px);
  const turn = $derived((((object.quarterTurns % 4) + 4) % 4) * 90);

  /** The browser's metrics of the font, read from a canvas once the font files are loaded. */
  function measure() {
    // Without web fonts (tests), the field keeps the browser's line placement.
    if (typeof document === 'undefined' || !document.fonts) return;
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext?.('2d');
    if (!ctx) return;
    ctx.font = `${style.bold ? 700 : 400} 100px "${CSS_FAMILY[style.font]}"`;
    const m = ctx.measureText('Hg');
    if (Number.isFinite(m.fontBoundingBoxAscent) && Number.isFinite(m.fontBoundingBoxDescent)) {
      metrics = { ascent: m.fontBoundingBoxAscent / 100, descent: m.fontBoundingBoxDescent / 100 };
    }
  }

  $effect(() => {
    void style.font;
    void style.bold;
    void registerTextFonts(engine).then(measure);
  });

  onMount(() => {
    const el = field;
    if (!el) return;
    // Not bound to the frame's text: writing the value back would move the caret.
    el.value = object.text;
    el.focus({ preventScroll: true });
    const at = caret === null ? el.value.length : Math.max(0, Math.min(el.value.length, caret));
    el.setSelectionRange(at, at);
  });

  /** The field is as large as its text: it must never scroll its text away from the frame drawn under it. */
  function unscroll() {
    if (field && (field.scrollTop !== 0 || field.scrollLeft !== 0)) {
      field.scrollTop = 0;
      field.scrollLeft = 0;
    }
  }
</script>

<textarea
  bind:this={field}
  class="text-field"
  data-slot="text-field"
  aria-label={t('editor.text.field')}
  spellcheck="false"
  autocomplete="off"
  wrap="off"
  style="left: {origin.x}px; top: {origin.y}px; width: {width}px; height: {height}px; transform: rotate({turn}deg) translateY({shift}px); {textFieldStyle(
    style,
    px,
  )}; caret-color: {cssColor({ ...object.color, a: 255 })};"
  oninput={(e) => {
    oninput(e.currentTarget.value);
    unscroll();
  }}
  onscroll={unscroll}
  onblur={() => {
    // The window itself lost the focus (another window, Alt + Tab): the field gets it back when the window does.
    if (typeof document !== 'undefined' && typeof document.hasFocus === 'function' && !document.hasFocus()) return;
    onblur();
  }}
></textarea>

<style>
  .text-field {
    position: absolute;
    z-index: 2;
    box-sizing: content-box;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 0;
    overflow: hidden;
    resize: none;
    background: transparent;
    color: transparent;
    -webkit-text-fill-color: transparent;
    transform-origin: 0 0;
    outline: 1.5px dashed var(--accent);
    outline-offset: 3px;
  }

  .text-field::selection {
    background: rgb(47 111 223 / 30%);
  }
</style>
