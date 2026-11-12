<script lang="ts">
  /**
   * The editor's Projection tab (milestone M17; docs/design/paper-projection-narration.md §2.6): the screen and
   * « Project » (`ProjectorBlock`, with what projecting does here), the projected area (`ZoneBlock`: no blackboard, no
   * laser pointer, Q13; « Largeur » without live's key), then the share (`ShareBlock`). Same hooks as live's Projection
   * tab (`project`, `stop-projecting`, `[data-presentation]`, `[data-zoom]`, `whole-page`, `page-width`, `share`). The
   * list of screens is asked for the first time the tab shows.
   */
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { EditorProjection } from '../../lib/editor/projection.svelte';
  import { MAX_ZOOM, MIN_ZOOM } from '../../lib/live-view';
  import ShareBlock from '../share/ShareBlock.svelte';
  import ProjectorBlock from './ProjectorBlock.svelte';
  import ZoneBlock from './ZoneBlock.svelte';

  let { projection }: { projection: EditorProjection } = $props();

  onMount(() => {
    if (!projection.loaded) void projection.refresh();
  });
</script>

<ProjectorBlock
  monitors={projection.monitors}
  bind:monitorKey={projection.monitorKey}
  loaded={projection.loaded}
  presentation={projection.presentation}
  busy={projection.busy}
  error={projection.error}
  onproject={() => void projection.open()}
  onstop={() => void projection.close()}
  onrefresh={() => void projection.refresh()}
  help={t('editor.projection.help')}
/>
<ZoneBlock
  zoom={projection.zoom}
  min={MIN_ZOOM}
  max={MAX_ZOOM}
  onzoom={(direction) => projection.zoomStep(direction)}
  onwhole={() => projection.wholePage()}
  onwidth={() => projection.pageWidth()}
  hint={t('editor.projection.zoneHint')}
  widthTitle={t('editor.projection.widthTitle')}
/>
<ShareBlock />
