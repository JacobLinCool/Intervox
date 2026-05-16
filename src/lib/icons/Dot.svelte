<script lang="ts">
  import { css } from "$lib/util";
  let { size = 6, color = "currentColor", extraStyle = {} as Record<string, string | number> } = $props();
</script>

<span style={css({ display: 'inline-block', width: size, height: size, borderRadius: '50%', background: color, ...extraStyle })}></span>
