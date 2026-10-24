<script lang="ts">
  import type { AppState } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import Logo from './Logo.svelte';
  import Settings from './Settings.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  let settingsOpen = $state(false);
</script>

<div class="shell">
  <header class="topbar">
    <Logo size={32} />
    <strong class="brand">{t('app.name')}</strong>
    <span class="path" title={app.library ?? ''}>{app.library}</span>
    <button class="button ghost" aria-expanded={settingsOpen} onclick={() => (settingsOpen = !settingsOpen)}>
      {t('settings.title')}
    </button>
  </header>

  <main class="content">
    <section class="empty">
      <h2>{t('home.library')}</h2>
      <p class="lead">{t('home.empty')}</p>
      <p class="muted">{t('home.emptyHint')}</p>
    </section>
    {#if settingsOpen}
      <Settings {app} {onchange} onclose={() => (settingsOpen = false)} />
    {/if}
  </main>
</div>
