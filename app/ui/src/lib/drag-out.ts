/**
 * Dragging notes out of the app to other programs (Explorer, Word, OneNote, a mail…): the gesture
 * on a card's drag handle. Rust side: `drag.rs`.
 *
 * The browser's own drag and drop cannot hand files to another program, so the handle does not
 * use it (the card's HTML5 drag, which moves notes to a binder, stays untouched): the pointer
 * goes down on the handle, the notes are rendered to PNG files at once, and when the pointer has
 * moved a few pixels and the files exist, Rust starts the system's drag over them. The system
 * drag needs the mouse button to be down at that moment, so a release before the files are ready
 * simply ends the gesture.
 */
import { api, errorMessage } from './api';

/** Pointer distance (pixels) that turns a press on the handle into a drag. */
export const DRAG_OUT_THRESHOLD = 6;

/** `preparing`: rendering the files while the button is held; `dragging`: the system drag runs. */
export type DragOutPhase = 'idle' | 'preparing' | 'dragging';

export interface DragOutHooks {
  /** The notes to drag, read when the pointer goes down. */
  paths: () => string[];
  onphase: (phase: DragOutPhase) => void;
  /** A translated message for a failure. */
  onerror: (message: string) => void;
}

/** Whether this computer can drag notes out of the app (decided by Rust, once). */
let supported: Promise<boolean> | null = null;
export function dragOutSupported(): Promise<boolean> {
  supported ??= api.noteDragSupported().catch(() => false);
  return supported;
}

/** Forgets the answer (tests). */
export function resetDragOutSupport(): void {
  supported = null;
}

/** Handles the `pointerdown` on a drag handle. */
export function startDragOut(event: PointerEvent, hooks: DragOutHooks): void {
  if (event.button !== 0 || event.isPrimary === false) return;
  // No text selection, no focus change, and the browser keeps its own hands off the gesture.
  event.preventDefault();
  const pointerId = event.pointerId;

  const origin = { x: event.clientX, y: event.clientY };
  let pressed = true;
  let armed = false;
  let launched = false;
  let files: string[] | null = null;
  let failure: unknown = null;

  const release = () => {
    window.removeEventListener('pointermove', move, true);
    window.removeEventListener('pointerup', up, true);
    window.removeEventListener('pointercancel', up, true);
  };

  const launch = () => {
    if (launched || !files) return;
    launched = true;
    release();
    hooks.onphase('dragging');
    void api
      .startNoteDrag(files)
      .catch((error: unknown) => hooks.onerror(errorMessage(error)))
      .finally(() => hooks.onphase('idle'));
  };

  const move = (e: PointerEvent) => {
    if (armed || e.pointerId !== pointerId) return;
    if (Math.hypot(e.clientX - origin.x, e.clientY - origin.y) < DRAG_OUT_THRESHOLD) return;
    armed = true;
    if (files) launch();
    else if (!failure) hooks.onphase('preparing');
  };

  const up = (e: PointerEvent) => {
    if (e.pointerId !== pointerId) return;
    pressed = false;
    release();
    // Let go before the files were ready (or before moving): nothing to drag any more.
    if (!launched) hooks.onphase('idle');
  };

  window.addEventListener('pointermove', move, true);
  window.addEventListener('pointerup', up, true);
  window.addEventListener('pointercancel', up, true);

  api.prepareNoteDrag(hooks.paths()).then(
    (prepared) => {
      files = prepared;
      if (armed && pressed) launch();
    },
    (error: unknown) => {
      failure = error;
      release();
      if (!launched) hooks.onphase('idle');
      // A failure with nobody waiting for it (released already) is still worth telling.
      hooks.onerror(errorMessage(error));
    },
  );
}
