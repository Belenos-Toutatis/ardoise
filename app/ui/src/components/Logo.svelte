<script lang="ts">
  let { size = 40 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
  <rect x="4" y="8" width="56" height="48" rx="10" fill="var(--slate)" />
  <path
    d="M15 40c6-12 10-14 13-8s6 6 9-2 7-9 12 2"
    fill="none"
    stroke="var(--chalk)"
    stroke-width="4"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</svg>
