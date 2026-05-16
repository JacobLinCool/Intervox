<script lang="ts">
  import type { Snippet } from "svelte";
  import { css } from "$lib/util";
  import { Check } from "$lib/icons";

  let {
    value,
    onChange,
    options,
    width,
    optionLeft,
    optionRight,
  }: {
    value: any;
    onChange: (v: any) => void;
    options: { value: any; label: string }[];
    width?: number;
    optionLeft?: Snippet<[any]>;
    optionRight?: Snippet<[any]>;
  } = $props();

  let open = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();

  let cur = $derived(options.find((o) => o.value === value) ?? options[0]);

  $effect(() => {
    const fn = (e: MouseEvent) => {
      if (!containerEl?.contains(e.target as Node)) open = false;
    };
    document.addEventListener("mousedown", fn);
    return () => document.removeEventListener("mousedown", fn);
  });
</script>

<div
  bind:this={containerEl}
  style={css({ position: "relative", width: width ?? 230 })}
>
  <button
    class="btn"
    onclick={() => (open = !open)}
    style={css({
      width: "100%",
      display: "flex",
      alignItems: "center",
      gap: 6,
      padding: "5px 8px 5px 10px",
      background: "var(--control-bg)",
      border: "0.5px solid var(--control-border)",
      fontSize: 12.5,
      justifyContent: "flex-start",
    })}
  >
    {#if optionLeft}
      {@render optionLeft(cur)}
    {/if}
    <span
      style={css({
        flex: 1,
        textAlign: "left",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      })}
    >
      {cur?.label ?? ""}
    </span>
    <span
      style={css({
        width: 16,
        height: 16,
        borderRadius: 4,
        background: "var(--c-mixed)",
        display: "grid",
        placeItems: "center",
        color: "#fff",
      })}
    >
      <svg width="10" height="10" viewBox="0 0 10 10"
        ><path
          d="M2 3.5 5 1l3 2.5M2 6.5 5 9l3-2.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        /></svg
      >
    </span>
  </button>

  {#if open}
    <div
      role="listbox"
      style={css({
        position: "absolute",
        top: "calc(100% + 4px)",
        left: 0,
        width: "100%",
        background: "var(--win-bg)",
        backdropFilter: "saturate(180%) blur(40px)",
        border: "0.5px solid var(--win-border)",
        borderRadius: 8,
        boxShadow:
          "0 14px 30px rgba(0,0,0,0.20), 0 0 0 0.5px rgba(0,0,0,0.06)",
        padding: 4,
        zIndex: 80,
        animation: "pop-in 100ms ease-out both",
        maxHeight: 280,
        overflowY: "auto",
      })}
    >
      {#each options as o (o.value)}
        <div
          role="option"
          tabindex="0"
          aria-selected={o.value === value}
          onclick={() => {
            onChange(o.value);
            open = false;
          }}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              onChange(o.value);
              open = false;
            }
          }}
          onmouseenter={(e) => {
            (e.currentTarget as HTMLElement).style.background = "var(--c-mixed)";
            (e.currentTarget as HTMLElement).style.color = "#fff";
          }}
          onmouseleave={(e) => {
            (e.currentTarget as HTMLElement).style.background = "transparent";
            (e.currentTarget as HTMLElement).style.color = "var(--txt-1)";
          }}
          style={css({
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "5px 8px",
            borderRadius: 5,
            fontSize: 12.5,
            cursor: "default",
          })}
        >
          <span
            style={css({
              width: 14,
              display: "grid",
              placeItems: "center",
            })}
          >
            {#if o.value === value}
              <Check size={10} color="currentColor" />
            {/if}
          </span>
          {#if optionLeft}
            {@render optionLeft(o)}
          {/if}
          <span style={css({ flex: 1 })}>{o.label}</span>
          {#if optionRight}
            {@render optionRight(o)}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
