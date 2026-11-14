/**
 * What the editor's text field holds while a frame is typed (`TextField.svelte`, `docs/design/page-objects.md` §4): the
 * field is a `<textarea>` laid over the frame, its own text transparent, that gives the caret, the selection, the keyboard
 * and the input methods. Its lines must be the frame's to the glyph, so it never wraps by itself (`white-space: pre`):
 * where the frame wraps (`TextLine.wrapped`), its value holds a line break of its own (a « soft » break), and a character no
 * face has is shown as the white square the frame draws (`MISSING_CHAR`), whose advance the browser then gives as the
 * layout does. So the field's value is a **display** of the frame's text, mapped to it position by position:
 *
 * - `fieldDisplay` builds it from the text and its layout;
 * - `applyFieldInput` turns what the browser made of the field's value (a key, a paste, an input method) into the frame's
 *   new text and caret: the change is found by comparing the values before and after (their common start and end), and
 *   its place in the text read through the map; removing only a soft break (Backspace at the start of a wrapped line,
 *   Delete at the end of one) removes the character of the text before (or after) it, as a word processor does;
 * - `textRange` gives the text under a selection of the field (to copy without the soft breaks);
 * - `FieldHistory` is the field's own undo (the browser's is lost each time the value is set again).
 *
 * Pure (no DOM), so that each rule is tested alone.
 */
import { MISSING_CHAR, type TextLayout } from './text';

/** The text as a field holds it: every `\r\n` or lone `\r` a `\n` (what a `<textarea>` does to its value). */
export function normalizeBreaks(text: string): string {
  return text.replace(/\r\n?/g, '\n');
}

/** The field's value for a text, and where each of its positions is in the text. */
export interface FieldDisplay {
  value: string;
  /** For each position 0…`value.length` of the field (between two UTF-16 units), the UTF-16 offset in the text. */
  toText: number[];
  /** The positions of the field's soft breaks (line breaks of its own, where the frame wraps). */
  soft: number[];
}

const isHigh = (code: number) => code >= 0xd800 && code <= 0xdbff;
const isLow = (code: number) => code >= 0xdc00 && code <= 0xdfff;

/**
 * The field's value for `text` (with `\n` line breaks only, `normalizeBreaks`) laid out as `layout`: a soft break where a
 * line wraps, the white square for a character no face has.
 */
export function fieldDisplay(text: string, layout: TextLayout): FieldDisplay {
  const breaks = new Set<number>();
  const missing = new Set<number>();
  for (const line of layout.lines) {
    if (line.wrapped && line.end > 0 && line.end < text.length) breaks.add(line.end);
    for (const glyph of line.glyphs) if (glyph.missing) missing.add(glyph.index);
  }
  let value = '';
  const toText: number[] = [];
  const soft: number[] = [];
  let at = 0;
  while (at < text.length) {
    if (breaks.has(at)) {
      soft.push(value.length);
      toText.push(at);
      value += '\n';
    }
    const pair = isHigh(text.charCodeAt(at)) && at + 1 < text.length && isLow(text.charCodeAt(at + 1));
    const units = pair ? 2 : 1;
    if (missing.has(at)) {
      toText.push(at);
      value += MISSING_CHAR;
    } else {
      for (let k = 0; k < units; k++) toText.push(at + k);
      value += text.slice(at, at + units);
    }
    at += units;
  }
  toText.push(text.length);
  return { value, toText, soft };
}

/**
 * The field's position for the text offset `offset`: after a soft break rather than before it (the caret at the start of
 * the next line, where typing goes on), never inside a character shown as one unit.
 */
export function displayOffset(display: FieldDisplay, offset: number): number {
  let best = 0;
  for (let d = 0; d < display.toText.length; d++) {
    if (display.toText[d] <= offset) best = d;
    else break;
  }
  return best;
}

/** The text offsets under the field's selection `start`…`end`. */
export function textRange(display: FieldDisplay, start: number, end: number): [number, number] {
  const clamp = (d: number) => display.toText[Math.max(0, Math.min(display.toText.length - 1, d))];
  return [clamp(Math.min(start, end)), clamp(Math.max(start, end))];
}

/** The length of the character of `text` before `at` (2 for a surrogate pair). */
function before(text: string, at: number): number {
  return at >= 2 && isLow(text.charCodeAt(at - 1)) && isHigh(text.charCodeAt(at - 2)) ? 2 : 1;
}

/** The length of the character of `text` at `at`. */
function after(text: string, at: number): number {
  return at + 1 < text.length && isHigh(text.charCodeAt(at)) && isLow(text.charCodeAt(at + 1)) ? 2 : 1;
}

/** A change of the frame's text: the new text, where the caret goes, and what kind of change it was (for the undo). */
export interface FieldEdit {
  text: string;
  caret: number;
  kind: 'insert' | 'delete' | 'replace';
  /** What was typed or pasted. */
  inserted: string;
}

/**
 * The frame's text after the field's value went from `display.value` to `next` (`inputType`: the browser's
 * `InputEvent.inputType`, to tell Backspace from Delete when only a soft break went); null when the text does not change
 * (only a soft break went, at an end of the text).
 */
export function applyFieldInput(display: FieldDisplay, text: string, next: string, inputType?: string): FieldEdit | null {
  const old = display.value;
  let p = 0;
  const most = Math.min(old.length, next.length);
  while (p < most && old.charCodeAt(p) === next.charCodeAt(p)) p++;
  let s = 0;
  while (s < most - p && old.charCodeAt(old.length - 1 - s) === next.charCodeAt(next.length - 1 - s)) s++;
  // Never between the two halves of a character.
  while (p > 0 && (isLow(old.charCodeAt(p)) || isLow(next.charCodeAt(p)))) p--;
  while (s > 0 && (isLow(old.charCodeAt(old.length - s)) || isLow(next.charCodeAt(next.length - s)))) s--;
  const oldEnd = old.length - s;
  const inserted = next.slice(p, next.length - s);
  const start = display.toText[p];
  const end = display.toText[oldEnd];
  if (inserted === '' && start === end) {
    if (oldEnd === p) return null;
    // Only soft breaks went: the character before them (Backspace) or after them (Delete) goes.
    const forward = inputType !== undefined && inputType.includes('Forward');
    if (forward) {
      if (start >= text.length) return null;
      return { text: text.slice(0, start) + text.slice(start + after(text, start)), caret: start, kind: 'delete', inserted: '' };
    }
    if (start <= 0) return null;
    const k = before(text, start);
    return { text: text.slice(0, start - k) + text.slice(start), caret: start - k, kind: 'delete', inserted: '' };
  }
  const kind = inserted === '' ? 'delete' : start === end ? 'insert' : 'replace';
  return { text: text.slice(0, start) + inserted + text.slice(end), caret: start + inserted.length, kind, inserted };
}

/** `text` with `inserted` in place of the offsets `start`…`end` (a letter of the Greek palette at the caret). */
export function insertText(text: string, start: number, end: number, inserted: string): FieldEdit {
  const [a, b] = [Math.max(0, Math.min(start, end)), Math.min(text.length, Math.max(start, end))];
  return { text: text.slice(0, a) + inserted + text.slice(b), caret: a + inserted.length, kind: a === b ? 'insert' : 'replace', inserted };
}

/** Changes typed within this many milliseconds of each other go into one undo step (a word typed is one step). */
const MERGE_MS = 1000;

/**
 * The field's own undo and redo, for one typing of a frame (the browser's is lost whenever the field's value is set again
 * to show a new wrapping): each step holds the text and its caret. Letters typed in a row make one step until a space, a
 * line break, a pause or another kind of change; a deletion of letters in a row another one.
 */
export class FieldHistory {
  private steps: { text: string; caret: number }[];
  private at = 0;
  private last: { kind: FieldEdit['kind']; time: number } | null = null;

  constructor(text: string, caret: number) {
    this.steps = [{ text, caret }];
  }

  /** The text after `edit`, at the time `time` (milliseconds). */
  record(edit: FieldEdit, time: number): void {
    const merge =
      this.last !== null &&
      this.at === this.steps.length - 1 &&
      this.at > 0 &&
      edit.kind !== 'replace' &&
      this.last.kind === edit.kind &&
      time - this.last.time < MERGE_MS &&
      !/\s/.test(edit.inserted);
    this.steps.length = this.at + 1;
    if (merge) this.steps[this.at] = { text: edit.text, caret: edit.caret };
    else {
      this.steps.push({ text: edit.text, caret: edit.caret });
      this.at += 1;
    }
    this.last = { kind: edit.kind, time };
  }

  canUndo(): boolean {
    return this.at > 0;
  }

  canRedo(): boolean {
    return this.at < this.steps.length - 1;
  }

  /** The step before, or null at the start of the typing. */
  undo(): { text: string; caret: number } | null {
    if (this.at === 0) return null;
    this.at -= 1;
    this.last = null;
    return this.steps[this.at];
  }

  /** The step undone last, or null. */
  redo(): { text: string; caret: number } | null {
    if (this.at >= this.steps.length - 1) return null;
    this.at += 1;
    this.last = null;
    return this.steps[this.at];
  }
}

/** The characters of `text` that no face has (`layout`'s missing glyphs), each once, in order. */
export function missingChars(text: string, layout: TextLayout): string[] {
  const offsets = layout.lines.flatMap((line) => line.glyphs.filter((g) => g.missing).map((g) => g.index));
  const found: string[] = [];
  for (const at of offsets.sort((a, b) => a - b)) {
    const c = String.fromCodePoint(text.codePointAt(at) ?? 0xfffd);
    if (!found.includes(c)) found.push(c);
  }
  return found;
}

/** `U+1F600` for a character. */
export function codePointName(c: string): string {
  return `U+${(c.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, '0')}`;
}
