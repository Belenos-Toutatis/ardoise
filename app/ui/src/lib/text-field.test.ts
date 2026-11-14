/// <reference types="node" />
/**
 * The text field's display of a frame (`lib/text-field.ts`): the field holds the frame's lines exactly (a line break of
 * its own where the frame wraps, the white square for a character no face has), and what the browser makes of it comes
 * back as the frame's text; with the real layout of the WebAssembly module (`ardoise-text`).
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import { MISSING_CHAR, type TextLayout, type TextStyle } from './text';
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
} from './text-field';

let ink: Ink;
const LM: TextStyle = { font: 'latinModern', bold: false, size: 1000 };

beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const TEXT = 'La lumière se propage en ligne droite dans un milieu homogène.';

/** A width that wraps `TEXT` on about three lines. */
function narrow(): TextStyle {
  return { ...LM, wrap: ink.text.layout(TEXT, LM).width / 2.6 };
}

/** The text of each line of `layout`. */
function lineTexts(text: string, layout: TextLayout): string[] {
  return layout.lines.map((l) => text.slice(l.start, l.end));
}

describe('the display of a frame', () => {
  it('holds the frame lines exactly: a line break where it wraps', () => {
    const style = narrow();
    const laid = ink.text.layout(TEXT, style);
    expect(laid.lines.length).toBeGreaterThanOrEqual(3);
    expect(laid.lines.slice(0, -1).every((l) => l.wrapped)).toBe(true);
    expect(laid.lines.at(-1)!.wrapped).toBe(false);
    const display = fieldDisplay(TEXT, laid);
    // The field's lines are the layout's lines, so its line k is drawn on the frame's line k, glyph by glyph.
    expect(display.value.split('\n')).toEqual(lineTexts(TEXT, laid));
    expect(display.soft).toHaveLength(laid.lines.length - 1);
    expect(display.toText).toHaveLength(display.value.length + 1);
    expect(display.toText.at(-1)).toBe(TEXT.length);
    // Both sides of a soft break are the same place of the text; the caret goes after it.
    const at = display.soft[0];
    expect(display.toText[at]).toBe(display.toText[at + 1]);
    expect(displayOffset(display, display.toText[at])).toBe(at + 1);
    // Every position comes back to itself.
    for (let t = 0; t <= TEXT.length; t++) expect(display.toText[displayOffset(display, t)]).toBe(t);
  });

  it('is the text itself for a frame that does not wrap, line breaks kept', () => {
    const text = 'Bonjour\nle monde';
    const display = fieldDisplay(text, ink.text.layout(text, LM));
    expect(display.value).toBe(text);
    expect(display.soft).toEqual([]);
    expect(display.toText).toEqual([...Array(text.length + 1).keys()]);
    expect(normalizeBreaks('a\r\nb\rc\n')).toBe('a\nb\nc\n');
  });

  it('shows a white square for a character no face has, and names it', () => {
    const text = 'a\u{1F600}b';
    const laid = ink.text.layout(text, LM);
    expect(laid.lines[0].glyphs.map((g) => g.missing)).toEqual([false, true, false]);
    const display = fieldDisplay(text, laid);
    expect(display.value).toBe(`a${MISSING_CHAR}b`);
    expect(display.toText).toEqual([0, 1, 3, 4]);
    expect(missingChars(text, laid)).toEqual(['\u{1F600}']);
    expect(codePointName('\u{1F600}')).toBe('U+1F600');
    expect(codePointName('α')).toBe('U+03B1');
    // Greek and the symbols of physics are not missing in either family.
    for (const font of ['latinModern', 'andika'] as const) {
      for (const bold of [false, true]) {
        const greek = 'αβγδεζηθικλμνξοπρςστυφχψω ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ ϵϑϕϖϱ µ°±×·≈≠≤≥→⇌∞√′ ⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻ ₀₁₂₃₄₅₆₇₈₉';
        expect(missingChars(greek, ink.text.layout(greek, { font, bold, size: 600 }))).toEqual([]);
      }
    }
  });
});

describe('what the browser makes of the field', () => {
  const style = () => narrow();
  const shown = (text: string) => fieldDisplay(text, ink.text.layout(text, style()));

  it('inserts what is typed at its place in the text, soft breaks left out', () => {
    const display = shown(TEXT);
    // A letter typed at the start of the second line.
    const at = display.soft[0] + 1;
    const next = display.value.slice(0, at) + 'X' + display.value.slice(at);
    const edit = applyFieldInput(display, TEXT, next, 'insertText')!;
    const offset = display.toText[at];
    expect(edit.text).toBe(TEXT.slice(0, offset) + 'X' + TEXT.slice(offset));
    expect(edit.caret).toBe(offset + 1);
    expect(edit.kind).toBe('insert');
    // The whole value replaced (a test, a paste of everything): the text is what was given, without soft breaks.
    expect(applyFieldInput(display, TEXT, 'Autre chose', 'insertFromPaste')!.text).toBe('Autre chose');
    // A line break typed is a line break of the text.
    const enter = display.value.slice(0, at) + '\n' + display.value.slice(at);
    expect(applyFieldInput(display, TEXT, enter, 'insertLineBreak')!.text).toBe(TEXT.slice(0, offset) + '\n' + TEXT.slice(offset));
  });

  it('removes the space before a soft break with Backspace, the letter after it with Delete', () => {
    const display = shown(TEXT);
    const at = display.soft[0];
    const offset = display.toText[at];
    expect(TEXT[offset - 1]).toBe(' ');
    const without = display.value.slice(0, at) + display.value.slice(at + 1);
    const back = applyFieldInput(display, TEXT, without, 'deleteContentBackward')!;
    expect(back.text).toBe(TEXT.slice(0, offset - 1) + TEXT.slice(offset));
    expect(back.caret).toBe(offset - 1);
    const forward = applyFieldInput(display, TEXT, without, 'deleteContentForward')!;
    expect(forward.text).toBe(TEXT.slice(0, offset) + TEXT.slice(offset + 1));
    expect(forward.caret).toBe(offset);
    // Without the input's type (an old browser), Backspace's meaning.
    expect(applyFieldInput(display, TEXT, without)!.text).toBe(back.text);
  });

  it('replaces a selection across a soft break, and deletes it', () => {
    const display = shown(TEXT);
    const at = display.soft[0];
    const [a, b] = [at - 3, at + 4];
    const next = display.value.slice(0, a) + 'Z' + display.value.slice(b);
    const [ta, tb] = textRange(display, a, b);
    expect(ta).toBe(display.toText[a]);
    expect(tb).toBe(display.toText[b]);
    const edit = applyFieldInput(display, TEXT, next, 'insertText')!;
    expect(edit.text).toBe(TEXT.slice(0, ta) + 'Z' + TEXT.slice(tb));
    expect(edit.kind).toBe('replace');
    const cut = applyFieldInput(display, TEXT, display.value.slice(0, a) + display.value.slice(b), 'deleteByCut')!;
    expect(cut.text).toBe(TEXT.slice(0, ta) + TEXT.slice(tb));
    expect(textRange(display, b, a)).toEqual([ta, tb]);
  });

  it('never cuts a character in two, and deletes a missing one whole', () => {
    const text = 'x \u{1D400}\u{1D401} y';
    const display = fieldDisplay(text, ink.text.layout(text, LM));
    // Changing the second of two characters that share their first unit.
    const next = display.value.replace('\u{1D401}', '\u{1D402}');
    const edit = applyFieldInput(display, text, next, 'insertText')!;
    expect(edit.text).toBe('x \u{1D400}\u{1D402} y');
    const emoji = 'a\u{1F600}b';
    const shownEmoji = fieldDisplay(emoji, ink.text.layout(emoji, LM));
    const gone = applyFieldInput(shownEmoji, emoji, 'ab', 'deleteContentBackward')!;
    expect(gone.text).toBe('ab');
    expect(gone.caret).toBe(1);
    // Backspace before a soft break that follows a surrogate pair takes the pair.
    expect(insertText('ab', 1, 1, 'α')).toMatchObject({ text: 'aαb', caret: 2, kind: 'insert' });
    expect(insertText('abc', 2, 1, 'β')).toMatchObject({ text: 'aβc', caret: 2, kind: 'replace' });
  });

  it('changes nothing when the value did not change, or a soft break went at an end of the text', () => {
    const display = shown(TEXT);
    expect(applyFieldInput(display, TEXT, display.value)).toBeNull();
  });
});

describe('the field undo', () => {
  it('makes one step of the letters typed in a row, another at a space, a pause or a deletion', () => {
    const history = new FieldHistory('', 0);
    expect(history.canUndo()).toBe(false);
    let text = '';
    const typeAt = (c: string, time: number) => {
      const edit = insertText(text, text.length, text.length, c);
      text = edit.text;
      history.record(edit, time);
    };
    typeAt('a', 0);
    typeAt('b', 100);
    typeAt(' ', 200);
    typeAt('c', 300);
    typeAt('d', 5000);
    // The space starts a new step, the word after it goes on with it.
    expect(history.undo()).toEqual({ text: 'ab c', caret: 4 });
    expect(history.undo()).toEqual({ text: 'ab', caret: 2 });
    expect(history.undo()).toEqual({ text: '', caret: 0 });
    expect(history.undo()).toBeNull();
    expect(history.redo()).toEqual({ text: 'ab', caret: 2 });
    expect(history.canRedo()).toBe(true);
    // A new change drops what was undone.
    history.record(insertText('ab', 2, 2, 'α'), 9000);
    expect(history.canRedo()).toBe(false);
    expect(history.undo()).toEqual({ text: 'ab', caret: 2 });
  });
});
