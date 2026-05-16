<script lang="ts">
  let { size = 16, color = "currentColor", filled = false } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" style="display:inline-block;vertical-align:middle">
  {#if filled}
    <path d="M4 6.5C4 5.12 5.12 4 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7A2.5 2.5 0 0 1 17.5 16H13l-3.2 3.4c-.4.42-1.1.14-1.1-.43V16H6.5A2.5 2.5 0 0 1 4 13.5v-7Z" fill={color}/>
    <path d="M9.5 8.5 7.5 10l2 1.5M14.5 8.5l2 1.5-2 1.5" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
  {:else}
    <path d="M4 6.5C4 5.12 5.12 4 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7A2.5 2.5 0 0 1 17.5 16H13l-3.2 3.4c-.4.42-1.1.14-1.1-.43V16H6.5A2.5 2.5 0 0 1 4 13.5v-7Z"
          stroke={color} stroke-width="1.6" fill="none" stroke-linejoin="round"/>
    <path d="M9.5 8.5 7.5 10l2 1.5M14.5 8.5l2 1.5-2 1.5" stroke={color} stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
  {/if}
</svg>
