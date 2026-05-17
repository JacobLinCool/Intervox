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
  let buttonEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLDivElement | undefined = $state();
  let menuLeft = $state(0);
  let menuTop = $state(0);
  let menuWidth = $state(230);
  let menuMaxHeight = $state(280);
  let activeIndex = $state(0);

  let cur = $derived(options.find((o) => o.value === value) ?? options[0]);
  const activeOption = $derived(options[activeIndex] ?? cur);

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function placeMenu() {
    if (!buttonEl) return;
    const rect = buttonEl.getBoundingClientRect();
    const below = window.innerHeight - rect.bottom - 10;
    const above = rect.top - 10;
    const maxHeight = Math.min(280, Math.max(120, Math.max(below, above)));
    menuLeft = rect.left;
    menuWidth = rect.width || width || 230;
    menuMaxHeight = maxHeight;
    menuTop = below >= 160 || below >= above
      ? rect.bottom + 4
      : Math.max(10, rect.top - maxHeight - 4);
  }

  function toggleMenu() {
    if (!open) {
      activeIndex = Math.max(0, options.findIndex((o) => o.value === value));
      placeMenu();
    }
    open = !open;
  }

  function choose(v: any) {
    onChange(v);
    open = false;
    buttonEl?.focus();
  }

  function onButtonKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (!open) {
        activeIndex = Math.max(0, options.findIndex((o) => o.value === value));
        placeMenu();
        open = true;
      } else if (e.key === "ArrowDown") {
        activeIndex = (activeIndex + 1) % options.length;
      } else {
        choose(activeOption.value);
      }
    } else if (e.key === "ArrowUp" && open) {
      e.preventDefault();
      activeIndex = (activeIndex - 1 + options.length) % options.length;
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      open = false;
    }
  }

  $effect(() => {
    const fn = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!containerEl?.contains(target) && !menuEl?.contains(target)) open = false;
    };
    document.addEventListener("mousedown", fn);
    return () => document.removeEventListener("mousedown", fn);
  });

  $effect(() => {
    if (!open) return;
    placeMenu();
    const close = () => (open = false);
    const reposition = () => placeMenu();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", close, true);
    };
  });
</script>

<div
  bind:this={containerEl}
  style={css({ position: "relative", width: width ?? 230 })}
>
  <button
    bind:this={buttonEl}
    class="btn"
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={toggleMenu}
    onkeydown={onButtonKey}
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
      use:portal
      bind:this={menuEl}
      role="listbox"
      tabindex="-1"
      aria-activedescendant={`pulldown-${String(activeOption?.value ?? "")}`}
      style={css({
        position: "fixed",
        top: menuTop,
        left: menuLeft,
        width: menuWidth,
        background: "var(--win-bg)",
        backdropFilter: "saturate(180%) blur(40px)",
        border: "0.5px solid var(--win-border)",
        borderRadius: 8,
        boxShadow:
          "0 14px 30px rgba(0,0,0,0.20), 0 0 0 0.5px rgba(0,0,0,0.06)",
        padding: 4,
        zIndex: 8000,
        animation: "pop-in 100ms ease-out both",
        maxHeight: menuMaxHeight,
        overflowY: "auto",
      })}
    >
      {#each options as o (o.value)}
        <div
          id={`pulldown-${String(o.value)}`}
          role="option"
          tabindex="0"
          aria-selected={o.value === value}
          onclick={() => {
            choose(o.value);
          }}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              choose(o.value);
            }
          }}
          onmouseenter={(e) => {
            activeIndex = options.findIndex((candidate) => candidate.value === o.value);
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
