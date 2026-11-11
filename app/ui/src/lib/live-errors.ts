/**
 * What the live control window says when a command of the computer's hand is refused (M14): in the words of what
 * failed. The codes the session gives (`errorCodes.*`) were written for the selection's touch-ups (« refaites la
 * sélection »): a layer operation or a stroke of the pen that fails must not ask to select again.
 */
import { errorMessage } from './api';
import { t } from './i18n.svelte';

/** What was refused: the computer's pen, its eraser end, an operation of the layers zone. */
export type ComputerAct = 'pen' | 'eraser' | 'layers';

export function computerErrorText(error: unknown, act: ComputerAct): string {
  const code = (error as { code?: unknown } | null)?.code;
  if (act === 'layers') {
    if (code === 'liveEditStale') return t('live.failed.layersStale');
    if (code === 'liveEditInvalid') return t('errorCodes.liveLayerInvalid');
  } else if (code === 'liveEditStale' || code === 'liveEditInvalid') {
    return t(act === 'pen' ? 'live.failed.pen' : 'live.failed.eraser');
  }
  return errorMessage(error);
}
