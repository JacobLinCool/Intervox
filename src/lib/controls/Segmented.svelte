<script lang="ts">
  let {
    value,
    options,
    onChange,
    ariaLabel = "Options",
  }: {
    value: any;
    options: { value: any; label: string }[];
    onChange: (v: any) => void;
    ariaLabel?: string;
  } = $props();

  function selectOffset(offset: number) {
    const current = options.findIndex((o) => o.value === value);
    const base = current < 0 ? 0 : current;
    const next = (base + offset + options.length) % options.length;
    onChange(options[next].value);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      selectOffset(-1);
    } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      selectOffset(1);
    } else if (e.key === "Home") {
      e.preventDefault();
      onChange(options[0].value);
    } else if (e.key === "End") {
      e.preventDefault();
      onChange(options[options.length - 1].value);
    }
  }
</script>

<div class="segmented" role="radiogroup" aria-label={ariaLabel} tabindex="-1" onkeydown={onKey}>
  {#each options as o (o.value)}
    <button
      role="radio"
      aria-checked={o.value === value}
      tabindex={o.value === value ? 0 : -1}
      class={o.value === value ? "on" : ""}
      onclick={() => onChange(o.value)}
    >
      {o.label}
    </button>
  {/each}
</div>
