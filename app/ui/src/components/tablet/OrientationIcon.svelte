<script lang="ts">
  /**
   * The tablet on the desk in one of its four positions, seen from above. Landscape is the
   * tablet as drawn in the registration wizard (TabletDrawing): the button (blue ring) and the
   * light are on the bottom edge. The other positions turn that picture by quarter turns, so the
   * button shows where it ends up. Decorative: the label next to it says the same in words.
   */
  import { quarterTurns, type Orientation } from '../../lib/orientation';

  let { orientation, size = 40 }: { orientation: Orientation; size?: number } = $props();

  const angle = $derived(quarterTurns(orientation) * 90);
</script>

<svg
  class="orientation-icon"
  data-orientation={orientation}
  width={size}
  height={size}
  viewBox="0 0 48 48"
  aria-hidden="true"
  focusable="false"
>
  <g transform="rotate({angle} 24 24)">
    <rect class="body" x="4" y="11" width="40" height="26" rx="4" />
    <rect class="paper" x="8" y="14" width="32" height="15" rx="1.5" />
    <circle class="led" cx="12" cy="33" r="1.4" />
    <circle class="button" cx="36" cy="33" r="2.2" />
  </g>
</svg>

<style>
  .orientation-icon {
    flex: none;
  }

  .body {
    fill: var(--slate);
  }

  .paper {
    fill: var(--surface);
    stroke: var(--border);
  }

  .led {
    fill: #3b82f6;
  }

  .button {
    fill: #cfd6e0;
    stroke: #3b82f6;
    stroke-width: 1.2;
  }
</style>
