<script lang="ts">
  /**
   * The editor's Projection tab (milestone M17; docs/design/paper-projection-narration.md §2.6): the screen and
   * « Project » (`ProjectorBlock`), the projected area (`ZoneBlock`: no blackboard, no laser pointer), then the share
   * (`ShareBlock`). Same hooks as live's Projection tab.
   *
   * Package 0: wired to the no-op `EditorProjection`; package D completes it (the blocks' `help` and `widthTitle`).
   */
  import { t } from '../../lib/i18n.svelte';
  import type { EditorProjection } from '../../lib/editor/projection.svelte';
  import { MAX_ZOOM, MIN_ZOOM } from '../../lib/live-view';
  import ShareBlock from '../share/ShareBlock.svelte';
  import ProjectorBlock from './ProjectorBlock.svelte';
  import ZoneBlock from './ZoneBlock.svelte';

  let { projection }: { projection: EditorProjection } = $props();
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
/>
<ZoneBlock
  zoom={projection.zoom}
  min={MIN_ZOOM}
  max={MAX_ZOOM}
  onzoom={(direction) => projection.zoomStep(direction)}
  onwhole={() => projection.wholePage()}
  onwidth={() => projection.pageWidth()}
  hint={t('editor.projection.zoneHint')}
/>
<ShareBlock />
