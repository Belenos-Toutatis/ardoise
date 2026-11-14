/**
 * What Escape does in the editor, by steps, the smallest first (SPEC §4.4 « Échap dans l'éditeur », docs/questions.md Q20,
 * docs/design/interface.md, "Escape in the editor"): the action in progress (a gesture; typing in a text frame, which
 * ends with the text kept; an equation being written, which is dropped), then the panel or the tool mode that is open,
 * then the selection, then the note is saved and the editor goes back to the library. One press does one step.
 * Pure, so that the order is tested alone; `Editor.svelte` gathers the state and does what the step says.
 */

/** What the editor is in when Escape is pressed. */
export interface EscapeState {
  /** The unsaved-changes question is open (its answer « Rester » is what Escape gives). */
  leaving: boolean;
  /** A dialog or a panel that answers Escape itself is open (a modal, the export, the print, the video, the share). */
  owned: boolean;
  /** A gesture is under way: a stroke, a shape, a lasso, a move, an erasing… */
  dragging: boolean;
  /** A text frame is being typed (its field has the key: `typing` is true too). */
  editingText?: boolean;
  /** The equation tool's panel is open (an equation being written, not yet put down or updated). */
  editingEquation?: boolean;
  /** The cutting panel is open. */
  split: boolean;
  /** The setting of « Redresser les formes » is open. */
  settings: boolean;
  /** The key is typed in a text field, a select or an editable element: it is that control's own. */
  typing: boolean;
  /** A question waits for its answer inside the editor (the deletion of the narration, the PDF page to choose…). */
  question: boolean;
  /** The replay is on. */
  replaying: boolean;
  /** The manual crop is on. */
  cropping: boolean;
  /** How many strokes and objects are selected. */
  selected: number;
}

export type EscapeStep =
  /** Nothing: another dialog or the field has it. */
  | 'ignore'
  /** « Rester » to the unsaved-changes question. */
  | 'stay'
  /** The gesture under way is dropped without changing the note. */
  | 'cancelDrag'
  /** The text frame being typed is left, its text kept (an empty frame goes). */
  | 'endText'
  /** The equation being written is dropped: the page stays as it was (nothing is put down or changed). */
  | 'cancelEquation'
  | 'closeSplit'
  | 'closeSettings'
  /** The inner question is dropped (or, while it is being answered, left alone). */
  | 'question'
  | 'leaveReplay'
  | 'leaveCrop'
  | 'deselect'
  /** Save the note and go back to the library, as the back button does (nothing while a recording runs). */
  | 'leave';

export function escapeStep(state: EscapeState): EscapeStep {
  if (state.leaving) return 'stay';
  if (state.owned) return 'ignore';
  if (state.dragging) return 'cancelDrag';
  if (state.editingText) return 'endText';
  if (state.editingEquation) return 'cancelEquation';
  if (state.split) return 'closeSplit';
  if (state.settings) return 'closeSettings';
  if (state.typing) return 'ignore';
  if (state.question) return 'question';
  if (state.replaying) return 'leaveReplay';
  if (state.cropping) return 'leaveCrop';
  if (state.selected > 0) return 'deselect';
  return 'leave';
}
