<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Home from './components/Home.svelte';
  import Wizard from './components/Wizard.svelte';
  import { api, errorMessage, onBridgeRequest, type AppState } from './lib/api';
  import { bridgeScreens, startAppBridge } from './lib/app-bridge.svelte';
  import { resolveLocale } from './lib/i18n-core';
  import { catalogs, currentLocale, setLocale, t } from './lib/i18n.svelte';
  import { uiSelfTest } from './lib/selftest';
  import { applyTheme } from './lib/theme';

  let appState = $state<AppState | null>(null);
  let loadError = $state<string | null>(null);

  function apply(next: AppState) {
    appState = next;
    setLocale(resolveLocale(next.language, next.systemLocale));
    applyTheme(next.theme);
  }

  // The assistant's bridge (MCP): requests of `ardoise.exe --mcp`, answered by the screens on display.
  onMount(() => startAppBridge({ listen: onBridgeRequest, reply: api.bridgeReply }));
  $effect(() => {
    bridgeScreens.app =
      loadError || appState?.settingsError ? 'error' : !appState ? 'loading' : appState.library ? 'home' : 'setup';
  });

  onMount(async () => {
    try {
      apply(await api.appState());
    } catch (error) {
      loadError = errorMessage(error);
    }
    if (appState?.selfTest) {
      await tick();
      await api.selfTestReport(uiSelfTest(document.getElementById('app'), catalogs, currentLocale()));
    }
  });
</script>

{#if loadError}
  <p class="banner error" role="alert">{t('errors.generic', { message: loadError })}</p>
{:else if !appState}
  <p class="loading">{t('common.loading')}</p>
{:else if appState.settingsError}
  <p class="banner error" role="alert">
    {t('errors.settings', { path: appState.settingsPath, message: appState.settingsError })}
  </p>
{:else if appState.library}
  <!-- A new library is a new screen: nothing of the previous one may stay on display. -->
  {#key appState.library}
    <Home app={appState} onchange={apply} />
  {/key}
{:else}
  <Wizard app={appState} onchange={apply} />
{/if}
