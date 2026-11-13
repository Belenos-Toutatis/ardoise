<script lang="ts">
  /**
   * The Xiaomi blackboard seen from the front, in one of its four positions: a portrait tablet with a dark screen and,
   * on the band under the screen, the bin (erase the screen) on the left and the floppy disk (store the page) on the
   * right. Upright is `buttonsBottom`; the other positions turn the same picture by quarter turns, so the two marks
   * show where the buttons end up. Decorative: the label next to it says the same in words. The marks are strokes of
   * the tablet's own colour (`currentColor`), legible down to about 26 px high.
   */
  import { drawingTurns, isLandscapePosition, type BoardPosition } from '../../lib/board-position';

  /** `size` is the long side of the picture, in pixels. */
  let { position = 'buttonsBottom', size = 40 }: { position?: BoardPosition; size?: number } = $props();

  const turns = $derived(drawingTurns(position));
  const landscape = $derived(isLandscapePosition(position));
  const short = $derived(Math.round((size * 46) / 62));
  /** Where the upright picture (46 × 62) goes in the turned frame. */
  const transform = $derived(
    turns === 1 ? 'translate(62 0) rotate(90)' : turns === 2 ? 'rotate(180 23 31)' : turns === 3 ? 'translate(0 46) rotate(-90)' : undefined,
  );
</script>

<svg
  class="board-drawing"
  data-position={position}
  width={landscape ? size : short}
  height={landscape ? short : size}
  viewBox={landscape ? '0 0 62 46' : '0 0 46 62'}
  aria-hidden="true"
  focusable="false"
>
  <g {transform}>
    <rect class="frame" x="1.5" y="1.5" width="43" height="59" rx="6" />
    <rect class="screen" x="5" y="5" width="36" height="43" rx="1.5" />
    <g class="marks">
      <!-- The bin: erase the screen. -->
      <path d="M12.8 52.4h5.4M13.8 52.4v4.6h3.4v-4.6M14.8 51.2h1.4" />
      <!-- The floppy disk: store the page. -->
      <path d="M28.4 51.6h3.8l1 1v4.4h-4.8zM29.8 51.6v1.7h2.2M29.8 57v-2h2.2v2" />
    </g>
  </g>
</svg>

<style>
  .board-drawing {
    flex: none;
  }

  .frame,
  .marks {
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .frame {
    stroke-width: 2;
  }

  .marks {
    stroke-width: 1.4;
  }

  .screen {
    fill: #1f2328;
  }
</style>
