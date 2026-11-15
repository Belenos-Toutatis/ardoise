<script lang="ts">
  /**
   * The field where a text frame is typed (SPEC §4.4, `docs/design/page-objects.md` §4): a `<textarea>` laid exactly
   * over the frame, with the frame's fonts (the files shipped with the app, `registerTextFonts`, in the layout's order)
   * and the layout rules of `ardoise-text` (`textFieldStyle`: no kerning, no ligature), its text transparent. What shows
   * is the frame itself, drawn by the canvas from the glyphs of the exports as it is typed; the field gives the caret,
   * the selection, the keyboard and its accents, the input methods and the OS's on-screen keyboard. The browser's
   * baseline is put on the frame's (`shift`), from the font's ascent and descent as the browser measures them.
   *
   * The field never wraps by itself: its value is a display of the frame's text (`lib/text-field.ts`), with a line break
   * of its own where the frame wraps and a white square for a character no face has, so that its lines and its caret are
   * the frame's to the glyph. What the browser makes of the value is turned back into the frame's text (`applyFieldInput`);
   * the value is set again when the wrapping changes. The undo of the field is its own (`FieldHistory`: Ctrl + Z,
   * Ctrl + Y), and a copy or a cut carries the text without the soft breaks. A character no face has is named under the
   * frame. The side panel's « Caractères » tab inserts at the caret (`registerTextInput`); going to that tab (its keys
   * with the keyboard, its tab on the rail) does not end the typing.
   *
   * Typing never reaches the editor's shortcuts (the editor ignores keys typed in a field); Escape is the editor's
   * (`lib/editor/escape.ts`: the first press ends the editing, the text kept). Leaving the field ends the editing too.
   */
  import { onMount, untrack } from 'svelte';
  import { objectBounds, textStyle } from '../../lib/editor/objects';
  import { registerTextInput } from '../../lib/editor/text-input.svelte';
  import { pageToScreen, type View } from '../../lib/editor/view';
  import { t } from '../../lib/i18n.svelte';
  import { cssColor, type TextObject } from '../../lib/note';
  import { CSS_FAMILY, registerTextFonts, textFieldStyle, wrapOf, type TextEngine } from '../../lib/text';
  import {
    FieldHistory,
    applyFieldInput,
    codePointName,
    displayOffset,
    fieldDisplay,
    insertText,
    missingChars,
    normalizeBreaks,
    textRange,
    type FieldDisplay,
    type FieldEdit,
  } from '../../lib/text-field';

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
    /** Where the caret starts (UTF-16 offset in the frame's text); null: at the end. */
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
  const widest = $derived(laid.lines.reduce((w, line) => Math.max(w, line.width), 0));
  /**
   * A frame that wraps: the field is as wide as its wrap width (its dashed outline shows where the lines end), or its
   * widest line with the white space hanging at its end. One that does not: one em of room after the widest line. Never
   * narrower than a line of the value, so that the field never scrolls.
   */
  const width = $derived(wrapOf(style) > 0 ? Math.max(wrapOf(style), widest) * px + 2 : (laid.width + object.size) * px);
  const height = $derived(Math.max(laid.height, laid.lineHeight) * px);
  const turn = $derived((((object.quarterTurns % 4) + 4) % 4) * 90);
  /** The characters no face has, named under the frame. */
  const missing = $derived(missingChars(normalizeBreaks(object.text), engine.layout(normalizeBreaks(object.text), style)));
  const hintAt = $derived.by(() => {
    const box = objectBounds(object);
    return pageToScreen(view, { x: box.x, y: box.y + box.height });
  });

  // The text the field shows, its display, and the field's own undo (not reactive: the field's value is the truth),
  // from the frame and the caret the field opens with.
  const opened = untrack(() => {
    const text = normalizeBreaks(object.text);
    const at = caret === null ? text.length : normalizeBreaks(object.text.slice(0, Math.max(0, caret))).length;
    return { text, caret: at, display: fieldDisplay(text, engine.layout(text, textStyle(object))) };
  });
  let current = opened.text;
  let display: FieldDisplay = opened.display;
  const startCaret = opened.caret;
  const history = new FieldHistory(current, startCaret);
  /** An input method is composing (a dead key, an IME): the field's value waits for its end. */
  let composing = false;

  /** The layout of `text` in the frame's style now. */
  function layoutOf(text: string) {
    return engine.layout(text, textStyle(object));
  }

  /** The text offsets under the field's selection. */
  function selection(): [number, number] {
    if (!field) return [current.length, current.length];
    return textRange(display, field.selectionStart, field.selectionEnd);
  }

  /** Shows `text`, the selection at the text offsets `start`…`end`; the value is set only when it changes. */
  function show(text: string, start: number, end = start, force = false) {
    current = text;
    display = fieldDisplay(text, layoutOf(text));
    if (!field) return;
    if (field.value !== display.value) {
      field.value = display.value;
      force = true;
    }
    if (force) field.setSelectionRange(displayOffset(display, start), displayOffset(display, end));
    unscroll();
  }

  /** A change of the text made by the field (typing, a paste, a cut, the palette, undo): shown, and given to the editor. */
  function apply(edit: FieldEdit, record = true) {
    if (record) history.record(edit, performance.now());
    show(edit.text, edit.caret);
    oninput(edit.text);
  }

  function input(e: Event) {
    if (!field || composing || (e as InputEvent).isComposing) return;
    const edit = applyFieldInput(display, current, field.value, (e as InputEvent).inputType);
    if (!edit) {
      // Only a soft break went at an end of the text: it comes back.
      show(current, ...selection(), true);
      return;
    }
    apply(edit);
  }

  function compositionend() {
    composing = false;
    if (field && field.value !== display.value) input(new Event('input'));
  }

  function undo(redo: boolean) {
    const step = redo ? history.redo() : history.undo();
    if (!step) return;
    show(step.text, step.caret, step.caret, true);
    oninput(step.text);
  }

  function keydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    if (key === 'z' || key === 'y') {
      e.preventDefault();
      undo(key === 'y' || e.shiftKey);
    }
  }

  function beforeinput(e: InputEvent) {
    // The context menu's « Annuler » and « Rétablir » are the field's own undo too.
    if (e.inputType === 'historyUndo' || e.inputType === 'historyRedo') {
      e.preventDefault();
      undo(e.inputType === 'historyRedo');
    }
  }

  /** A copy carries the frame's text: no soft break, a missing character as it is. */
  function copy(e: ClipboardEvent, cut: boolean) {
    const [a, b] = selection();
    if (a === b || !e.clipboardData) return;
    e.preventDefault();
    e.clipboardData.setData('text/plain', current.slice(a, b));
    if (cut) apply({ ...insertText(current, a, b, ''), kind: 'delete' });
  }

  /** The palette's letter at the caret, in place of the selection. */
  function insert(text: string, refocus: boolean) {
    const [a, b] = selection();
    apply(insertText(current, a, b, text));
    if (refocus) field?.focus({ preventScroll: true });
  }

  // The frame's text changed from outside (the editor), or its style: the field follows, the selection kept.
  $effect(() => {
    const text = normalizeBreaks(object.text);
    void style.font;
    void style.bold;
    void style.size;
    void style.wrap;
    if (!field || composing) return;
    const [a, b] = text === current ? selection() : [text.length, text.length];
    const before = display.value;
    current = text;
    display = fieldDisplay(text, layoutOf(text));
    if (display.value !== before || field.value !== display.value) show(text, a, b, true);
  });

  $effect(() => {
    void style.font;
    void style.bold;
    void registerTextFonts(engine).then(measure);
  });

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

  onMount(() => {
    const el = field;
    if (!el) return;
    // Not bound to the frame's text: writing the value back would move the caret.
    el.value = display.value;
    el.focus({ preventScroll: true });
    const at = displayOffset(display, Math.min(current.length, startCaret));
    el.setSelectionRange(at, at);
    return registerTextInput({
      insert,
      focus: () => field?.focus({ preventScroll: true }),
      end: () => onblur(),
    });
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
  oninput={input}
  onbeforeinput={beforeinput}
  onkeydown={keydown}
  oncompositionstart={() => (composing = true)}
  oncompositionend={compositionend}
  oncopy={(e) => copy(e, false)}
  oncut={(e) => copy(e, true)}
  onscroll={unscroll}
  onblur={(e) => {
    // The window itself lost the focus (another window, Alt + Tab): the field gets it back when the window does.
    if (typeof document !== 'undefined' && typeof document.hasFocus === 'function' && !document.hasFocus()) return;
    const next = e.relatedTarget instanceof Element ? e.relatedTarget : null;
    // The focus went to the keys of the « Caractères » tab (Tab, the keyboard): the typing goes on there.
    if (next?.closest('[data-slot="greek-palette"]')) return;
    // The « Caractères » tab pressed on the rail: the typing goes on, the focus comes back here once the press is over.
    if (next?.closest('[role="tab"][data-tab="characters"]')) {
      setTimeout(() => field?.focus({ preventScroll: true }), 0);
      return;
    }
    onblur();
  }}
></textarea>
{#if missing.length > 0}
  <p class="missing-hint" data-slot="text-missing" role="status" style="left: {hintAt.x}px; top: {hintAt.y + 6}px;">
    {t(missing.length === 1 ? 'editor.text.missingOne' : 'editor.text.missingMany', {
      chars: missing.map((c) => `« ${c} » (${codePointName(c)})`).join(', '),
    })}
  </p>
{/if}

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

  .missing-hint {
    position: absolute;
    z-index: 3;
    max-width: min(28rem, 80vw);
    margin: 0;
    padding: 0.3rem 0.55rem;
    border: 1px solid var(--warn);
    border-radius: 6px;
    background: var(--warn-bg);
    color: var(--text);
    font-size: 0.8rem;
    line-height: 1.3;
    pointer-events: none;
    box-shadow: var(--shadow);
  }
</style>
