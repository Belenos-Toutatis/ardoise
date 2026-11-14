<script lang="ts">
  /**
   * What the screen says about the last opening of the guide (`lib/guide.svelte.ts`): that a web address was opened
   * because the `guide` folder is missing next to the executable (the only time Ardoise goes online, at the user's
   * request), or that the browser could not be started. One strip at the foot of the window, over any screen (library,
   * editor, live mode, dialogs), mounted once by `App.svelte`; it goes by itself after a while and has a close button.
   */
  import { dismissGuideNotice, guideNotice } from '../lib/guide.svelte';
  import { t } from '../lib/i18n.svelte';

  /** How long the notice stays when nobody closes it. */
  const SHOWN_MS = 12_000;

  $effect(() => {
    if (guideNotice.text === null) return;
    const timer = setTimeout(dismissGuideNotice, SHOWN_MS);
    return () => clearTimeout(timer);
  });
</script>

{#if guideNotice.text !== null}
  <div class="guide-notice banner" class:error={guideNotice.failed} role="status" data-slot="guide-notice" data-state={guideNotice.failed ? 'failed' : 'web'}>
    <span>{guideNotice.text}</span>
    <button type="button" class="button ghost" data-action="dismiss-guide-notice" onclick={dismissGuideNotice}>
      {t('guide.dismiss')}
    </button>
  </div>
{/if}

<style>
  .guide-notice {
    position: fixed;
    inset-inline: 0;
    bottom: 0;
    z-index: 200;
    display: flex;
    align-items: center;
    gap: 1rem;
    width: max-content;
    max-width: min(46rem, calc(100vw - 2rem));
    margin: 0 auto 1rem;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    box-shadow: 0 4px 16px rgb(0 0 0 / 25%);
  }

  /* An error keeps the danger colour of the text, on the same surface. */
  .guide-notice.error {
    color: var(--danger);
    background: var(--danger-bg);
  }
</style>
