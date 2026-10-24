<script lang="ts">
  /**
   * Tablet block at the bottom of the sidebar. Placeholder: it shows the registered tablet's name
   * (or that none is registered); milestone M4 replaces it with the connection state and battery.
   */
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';
  import type { TabletInfo } from '../../lib/note';

  let info = $state<TabletInfo | null>(null);

  onMount(() => {
    api.tabletInfo().then(
      (found) => (info = found),
      () => (info = null),
    );
  });
</script>

<div class="tablet-status" data-slot="tablet-status" title={t('library.tabletState')}>
  <span class="tablet-label">{t('library.tabletStatus')}</span>
  <span class="tablet-name" class:muted={!info}>{info ? info.name : t('library.tabletNone')}</span>
</div>
