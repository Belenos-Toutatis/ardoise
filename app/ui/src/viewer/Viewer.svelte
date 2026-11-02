<script lang="ts">
  /**
   * Root of the viewer page (`viewer.html`, served by the Ardoise PC to the browsers of the local
   * network; docs/design/lan-share.md, "The viewer"). Receives, never sends but the code. Screens:
   * the code form, "waiting", the shared page full window on black (a live sheet or the note open
   * in the editor), "reconnecting" over it, "sharing ended", "too many screens". A button (also a
   * double click on the page) goes full screen; it hides with the cursor after 2 s without moving.
   *
   * No Tauri and no `lib/api.ts` here: this page runs in any browser (a test checks the imports).
   */
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n.svelte';
  import { loadInk, type Ink } from '../lib/ink';
  import { ViewerState } from '../lib/viewer-state.svelte';
  import { ViewerStream, join, type JoinResult } from '../lib/viewer-stream';
  import CodeForm from './CodeForm.svelte';
  import ViewerCanvas from './ViewerCanvas.svelte';

  /** The cursor and the full-screen button hide after this long without moving. */
  const HIDE_CURSOR_MS = 2000;

  const viewer = new ViewerState();
  let ink = $state.raw<Ink | null>(null);
  let idle = $state(false);
  let fullscreen = $state(false);
  /** The code was just accepted: a `401` right after it means that the browser dropped the cookie. */
  let joined = false;
  let cookiesRefused = $state(false);
  let idleTimer: ReturnType<typeof setTimeout> | undefined;

  const canFullscreen = typeof document !== 'undefined' && document.fullscreenEnabled === true;

  const stream = new ViewerStream({
    onMessage: (message) => viewer.handle(message),
    onStatus: (status) => {
      if (status === 'open') {
        joined = false;
        cookiesRefused = false;
      } else if (status === 'needsCode') {
        cookiesRefused = joined;
        joined = false;
      }
      viewer.status(status);
    },
  });

  onMount(() => {
    let alive = true;
    loadInk()
      .then((module) => {
        if (alive) ink = module;
      })
      .catch(() => {
        // Without the geometry module the page and its paper still show.
      });
    stream.start();
    wake();
    return () => {
      alive = false;
      clearTimeout(idleTimer);
      stream.stop();
      viewer.close();
    };
  });

  $effect(() => {
    document.title = t('viewer.title');
  });

  async function onjoin(code: string): Promise<JoinResult> {
    cookiesRefused = false;
    const result = await join(code);
    if (result.ok) {
      joined = true;
      stream.start();
    }
    return result;
  }

  /** The mouse moved or a key was pressed: show the cursor and the button, hide them again after a while. */
  function wake() {
    idle = false;
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => (idle = true), HIDE_CURSOR_MS);
  }

  function toggleFullscreen() {
    if (!canFullscreen) return;
    if (document.fullscreenElement) document.exitFullscreen().catch(() => undefined);
    else document.documentElement.requestFullscreen().catch(() => undefined);
  }

  function ondblclick(e: MouseEvent) {
    if (viewer.shown && !(e.target instanceof Element && e.target.closest('button'))) toggleFullscreen();
  }

  const sheet = $derived(viewer.screen === 'waiting' || viewer.shown !== null);
</script>

<svelte:window onmousemove={wake} onpointerdown={wake} onpointermove={wake} onkeydown={wake} ontouchstart={wake} />
<svelte:document onfullscreenchange={() => (fullscreen = !!document.fullscreenElement)} />

<!-- A double click is a shortcut for the button, which keyboards use. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<main
  class="viewer"
  class:idle
  data-screen={viewer.tooMany ? 'tooMany' : viewer.screen}
  data-reconnecting={viewer.reconnecting}
  data-paused={viewer.paused}
  {ondblclick}
>
  {#if viewer.tooMany}
    <section class="panel" data-view="too-many">
      <h1 class="brand">{t('viewer.brand')}</h1>
      <h2>{t('viewer.tooMany.title')}</h2>
      <p>{t('viewer.tooMany.text')}</p>
    </section>
  {:else if viewer.screen === 'code'}
    <CodeForm {onjoin} {cookiesRefused} />
  {:else if viewer.screen === 'waiting'}
    <section class="panel" data-view="waiting">
      <h1 class="brand">{t('viewer.brand')}</h1>
      <h2>{t('viewer.waiting.title')}</h2>
      <p>{t('viewer.waiting.text')}</p>
    </section>
  {:else if viewer.screen === 'ended'}
    <section class="panel" data-view="ended">
      <h1 class="brand">{t('viewer.brand')}</h1>
      <h2>{t('viewer.ended.title')}</h2>
      <p>{t('viewer.ended.text')}</p>
    </section>
  {:else if viewer.shown}
    <ViewerCanvas live={viewer.live} {ink} background={viewer.background} shown={viewer.shown} label={t('viewer.canvas')} />
  {:else}
    <section class="panel" data-view="connecting">
      <h1 class="brand">{t('viewer.brand')}</h1>
      <p role="status">{viewer.reconnecting ? t('viewer.reconnecting') : t('viewer.connecting')}</p>
    </section>
  {/if}

  {#if sheet && !viewer.tooMany}
    {#if viewer.paused}
      <div class="badge" role="status" data-badge="paused">{t('viewer.paused')}</div>
    {/if}
    {#if viewer.reconnecting}
      <div class="banner" role="status" data-banner="reconnecting">{t('viewer.reconnecting')}</div>
    {/if}
  {/if}

  {#if canFullscreen && viewer.shown && !idle}
    <button type="button" class="fullscreen" data-action="fullscreen" onclick={toggleFullscreen}>
      {fullscreen ? t('viewer.fullscreen.exit') : t('viewer.fullscreen.enter')}
    </button>
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    height: 100%;
    margin: 0;
    background: #000;
  }

  .viewer {
    --text: #e8eaee;
    --muted: #a3acb9;
    --border: #3a4350;
    --field: #1b2028;
    --accent: #6c9bf0;
    --accent-text: #0d1420;
    --danger: #ff8a80;
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
    overflow: hidden;
    background: #000;
    color: var(--text);
    font-family: 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    font-size: 16px;
    line-height: 1.5;
  }

  .viewer.idle,
  .viewer.idle :global(*) {
    cursor: none !important;
  }

  .panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    max-width: 32rem;
    text-align: center;
  }

  .panel h1 {
    margin: 0;
    font-size: 2.4rem;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .panel h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
  }

  .panel p {
    margin: 0;
    color: var(--muted);
  }

  .badge,
  .banner,
  .fullscreen {
    position: fixed;
    padding: 0.35rem 0.8rem;
    border: 1px solid rgb(255 255 255 / 35%);
    border-radius: 999px;
    background: rgb(0 0 0 / 65%);
    color: #fff;
    font: inherit;
    font-size: 0.9rem;
  }

  .badge {
    top: 0.75rem;
    right: 0.75rem;
  }

  .banner {
    top: 0.75rem;
    left: 50%;
    transform: translateX(-50%);
  }

  .fullscreen {
    right: 0.75rem;
    bottom: 0.75rem;
    cursor: pointer;
  }

  .fullscreen:hover,
  .fullscreen:focus-visible {
    background: rgb(0 0 0 / 85%);
  }
</style>
