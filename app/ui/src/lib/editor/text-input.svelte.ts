/**
 * What the Greek palette of the text tool (`GreekPalette.svelte`) shares with the field of the frame being typed
 * (`TextField.svelte`) and the editor (`Editor.svelte`): the field registers itself while it is there (to insert at its
 * caret, to take the focus back, to end the typing), and the palette's open state is the editor's (Escape closes it
 * before it ends the typing, a change of tool closes it: `lib/editor/escape.ts`).
 */

export interface TextInputTarget {
  /**
   * Inserts `text` at the field's caret (in place of its selection), as one change of the typing. `refocus`: the focus goes
   * back to the field (a press of the pointer never took it; a key of the keyboard did, and keeps it).
   */
  insert(text: string, refocus: boolean): void;
  /** Gives the focus back to the field (the palette closed with Escape). */
  focus(): void;
  /** Ends the typing, as leaving the field does (the focus went from the palette to elsewhere). */
  end(): void;
}

class TextInputState {
  /** The field of the frame being typed, while there is one. */
  target = $state.raw<TextInputTarget | null>(null);
  /** The Greek palette is open. */
  paletteOpen = $state(false);
}

export const textInput = new TextInputState();

/** The field of a frame registers itself; the function returned takes it away (only if it is still the one). */
export function registerTextInput(target: TextInputTarget): () => void {
  textInput.target = target;
  return () => {
    if (textInput.target === target) textInput.target = null;
  };
}
