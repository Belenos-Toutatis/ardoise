<script lang="ts">
  /**
   * Root of the editor's projection window (`index.html?view=projection`, full screen on the projector; milestone M17,
   * docs/design/paper-projection-narration.md §2.5): only the editor's page, drawn as live's presentation window draws
   * its own (`LiveState` fed by `projection_subscribe`, which speaks live's vocabulary, and `LiveCanvas` in presentation
   * mode), on black, the cursor hidden after 2 s without moving.
   *
   * Display-only (§11 O6): no temporary marks, no pinch, no wheel (`interactive={false}`); the teacher writes in the
   * editor itself. Esc, Ctrl+W, Ctrl+F4 and Ctrl+Q close the window (`projection_close`; the editor's tab hears it
   * through `projection_watch`); no other key does anything.
   *
   * The paper is the editor's: `render_background` with the editor's staged pixels (`imagePixels.key`, in the same
   * process) when the page has an image and they are staged, else the paper alone. The editor's stroke in progress
   * arrives as the computer's, its points stamped with their pointer sample's wall time: its latency is
   * `window.__ardoiseLatency.computer`.
   */
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { closeKey } from '../../lib/close-keys';
  import { resolveLocale } from '../../lib/i18n-core';
  import { setLocale, t } from '../../lib/i18n.svelte';
  import { loadInk, type Ink } from '../../lib/ink';
  import { LatencyRecorder, exposeLatency } from '../../lib/latency';
  import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
  import type { Background, PageSize } from '../../lib/note';
  import LiveCanvas from '../live/LiveCanvas.svelte';

  /** The cursor hides after this long without moving. */
  const HIDE_CURSOR_MS = 2000;

  const live = new LiveState();
  /** The tablet's points never come here; the editor's are the computer's. */
  const latency = new LatencyRecorder();
  const computerLatency = new LatencyRecorder();
  let ink = $state.raw<Ink | null>(null);
  let cursorHidden = $state(false);
  let cursorTimer: ReturnType<typeof setTimeout> | undefined;

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
    subscribeLive(live, api.projectionSubscribe).catch(() => {
      // Nothing to show until the editor sends its page; the window stays black.
    });
    wake();
    return () => {
      alive = false;
      clearTimeout(cursorTimer);
      live.close();
    };
  });

  /** The paper of the editor's page: its staged pixels when it has an image and they are staged (read in the canvas's effect). */
  function paper(page: PageSize, background: Background, widthPx: number): Promise<string> {
    const image = live.pageImage;
    const pixels = live.imagePixels;
    return api.renderBackground(page, background, widthPx, image && pixels ? { image, key: pixels.key } : undefined);
  }

  /** The mouse moved: show the cursor, and hide it again after a while. */
  function wake() {
    cursorHidden = false;
    clearTimeout(cursorTimer);
    cursorTimer = setTimeout(() => (cursorHidden = true), HIDE_CURSOR_MS);
  }

  function close() {
    api.projectionClose().catch(() => {
      // Already closed.
    });
  }

  function onkeydown(e: KeyboardEvent) {
    const plainEscape = e.key === 'Escape' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
    if (plainEscape || closeKey(e) !== null) {
      e.preventDefault();
      if (!e.repeat) close();
    }
  }
</script>

<svelte:window {onkeydown} onmousemove={wake} onpointerdown={wake} onpointermove={wake} />

<main
  class="projection"
  class:cursor-hidden={cursorHidden}
  aria-label={t('presentation.editorTitle')}
  data-state={live.ready ? live.state : 'idle'}
>
  <LiveCanvas
    {live}
    {ink}
    mode="presentation"
    {latency}
    {computerLatency}
    label={t('presentation.editorCanvas')}
    interactive={false}
    {paper}
  />
</main>

<style>
  .projection {
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
