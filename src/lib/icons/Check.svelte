<script lang="ts">
  let { size = 12, color = "currentColor" } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 12 12" style="vertical-align:middle">
  <path d="M2.5 6.2 L4.8 8.5 L9.5 3.5" fill="none" stroke={color} stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
