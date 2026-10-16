<script lang="ts">
  import type { ComponentType } from "svelte";
  import { formatBytes } from "../lib/format";

  // The Kudu-style building block: one card per cleaning category, showing
  // what was found and a single primary action. Categories with more than
  // one action (like User Cache's per-app selection) put extra controls in
  // the default slot below the summary line instead of inventing a second
  // card shape.
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

<div class="border border-border rounded-lg p-4 bg-surface flex flex-col gap-3">
  <div class="flex items-start gap-3">
    <div class="w-10 h-10 rounded-lg bg-primary/15 text-primary grid place-items-center shrink-0">
      <svelte:component this={icon} size={20} />
    </div>
    <div class="min-w-0 flex-1">
      <p class="text-[0.9rem] font-semibold text-text-bright">{title}</p>
      <p class="text-[0.72rem] text-text-muted mt-0.5">{description}</p>
    </div>
  </div>

  <div class="flex items-center justify-between mt-1">
    <span class="text-[1.15rem] font-bold text-text-bright">
      {#if loading}
        <span class="text-[0.85rem] font-normal text-text-muted">…</span>
      {:else if sizeBytes === null}
        <span class="text-[0.85rem] font-normal text-text-muted">—</span>
      {:else}
        {formatBytes(sizeBytes)}
      {/if}
    </span>
    <button
      class="px-3 py-1.5 rounded text-[0.78rem] font-semibold disabled:opacity-50 disabled:cursor-not-allowed {danger
        ? 'border border-danger text-danger hover:bg-danger/10'
        : 'bg-primary hover:bg-primary-hover text-text-bright'}"
      disabled={actionDisabled || loading}
      on:click={onAction}
    >
      {actionLabel}
    </button>
  </div>

  <slot />
</div>
