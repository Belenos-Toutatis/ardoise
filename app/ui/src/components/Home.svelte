<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppState } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import Diagnostic from './Diagnostic.svelte';
  import Logo from './Logo.svelte';
  import Settings from './Settings.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  // One side panel at a time.
  let panel = $state<'settings' | 'diagnostic' | null>(null);

  // Started with `--diagnostic`: the tablet test is shown right away.
  onMount(() => {
    if (app.openDiagnostic) panel = 'diagnostic';
  });

  function toggle(name: 'settings' | 'diagnostic') {
    panel = panel === name ? null : name;
  }
</script>

<div class="shell">
  <header class="topbar">
    <Logo size={32} />
    <strong class="brand">{t('app.name')}</strong>
    <span class="path" title={app.library ?? ''}>{app.library}</span>
    <button class="button ghost" aria-expanded={panel === 'diagnostic'} onclick={() => toggle('diagnostic')}>
      {t('diagnostic.open')}
    </button>
    <button class="button ghost" aria-expanded={panel === 'settings'} onclick={() => toggle('settings')}>
      {t('settings.title')}
    </button>
  </header>

  <main class="content">
    <section class="empty">
      <h2>{t('home.library')}</h2>
      <p class="lead">{t('home.empty')}</p>
      <p class="muted">{t('home.emptyHint')}</p>
    </section>
    {#if panel === 'settings'}
      <Settings {app} {onchange} onclose={() => (panel = null)} />
    {:else if panel === 'diagnostic'}
      <Diagnostic onclose={() => (panel = null)} />
    {/if}
  </main>
</div>
