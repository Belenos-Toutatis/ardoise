<script lang="ts">
  /**
   * Root of the presentation window (`index.html?view=presentation`, full screen on the
   * projector; docs/design/tablet-live-classroom.md, "Presentation window"): only the projected
   * page, with the view (zoom, pan), the blackboard and the laser dot. No element of interface;
   * the mouse cursor hides after 2 s without moving. The keyboard works as in the control window,
   * and Esc closes this window.
   *
   * The teacher can write over the page with the pen, a finger or the mouse (temporary marks, not
   * saved; the back of the pen, Delete or the button that shows with the cursor erase them) and
   * pinch with two fingers to zoom the projected area (docs/design/tablet-live-classroom.md,
   * "M8: writing over the presentation"). The marks stay in this window, which draws them itself;
   * they are also reported to Rust (`MarksSender`) for the viewers of the share.
   *
   * M14: the computer's stroke in progress (the control window's pen) is drawn from the session's events, and
   * the latency of its points is kept apart: `window.__ardoiseLatency.computer`.
   */
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { resolveLocale } from '../../lib/i18n-core';
  import { setLocale, t } from '../../lib/i18n.svelte';
  import { loadInk, type Ink } from '../../lib/ink';
  import { LatencyRecorder, exposeLatency } from '../../lib/latency';
  import { runKeyAction, type LiveActions } from '../../lib/live-actions';
  import { closeKey } from '../../lib/close-keys';
  import { liveKeyAction } from '../../lib/live-keys';
  import { MarksSender } from '../../lib/live-marks-sender';
  import { changeView } from '../../lib/live-actions';
  import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
  import { aspectOf } from '../../lib/live-view';
  import LiveCanvas from './LiveCanvas.svelte';

  /** The cursor hides after this long without moving. */
  const HIDE_CURSOR_MS = 2000;

  const live = new LiveState();
  const latency = new LatencyRecorder();
  /** The computer's points (M14: the control window's pen), measured apart from the tablet's. */
  const computerLatency = new LatencyRecorder();
  let ink = $state.raw<Ink | null>(null);
  let cursorHidden = $state(false);
  let cursorTimer: ReturnType<typeof setTimeout> | undefined;
  let canvas = $state<ReturnType<typeof LiveCanvas>>();
  /** Marks written over the page (the clear button shows while there are some). */
  let markCount = $state(0);
  const marksSender = new MarksSender((pageIndex, marks) => api.liveSetMarks(pageIndex, marks));

  const actions: LiveActions = {
    live,
    commands: api,
    aspect: () => aspectOf({ width: window.innerWidth, height: window.innerHeight }),
    onerror: () => {
      // Nothing can be shown on the projector; the control window reports problems.
    },
    clearMarks: () => canvas?.clearMarks(),
  };

  onMount(() => {
    let alive = true;
    exposeLatency(latency, window, computerLatency);
    // The page's accessible name follows the user's language.
    api
      .appState()
      .then((state) => {
        if (alive) setLocale(resolveLocale(state.language, state.systemLocale));
      })
      .catch(() => {
        // The default language stays.
      });
    loadInk()
      .then((module) => {
        if (alive) ink = module;
      })
      .catch(() => {
        // Without the geometry module the page and its paper still show.
      });
    subscribeLive(live, api.liveSubscribe).catch(() => {
      // Nothing to show until live mode runs; the window stays black.
    });
    wake();
    return () => {
      alive = false;
      clearTimeout(cursorTimer);
      marksSender.close();
      live.close();
    };
  });

  /** The mouse moved: show the cursor, and hide it again after a while. */
  function wake() {
    cursorHidden = false;
    clearTimeout(cursorTimer);
    cursorTimer = setTimeout(() => (cursorHidden = true), HIDE_CURSOR_MS);
  }

  function close() {
    api.closePresentation().catch(() => {
      // Already closed.
    });
  }

  function onkeydown(e: KeyboardEvent) {
    // Ctrl + Q in this window closes this window: the control window has the question about quitting.
    if (closeKey(e) === 'quit') {
      e.preventDefault();
      close();
      return;
    }
    const action = liveKeyAction(e);
    if (!action) return;
    e.preventDefault();
    runKeyAction(actions, action, close);
  }
</script>

<svelte:window {onkeydown} onmousemove={wake} onpointerdown={wake} onpointermove={wake} />

<main
  class="presentation"
  class:cursor-hidden={cursorHidden}
  aria-label={t('presentation.title')}
  data-state={live.ready ? live.state : 'idle'}
>
  <LiveCanvas
    bind:this={canvas}
    {live}
    {ink}
    mode="presentation"
    {latency}
    {computerLatency}
    label={t('presentation.canvas', { page: `${live.pageIndex + 1} / ${live.pageCount}` })}
    onview={(view) => changeView(actions, view)}
    onmarks={(count) => (markCount = count)}
    onmarkschange={(marks, current) => marksSender.update(live.pageIndex, marks, current)}
  />
  {#if markCount > 0 && !cursorHidden}
    <button type="button" class="clear-marks" data-action="clear-marks" onclick={() => canvas?.clearMarks()}>
      {t('presentation.clearMarks')}
    </button>
  {/if}
</main>

<style>
  .presentation {
    position: fixed;
    inset: 0;
    overflow: hidden;
    background: #000;
  }

  .clear-marks {
    position: fixed;
    right: 1rem;
    bottom: 1rem;
    padding: 0.6rem 1rem;
    border: 1px solid rgb(255 255 255 / 45%);
    border-radius: 8px;
    background: rgb(0 0 0 / 70%);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }

  .cursor-hidden,
  .cursor-hidden :global(*) {
    cursor: none !important;
  }
</style>
