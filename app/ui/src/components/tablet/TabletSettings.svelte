<script lang="ts">
  /**
   * The settings of the tablets, one tablet at a time (Q17): a list of the known tablets (shown only when there are two;
   * the connected tablet is chosen to begin with, else the first), then that tablet's options (`TabletOptions`) and,
   * for the Bamboo Slate in the tablet panel, its own settings: its name and its clock (the blackboard has no name to
   * change, and its clock is set at each connection). Shared by the tablet panel (`compact`: long hints folded, no
   * scrolling) and the settings.
   */
  import { onMount, untrack } from 'svelte';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import type { TabletKind } from '../../lib/tablet';
  import { tablet, tabletName, validTabletName } from '../../lib/tablet-state.svelte';
  import TabletOptions from './TabletOptions.svelte';

  let { manage = true, compact = false }: { manage?: boolean; compact?: boolean } = $props();

  onMount(() => tablet.start());

  const uid = $props.id();
  /** The tablet the user chose in the list; null until one is, and again whenever another tablet connects. */
  let picked = $state<TabletKind | null>(null);
  /** The name as being edited. */
  let draft = $state(untrack(() => tablet.status?.registered?.name ?? ''));
  /** The name was submitted and refused for its form. */
  let nameRefused = $state(false);

  const known = $derived(tablet.known);
  /** The tablet whose settings are shown. */
  const kind = $derived<TabletKind | null>(
    picked !== null && known.some((entry) => entry.kind === picked) ? picked : (tablet.active ?? known[0]?.kind ?? null),
  );
  const registered = $derived(tablet.status?.registered ?? null);
  const registeredName = $derived(registered?.name ?? '');
  const drift = $derived(
    tablet.drift === null
      ? null
      : new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: 1 }).format(Math.abs(tablet.drift)),
  );

  // The connected tablet is the one to set, until the user chooses: another one connecting takes the choice back.
  $effect(() => {
    if (tablet.active !== null) picked = null;
  });

  // A new name (this PC's rename, or another tablet) replaces what is in the field.
  $effect(() => {
    draft = registeredName;
    nameRefused = false;
  });

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    nameRefused = !validTabletName(draft);
    if (!nameRefused) await tablet.rename(draft);
  }
</script>

<div class="tablet-settings" data-slot="tablet-settings">
  {#if kind === null}
    <p class="muted" data-slot="settings-none">{t('tablet.settings.none')}</p>
  {:else}
    {#if known.length > 1}
      <label class="field chooser">
        <span>{t('tablet.settings.choose')}</span>
        <select
          data-slot="settings-tablet"
          value={kind}
          onchange={(event) => (picked = event.currentTarget.value as TabletKind)}
        >
          {#each known as entry (entry.kind)}
            <option value={entry.kind}>{tabletName(entry)}</option>
          {/each}
        </select>
      </label>
    {/if}

    <TabletOptions {kind} {compact} />

    {#if manage && kind === 'slate' && registered}
      <!-- The Slate's own settings: the blackboard has no name to change, and its clock is set at each connection. -->
      <section class="own" aria-labelledby="{uid}-manage">
        <h3 id="{uid}-manage" class:visually-hidden={compact}>{t('tablet.manage.title')}</h3>
        <form class="field" onsubmit={rename}>
          <label for="tablet-name"><span>{t('tablet.manage.name')}</span></label>
          <div class="inline">
            <input
              id="tablet-name"
              class="text-input"
              type="text"
              bind:value={draft}
              maxlength="40"
              aria-invalid={nameRefused}
              aria-describedby="tablet-name-hint"
              oninput={() => (nameRefused = false)}
            />
            <button
              type="submit"
              class="button"
              data-action="rename"
              disabled={tablet.working || draft.trim() === registered.name}
            >
              {t('tablet.manage.rename')}
            </button>
          </div>
          <span id="tablet-name-hint" class="muted small field-hint">{t('tablet.manage.nameHint')}</span>
          {#if nameRefused}
            <span class="error small" role="alert">{t('tablet.manage.nameInvalid')}</span>
          {:else if tablet.renamed}
            <span class="muted small" role="status">{t('tablet.manage.renamed')}</span>
          {/if}
        </form>

        <div class="field">
          <span class="muted small">{t('tablet.manage.timeHint')}</span>
          <button type="button" class="button" data-action="set-time" disabled={tablet.working} onclick={() => tablet.setTime()}>
            {t('tablet.manage.setTime')}
          </button>
          {#if drift !== null}
            <span class="muted small" role="status" data-slot="time-result">{t('tablet.manage.timeSet', { seconds: drift })}</span>
          {/if}
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  .chooser {
    margin: 0 0 0.75rem;
  }

  .chooser select {
    width: 100%;
  }

  .own {
    margin-top: 0.6rem;
    padding-top: 0.4rem;
    border-top: 1px solid var(--border);
  }

  .own .field {
    margin: 0.55rem 0;
  }

  .own h3 {
    margin: 0 0 0.4rem;
    font-size: 0.95rem;
  }

  .inline {
    display: flex;
    gap: 0.5rem;
  }

  .inline input {
    flex: 1;
    min-width: 0;
  }
</style>
