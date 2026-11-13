<script lang="ts">
  /**
   * The tablet's options (`[tablet]` of `ardoise.toml`): automatic sync, deleting pages after a
   * verified import (on by default), the button's action in live mode, and how the tablet is held
   * (four positions, each with its picture). Shared by the
   * tablet panel and the settings; changes go through the shared tablet state. The button and the
   * position are the Slate's: without a Slate (only the blackboard is known) they are not shown.
   */
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { ORIENTATIONS } from '../../lib/orientation';
  import type { LiveButton } from '../../lib/tablet';
  import { tablet } from '../../lib/tablet-state.svelte';
  import OrientationIcon from './OrientationIcon.svelte';

  onMount(() => tablet.start());

  const uid = $props.id();
  const options = $derived(tablet.status?.options ?? null);
  /** The button and the position of the tablet are the Slate's own. */
  const hasSlate = $derived(tablet.slate !== null);
  const hasBoard = $derived(tablet.board !== null);

  const BUTTONS: LiveButton[] = ['newPage', 'newLayer'];
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
    {#if hasBoard}
      <p class="muted small hint" data-slot="board-delete-hint">{t('tablet.options.deleteAfterImportBoard')}</p>
    {/if}

    {#if hasSlate}
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

      <div
        class="group"
        role="radiogroup"
        aria-label={t('tablet.options.orientation')}
        aria-describedby="{uid}-orientation"
      >
        <span class="group-label">{t('tablet.options.orientation')}</span>
        {#each ORIENTATIONS as orientation (orientation)}
          <label class="choice position">
            <input
              type="radio"
              name="{uid}-orientation"
              data-option="orientation"
              value={orientation}
              checked={options.orientation === orientation}
              onchange={() => tablet.setOptions({ orientation })}
            />
            <OrientationIcon {orientation} size={34} />
            {t(`tablet.orientation.${orientation}`)}
          </label>
        {/each}
        <p id="{uid}-orientation" class="muted small hint">{t('tablet.options.orientationHint')}</p>
      </div>
    {/if}
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

  .position {
    align-items: center;
    gap: 0.5rem;
  }

  .group-label {
    font-size: 0.875rem;
    font-weight: 600;
  }
</style>
