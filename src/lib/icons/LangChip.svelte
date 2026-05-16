<script lang="ts">
  import { css } from "$lib/util";
  let { code, size = 18 }: { code: string; size?: number } = $props();
</script>

<span style={css({
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  minWidth: size + 4,
  height: size,
  padding: '0 4px',
  borderRadius: 4,
  background: 'color-mix(in oklch, var(--c-mixed) 14%, transparent)',
  color: 'var(--c-mixed)',
  fontSize: Math.max(9.5, size * 0.55),
  fontWeight: 700,
  letterSpacing: 0.4,
  fontFamily: 'ui-monospace, SF Mono, Menlo, monospace',
  textTransform: 'uppercase',
  lineHeight: 1,
  flexShrink: 0,
})}>{code}</span>
