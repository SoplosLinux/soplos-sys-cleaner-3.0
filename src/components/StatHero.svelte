<script lang="ts">
  import type { ComponentType } from "svelte";
  import { formatBytes } from "../lib/format";

  // Full-weight single-stat page — Trash, APT cache and journald have
  // nothing to list (just a size and one action), so they don't get a
  // CleanCard floating alone in an otherwise empty page; this fills the
  // space on purpose instead, like a hero panel.
  export let icon: ComponentType;
  export let title: string;
  export let description: string;
  export let sizeBytes: number | null = null;
  export let loading = false;
  export let actionLabel: string;
  export let actionDisabled = false;
  export let onAction: () => void;
  export let danger = false;
</script>

<div class="border border-border rounded-xl bg-surface p-7 flex items-center gap-6">
  <div class="w-16 h-16 rounded-2xl bg-primary/15 text-primary grid place-items-center shrink-0">
    <svelte:component this={icon} size={30} />
  </div>

  <div class="flex-1 min-w-0">
    <p class="text-[1.05rem] font-semibold text-text-bright">{title}</p>
    <p class="text-[0.8rem] text-text-muted mt-0.5">{description}</p>
  </div>

  <p class="text-[2.2rem] font-bold text-text-bright shrink-0 px-4">
    {#if loading}
      <span class="text-[1.1rem] font-normal text-text-muted">…</span>
    {:else if sizeBytes === null}
      <span class="text-[1.1rem] font-normal text-text-muted">—</span>
    {:else}
      {formatBytes(sizeBytes)}
    {/if}
  </p>

  <button
    class="shrink-0 px-5 py-2.5 rounded-lg text-[0.85rem] font-semibold disabled:opacity-50 disabled:cursor-not-allowed {danger
      ? 'border border-danger text-danger hover:bg-danger/10'
      : 'bg-primary hover:bg-primary-hover text-text-bright'}"
    disabled={actionDisabled || loading}
    on:click={onAction}
  >
    {actionLabel}
  </button>
</div>
