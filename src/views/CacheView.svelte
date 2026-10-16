<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Folder } from "lucide-svelte";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import type { CacheEntry } from "../lib/scanTypes";
  import Modal from "../components/Modal.svelte";

  let entries: CacheEntry[] = [];
  let loading = true;
  let selected = new Set<string>();
  let confirmDelete = false;

  $: allSelected = entries.length > 0 && selected.size === entries.length;
  $: maxBytes = entries.length > 0 ? Math.max(...entries.map((e) => e.sizeBytes)) : 1;
  $: selectedBytes = entries.filter((e) => selected.has(e.path)).reduce((sum, e) => sum + e.sizeBytes, 0);

  async function load() {
    loading = true;
    try {
      entries = await api.invoke<CacheEntry[]>("scan_user_cache");
      selected = new Set([...selected].filter((p) => entries.some((e) => e.path === p)));
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function toggle(path: string) {
    if (selected.has(path)) selected.delete(path);
    else selected.add(path);
    selected = selected;
  }

  function toggleAll() {
    selected = allSelected ? new Set() : new Set(entries.map((e) => e.path));
  }

  async function doDelete() {
    confirmDelete = false;
    const bytes = selectedBytes;
    const items = selected.size;
    await api.invoke("delete_cache_entries", { paths: [...selected] });
    await api.invoke("record_cleanup", { label: "overview.cache.title", bytes, items });
    selected = new Set();
    await load();
  }
</script>

<div class="grid grid-cols-1 lg:grid-cols-[1fr_280px] gap-5 items-start">
  <div class="border border-border rounded-lg bg-surface flex flex-col">
    {#if loading}
      <p class="text-[0.85rem] text-text-muted p-5">…</p>
    {:else if entries.length === 0}
      <p class="text-[0.85rem] text-text-muted p-5">{$_("cacheView.empty")}</p>
    {:else}
      <label class="flex items-center gap-3 px-4 py-3 border-b border-border cursor-pointer hover:bg-surface-2 text-[0.8rem] text-text-muted">
        <input type="checkbox" class="w-4 h-4 accent-primary shrink-0" checked={allSelected} on:change={toggleAll} />
        {$_("common.selectAll")}
        <span class="ml-auto text-[0.74rem]">{$_("overview.cache.description", { values: { count: entries.length } })}</span>
      </label>
      <div class="divide-y divide-border max-h-[50vh] overflow-y-auto content-scroll">
        {#each entries as entry (entry.path)}
          <label
            class="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-surface-2 {selected.has(entry.path) ? 'bg-primary/5' : ''}"
          >
            <input type="checkbox" class="w-4 h-4 accent-primary shrink-0" checked={selected.has(entry.path)} on:change={() => toggle(entry.path)} />
            <span class="w-8 h-8 rounded-lg bg-surface-2 text-text-muted grid place-items-center shrink-0">
              <Folder size={15} />
            </span>
            <span class="flex-1 min-w-0">
              <span class="block truncate text-[0.85rem] text-text-bright">{entry.name}</span>
              <span class="block h-1 rounded-full bg-surface-2 overflow-hidden mt-1.5 max-w-[180px]">
                <span class="block h-full rounded-full bg-primary" style="width: {(entry.sizeBytes / maxBytes) * 100}%"></span>
              </span>
            </span>
            <span class="text-[0.78rem] text-text-muted shrink-0">{formatBytes(entry.sizeBytes)}</span>
          </label>
        {/each}
      </div>
      <div class="flex items-center justify-between px-4 py-3 border-t border-border">
        <span class="text-[0.78rem] text-text-muted">
          {$_("overview.cache.selectedSummary", { values: { count: selected.size, size: formatBytes(selectedBytes) } })}
        </span>
        <button
          class="px-4 py-1.5 rounded text-[0.78rem] font-semibold border border-danger text-danger hover:bg-danger/10 disabled:opacity-50 disabled:cursor-not-allowed"
          disabled={selected.size === 0}
          on:click={() => (confirmDelete = true)}
        >
          {$_("common.delete")}
        </button>
      </div>
    {/if}
  </div>

  <div class="flex flex-col gap-3">
    <div class="border border-border rounded-lg bg-surface p-4">
      <p class="text-[0.7rem] font-semibold text-text-muted mb-2 uppercase tracking-wide">{$_("common.about")}</p>
      <p class="text-[0.78rem] text-text-muted">{$_("cacheView.context")}</p>
    </div>
    <div class="border border-border rounded-lg bg-surface p-4">
      <p class="text-[0.7rem] font-semibold text-text-muted mb-2 uppercase tracking-wide">{$_("cacheView.preservedTitle")}</p>
      <p class="text-[0.78rem] text-text-muted">{$_("cacheView.preserved")}</p>
    </div>
    <div class="border border-border rounded-lg bg-surface p-4">
      <p class="text-[0.7rem] font-semibold text-text-muted mb-2 uppercase tracking-wide">{$_("cacheView.equivalentTitle")}</p>
      <code class="block text-[0.72rem] font-mono bg-surface-2 rounded px-2.5 py-2 text-text-bright break-all">rm -rf ~/.cache/&lt;folder&gt;</code>
    </div>
  </div>
</div>

{#if confirmDelete}
  <Modal
    variant="danger"
    title={$_("overview.cache.confirmTitle")}
    confirmLabel={$_("common.delete")}
    onClose={() => (confirmDelete = false)}
    onConfirm={doDelete}
  >
    <p class="text-text-muted">
      {$_("overview.cache.confirmBody", { values: { count: selected.size, size: formatBytes(selectedBytes) } })}
    </p>
  </Modal>
{/if}
