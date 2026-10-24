<script lang="ts">
  /**
   * Root of the presentation window (`index.html?view=presentation`, full screen on the
   * projector; docs/design/tablet-live-classroom.md, "Presentation window"): only the projected
   * page, with the view (zoom, pan), the blackboard and the laser dot. No element of interface;
   * the mouse cursor hides after 2 s without moving. The keyboard works as in the control window,
   * and Esc closes this window.
   */
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { resolveLocale } from '../../lib/i18n-core';
  import { setLocale, t } from '../../lib/i18n.svelte';
  import { loadInk, type Ink } from '../../lib/ink';
  import { LatencyRecorder, exposeLatency } from '../../lib/latency';
  import { runKeyAction, type LiveActions } from '../../lib/live-actions';
  import { liveKeyAction } from '../../lib/live-keys';
  import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
  import { aspectOf } from '../../lib/live-view';
  import LiveCanvas from './LiveCanvas.svelte';

  /** The cursor hides after this long without moving. */
  const HIDE_CURSOR_MS = 2000;

  const live = new LiveState();
  const latency = new LatencyRecorder();
  let ink = $state.raw<Ink | null>(null);
  let cursorHidden = $state(false);
  let cursorTimer: ReturnType<typeof setTimeout> | undefined;

  const actions: LiveActions = {
    live,
    commands: api,
    aspect: () => aspectOf({ width: window.innerWidth, height: window.innerHeight }),
    onerror: () => {
      // Nothing can be shown on the projector; the control window reports problems.
    },
  };

  onMount(() => {
    let alive = true;
    exposeLatency(latency);
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
    const action = liveKeyAction(e);
    if (!action) return;
    e.preventDefault();
    runKeyAction(actions, action, close);
  }
</script>

<svelte:window {onkeydown} onmousemove={wake} onpointerdown={wake} />

<main
  class="presentation"
  class:cursor-hidden={cursorHidden}
  aria-label={t('presentation.title')}
  data-state={live.ready ? live.state : 'idle'}
>
  <LiveCanvas {live} {ink} mode="presentation" {latency} label={t('presentation.canvas', { page: `${live.pageIndex + 1} / ${live.pageCount}` })} />
</main>

<style>
  .presentation {
    position: fixed;
    inset: 0;
    overflow: hidden;
    background: #000;
  }

  .cursor-hidden,
  .cursor-hidden :global(*) {
    cursor: none !important;
  }
</style>
