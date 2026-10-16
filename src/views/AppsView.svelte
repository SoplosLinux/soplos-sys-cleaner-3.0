<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { api } from "../lib/api";
  import { runJob } from "../lib/job";
  import { formatBytes } from "../lib/format";
  import type { AppInfo } from "../lib/scanTypes";
  import Modal from "../components/Modal.svelte";

  let apps: AppInfo[] = [];
  let loading = true;
  let busy = false;
  let selected = new Set<string>();
  let confirmDelete = false;

  $: allSelected = apps.length > 0 && selected.size === apps.length;
  $: selectedBytes = apps.filter((a) => selected.has(a.name)).reduce((sum, a) => sum + a.sizeBytes, 0);

  async function load() {
    loading = true;
    try {
      apps = await api.invoke<AppInfo[]>("scan_installed_apps");
      selected = new Set([...selected].filter((n) => apps.some((a) => a.name === n)));
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function toggle(name: string) {
    if (selected.has(name)) selected.delete(name);
    else selected.add(name);
    selected = selected;
  }

  function toggleAll() {
    selected = allSelected ? new Set() : new Set(apps.map((a) => a.name));
  }

  async function doDelete() {
    confirmDelete = false;
    busy = true;
    const bytes = selectedBytes;
    const items = selected.size;
    try {
      const success = await runJob("remove_installed_apps", { names: [...selected] });
      if (success) {
        await api.invoke("record_cleanup", { label: "overview.apps.title", bytes, items });
      }
      selected = new Set();
      await load();
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col h-full">
  {#if loading}
    <p class="text-[0.85rem] text-text-muted">…</p>
  {:else if apps.length === 0}
    <p class="text-[0.85rem] text-text-muted">{$_("appsView.empty")}</p>
  {:else}
    <div class="border border-border rounded-lg bg-surface flex-1 overflow-y-auto pb-24 content-scroll">
      <label
        class="flex items-center gap-3 px-4 py-3 border-b border-border cursor-pointer hover:bg-surface-2 text-[0.8rem] text-text-muted sticky top-0 bg-surface"
      >
        <input type="checkbox" class="w-4 h-4 accent-primary shrink-0" checked={allSelected} on:change={toggleAll} disabled={busy} />
        {$_("common.selectAll")}
      </label>
      <div class="divide-y divide-border">
        {#each apps as app (app.name)}
          <label class="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-surface-2 {selected.has(app.name) ? 'bg-primary/5' : ''}">
            <input type="checkbox" class="w-4 h-4 accent-primary shrink-0" checked={selected.has(app.name)} on:change={() => toggle(app.name)} disabled={busy} />
            <span class="flex-1 min-w-0">
              <span class="block truncate text-[0.85rem] text-text-bright">{app.name}</span>
              {#if app.summary}
                <span class="block truncate text-[0.75rem] text-text-muted">{app.summary}</span>
              {/if}
            </span>
            <span class="text-[0.78rem] text-text-muted shrink-0">{formatBytes(app.sizeBytes)}</span>
          </label>
        {/each}
      </div>
    </div>

    <div class="fixed bottom-0 left-[230px] right-0 flex items-center justify-between px-7 py-3 bg-panel border-t border-border">
      <span class="text-[0.82rem] text-text-muted">
        {$_("overview.apps.selectedSummary", { values: { count: selected.size, size: formatBytes(selectedBytes) } })}
      </span>
      <button
        class="px-4 py-2 rounded text-[0.82rem] font-semibold border border-danger text-danger hover:bg-danger/10 disabled:opacity-50 disabled:cursor-not-allowed"
        disabled={selected.size === 0 || busy}
        on:click={() => (confirmDelete = true)}
      >
        {$_("common.delete")}
      </button>
    </div>
  {/if}
</div>

{#if confirmDelete}
  <Modal
    variant="danger"
    title={$_("overview.apps.confirmTitle")}
    confirmLabel={$_("common.delete")}
    onClose={() => (confirmDelete = false)}
    onConfirm={doDelete}
  >
    <p class="text-text-muted">
      {$_("overview.apps.confirmBody", { values: { count: selected.size, size: formatBytes(selectedBytes) } })}
    </p>
  </Modal>
{/if}
