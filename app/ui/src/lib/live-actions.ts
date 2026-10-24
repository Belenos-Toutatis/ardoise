/**
 * What the buttons and the keyboard of live mode do, shared by the control window and the
 * presentation window: commands go to Rust (`live_command`), view changes show at once and go
 * to Rust (`live_set_view`, see `LiveState.changeView`).
 */
import type { api as Api } from './api';
import type { LiveCommandKind, LiveViewState } from './live';
import type { LiveKeyAction } from './live-keys';
import type { LiveState } from './live-state.svelte';
import { PAN_STEP, ZOOM_STEP, panView, wholePage, zoomView } from './live-view';

/** The commands used (the real `api`, or a stand-in in tests). */
export type LiveCommands = Pick<typeof Api, 'liveCommand' | 'liveSetView'>;

export interface LiveActions {
  live: LiveState;
  commands: LiveCommands;
  /** Shape of the projected area (width / height). */
  aspect: () => number;
  /** A command was refused (e.g. the tablet is gone). */
  onerror: (error: unknown) => void;
}

export function sendCommand(actions: LiveActions, kind: LiveCommandKind): void {
  actions.commands.liveCommand({ kind }).catch(actions.onerror);
}

export function changeView(actions: LiveActions, view: LiveViewState): void {
  actions.live.changeView(view, (v) => actions.commands.liveSetView(v));
}

export function toggleBoard(actions: LiveActions): void {
  const view = actions.live.view;
  changeView(actions, { ...view, board: !view.board });
}

export function toggleLaser(actions: LiveActions): void {
  const view = actions.live.view;
  changeView(actions, { ...view, laser: !view.laser });
}

/** Zooms in (`direction` 1) or out (−1) by one step, around the centre of the projected area. */
export function zoomStep(actions: LiveActions, direction: 1 | -1): void {
  const { live } = actions;
  changeView(actions, zoomView(live.view, ZOOM_STEP ** direction, live.pageSize, actions.aspect()));
}

export function showWholePage(actions: LiveActions): void {
  changeView(actions, wholePage(actions.live.view, actions.live.pageSize));
}

/** Runs a keyboard action; `leave` is the window's own (stop live mode, close the presentation). */
export function runKeyAction(actions: LiveActions, action: LiveKeyAction, leave: () => void): void {
  const { live } = actions;
  switch (action.kind) {
    case 'command':
      sendCommand(actions, action.command);
      break;
    case 'board':
      toggleBoard(actions);
      break;
    case 'laser':
      toggleLaser(actions);
      break;
    case 'zoom':
      zoomStep(actions, action.direction);
      break;
    case 'wholePage':
      showWholePage(actions);
      break;
    case 'pan':
      changeView(actions, panView(live.view, action.dx * PAN_STEP, action.dy * PAN_STEP, live.pageSize, actions.aspect()));
      break;
    case 'leave':
      leave();
      break;
  }
}
