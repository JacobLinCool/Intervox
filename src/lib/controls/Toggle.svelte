<script lang="ts">
  import { css } from "$lib/util";

  let {
    value,
    onChange,
    tint = "var(--c-translate)",
    ariaLabel = "Toggle setting",
    disabled = false,
  }: {
    value: boolean;
    onChange: (v: boolean) => void;
    tint?: string;
    ariaLabel?: string;
    disabled?: boolean;
  } = $props();
</script>

<button
  onclick={() => { if (!disabled) onChange(!value); }}
  aria-label={ariaLabel}
  aria-pressed={value}
  disabled={disabled}
  style={css({
    width: 36,
    height: 22,
    borderRadius: 11,
    border: "none",
    padding: 2,
    background: value ? tint : "rgba(120,120,128,0.28)",
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.55 : 1,
    transition: "background 160ms",
    display: "inline-flex",
    alignItems: "center",
  })}
>
  <span
    style={css({
      width: 18,
      height: 18,
      borderRadius: "50%",
      background: "#fff",
      transform: value ? "translateX(14px)" : "translateX(0)",
      transition: "transform 160ms cubic-bezier(.2,.8,.3,1)",
      boxShadow: "0 1px 3px rgba(0,0,0,0.25), 0 0 0 0.5px rgba(0,0,0,0.05)",
    })}
  ></span>
</button>
