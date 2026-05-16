<script lang="ts">
  let {
    value,
    options,
    onChange,
  }: {
    value: any;
    options: { value: any; label: string }[];
    onChange: (v: any) => void;
  } = $props();
</script>

<div class="segmented">
  {#each options as o (o.value)}
    <button
      class={o.value === value ? "on" : ""}
      onclick={() => onChange(o.value)}
    >
      {o.label}
    </button>
  {/each}
</div>
