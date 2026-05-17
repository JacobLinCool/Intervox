<script lang="ts">
  import { css } from "$lib/util";

  let {
    value,
    onChange,
    min = 0,
    max = 100,
    disabled = false,
  }: {
    value: number;
    onChange: (v: number) => void;
    min?: number;
    max?: number;
    disabled?: boolean;
  } = $props();

  let trackEl: HTMLDivElement | undefined = $state();
  let drag = $state(false);

  function handle(e: MouseEvent) {
    if (disabled) return;
    if (!trackEl) return;
    const rect = trackEl.getBoundingClientRect();
    const x = (e.clientX - rect.left) / rect.width;
    const clamped = Math.max(0, Math.min(1, x));
    onChange(Math.round(min + clamped * (max - min)));
  }

  function commit(v: number) {
    if (disabled) return;
    onChange(Math.max(min, Math.min(max, Math.round(v))));
  }

  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 1;
    if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
      e.preventDefault();
      commit(value - step);
    } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
      e.preventDefault();
      commit(value + step);
    } else if (e.key === "Home") {
      e.preventDefault();
      commit(min);
    } else if (e.key === "End") {
      e.preventDefault();
      commit(max);
    }
  }

  $effect(() => {
    if (!drag) return;
    const move = (e: MouseEvent) => handle(e);
    const up = () => (drag = false);
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
  });

  let pct = $derived(((value - min) / (max - min)) * 100);
</script>

<div
  bind:this={trackEl}
  role="slider"
  tabindex="0"
  aria-valuemin={min}
  aria-valuemax={max}
  aria-valuenow={value}
  aria-disabled={disabled}
  aria-orientation="horizontal"
  onkeydown={handleKey}
  onmousedown={(e) => {
    drag = true;
    handle(e);
  }}
  style={css({
    flex: 1,
    height: 18,
    position: "relative",
    cursor: disabled ? "default" : "pointer",
    display: "flex",
    alignItems: "center",
  })}
>
  <div
    style={css({
      position: "absolute",
      left: 0,
      right: 0,
      height: 4,
      background: "rgba(120,120,128,0.22)",
      borderRadius: 2,
    })}
  ></div>
  <div
    style={css({
      position: "absolute",
      left: 0,
      width: `${pct}%`,
      height: 4,
      background: "var(--c-accent)",
      borderRadius: 2,
    })}
  ></div>
  <div
    style={css({
      position: "absolute",
      left: `calc(${pct}% - 9px)`,
      width: 18,
      height: 18,
      borderRadius: "50%",
      background: "#fff",
      boxShadow: "0 1px 4px rgba(0,0,0,0.25), 0 0 0 0.5px rgba(0,0,0,0.08)",
    })}
  ></div>
</div>
