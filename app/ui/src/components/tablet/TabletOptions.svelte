<script lang="ts">
  /**
   * The tablet's options (`[tablet]` of `ardoise.toml`): automatic sync, deleting pages after a
   * verified import (on by default), the button's action in live mode, and the hand. Shared by the
   * tablet panel and the settings; changes go through the shared tablet state.
   */
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { Hand, LiveButton } from '../../lib/tablet';
  import { tablet } from '../../lib/tablet-state.svelte';

  onMount(() => tablet.start());

  const uid = $props.id();
  const options = $derived(tablet.status?.options ?? null);

  const BUTTONS: LiveButton[] = ['newPage', 'newLayer'];
  const HANDS: Hand[] = ['right', 'left'];
</script>

{#if options}
  <div class="tablet-options" data-slot="tablet-options">
    <label class="choice">
      <input
        type="checkbox"
        role="switch"
        data-option="autoSync"
        checked={options.autoSync}
        aria-describedby="{uid}-auto"
        onchange={(event) => tablet.setOptions({ autoSync: event.currentTarget.checked })}
      />
      {t('tablet.options.autoSync')}
    </label>
    <p id="{uid}-auto" class="muted small hint">{t('tablet.options.autoSyncHint')}</p>

    <label class="choice">
      <input
        type="checkbox"
        role="switch"
        data-option="deleteAfterImport"
        checked={options.deleteAfterImport}
        aria-describedby="{uid}-delete"
        onchange={(event) => tablet.setOptions({ deleteAfterImport: event.currentTarget.checked })}
      />
      {t('tablet.options.deleteAfterImport')}
    </label>
    <p id="{uid}-delete" class="muted small hint">{t('tablet.options.deleteAfterImportHint')}</p>

    <div class="group" role="radiogroup" aria-label={t('tablet.options.button')}>
      <span class="group-label">{t('tablet.options.button')}</span>
      {#each BUTTONS as button (button)}
        <label class="choice">
          <input
            type="radio"
            name="{uid}-button"
            data-option="button"
            value={button}
            checked={options.button === button}
            onchange={() => tablet.setOptions({ button })}
          />
          {t(button === 'newPage' ? 'tablet.options.buttonNewPage' : 'tablet.options.buttonNewLayer')}
        </label>
      {/each}
    </div>

    <div class="group" role="radiogroup" aria-label={t('tablet.options.hand')} aria-describedby="{uid}-hand">
      <span class="group-label">{t('tablet.options.hand')}</span>
      {#each HANDS as hand (hand)}
        <label class="choice">
          <input
            type="radio"
            name="{uid}-hand"
            data-option="hand"
            value={hand}
            checked={options.hand === hand}
            onchange={() => tablet.setOptions({ hand })}
          />
          {t(hand === 'right' ? 'tablet.options.handRight' : 'tablet.options.handLeft')}
        </label>
      {/each}
      <p id="{uid}-hand" class="muted small hint">{t('tablet.options.handHint')}</p>
    </div>
  </div>
{/if}

<style>
  .tablet-options {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .hint {
    margin: 0 0 0.6rem 1.6rem;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    margin-top: 0.4rem;
  }

  .group-label {
    font-size: 0.875rem;
    font-weight: 600;
  }
</style>
