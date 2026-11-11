<script lang="ts" module>
  export type SideIconName = 'layers' | 'page' | 'background' | 'sound' | 'projection' | 'help' | 'collapse' | 'close';
</script>

<script lang="ts">
  /**
   * The icons of the side panel's rail and buttons (docs/design/side-panel.md, §2): inline SVG, 24 × 24, drawn with
   * `currentColor`, so that the tab or button gives them their colour; decorative (`aria-hidden`), the control
   * carries its name.
   */
  let { name }: { name: SideIconName } = $props();
</script>

<svg
  class="side-icon"
  viewBox="0 0 24 24"
  width="20"
  height="20"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === 'layers'}
    <path d="M12 3 3 8l9 5 9-5-9-5Z" />
    <path d="m3 12.5 9 5 9-5" />
    <path d="m3 17 9 5 9-5" />
  {:else if name === 'page'}
    <path d="M6 3h8l5 5v13H6Z" />
    <path d="M14 3v5h5" />
    <path d="M9 13h7M9 17h7" />
  {:else if name === 'background'}
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <circle cx="9" cy="10" r="1.6" />
    <path d="m3 17 5-5 4 4 3-3 6 5" />
  {:else if name === 'sound'}
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3M9 21h6" />
  {:else if name === 'projection'}
    <rect x="2" y="8" width="20" height="9" rx="2" />
    <circle cx="14" cy="12.5" r="2.6" />
    <path d="M6 12.5h2M6 20v-3M18 20v-3" />
  {:else if name === 'help'}
    <rect x="2" y="6" width="20" height="12" rx="2" />
    <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
  {:else if name === 'collapse'}
    <path d="m9 6 6 6-6 6" />
  {:else if name === 'close'}
    <path d="m6 6 12 12M18 6 6 18" />
  {/if}
</svg>
