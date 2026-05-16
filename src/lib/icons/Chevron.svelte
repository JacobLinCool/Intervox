<script lang="ts">
  import { css } from "$lib/util";
  let { size = 10, dir = "down" } = $props();
  const rotMap: Record<string, number> = { down: 0, up: 180, right: -90, left: 90 };
  let rot = $derived(rotMap[dir] ?? 0);
</script>

<svg width={size} height={size} viewBox="0 0 10 10" style={css({ transform: 'rotate(' + rot + 'deg)', verticalAlign: 'middle' })}>
  <path d="M2 3.5 L5 6.5 L8 3.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
