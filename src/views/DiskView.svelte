<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Folder, File as FileIcon, Link as LinkIcon, ArrowUp, ChevronRight, HardDrive, Copy, ShieldCheck, RefreshCw } from "lucide-svelte";
  import { crumbs, diskHome, listFolder, VIRTUAL_FOLDERS, type DiskEntry, type DiskListing, type DiskScope } from "../lib/disk";
  import { formatBytes } from "../lib/format";
  import { toast } from "../lib/ui";

  // Where the space goes, folder by folder. It only shows: what to delete is
  // the user's decision, made in the folders themselves, so no row here has a
  // delete button. The system scope can be read again as administrator to
  // count the folders the user cannot open.
  const scopes: { value: DiskScope; label: string }[] = [
    { value: "user", label: "disk.scopeUser" },
    { value: "system", label: "disk.scopeSystem" },
  ];
  let scope: DiskScope = "user";
  let home = "/";
  let root = "/";
  let path = "/";
  let listing: DiskListing | null = null;
  let loading = true;
  let failed = false;
  let admin = false;
  // A newer request makes an older answer that arrives late worthless.
  let request = 0;

  async function load(target: string) {
    const mine = ++request;
    loading = true;
    failed = false;
    try {
      const result = await listFolder(target, scope, admin && scope === "system");
      if (mine !== request) return;
      listing = result;
      path = result.path;
    } catch (error) {
      if (mine !== request) return;
      console.error("Reading the folder failed", error);
      failed = true;
    } finally {
      if (mine === request) loading = false;
    }
  }

  onMount(async () => {
    home = await diskHome().catch(() => "/");
    root = home;
    path = home;
    await load(home);
  });

  function setScope(next: DiskScope) {
    if (next === scope) return;
    scope = next;
    admin = false;
    root = next === "user" ? home : "/";
    listing = null;
    void load(root);
  }

  function readAsAdmin() {
    admin = true;
    void load(path);
  }

  const canOpen = (entry: DiskEntry) => entry.kind === "dir" && !VIRTUAL_FOLDERS.includes(entry.path);
  const parentOf = (value: string) => value.slice(0, value.lastIndexOf("/")) || "/";
  $: atRoot = path === root;
  $: trail = crumbs(path, root);
  $: largest = listing?.entries.reduce((max, e) => Math.max(max, e.sizeBytes), 0) ?? 0;
  $: usedShare = listing && listing.fsTotalBytes > 0 ? Math.round(((listing.fsTotalBytes - listing.fsFreeBytes) / listing.fsTotalBytes) * 100) : 0;
  $: anyUnreadable = listing?.entries.some((e) => e.unreadable) ?? false;

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
      toast($_("disk.copied"));
    } catch {
      // Without clipboard access the path is still shown above.
    }
  }

  const share = (entry: DiskEntry) => (listing && listing.totalBytes > 0 ? (entry.sizeBytes / listing.totalBytes) * 100 : 0);
  const percentText = (value: number) => (value >= 10 ? Math.round(value) : value.toFixed(1)).toString() + " %";
</script>

<div class="flex flex-col gap-3">
  <p class="text-[12px] text-text-muted">{$_("disk.readOnly")}</p>

  <div class="flex flex-wrap items-center gap-2">
    <div class="flex overflow-hidden rounded-md border border-border text-[12px]" role="group" aria-label={$_("disk.scope")}>
      {#each scopes as option (option.value)}
        <button
          class="px-3 py-1 {scope === option.value ? 'bg-primary font-semibold text-white' : 'bg-surface-2 text-text hover:bg-border'}"
          aria-pressed={scope === option.value}
          on:click={() => setScope(option.value)}
        >
          {$_(option.label)}
        </button>
      {/each}
    </div>

    {#if scope === "system"}
      {#if admin}
        <span class="flex items-center gap-1.5 text-[12px] text-text-muted"><ShieldCheck size={13} class="text-primary" />{$_("disk.adminOn")}</span>
      {:else}
        <button
          class="flex items-center gap-1.5 rounded-md border border-border bg-surface-2 px-2.5 py-1 text-[12px] text-text hover:bg-border disabled:opacity-60"
          title={$_("disk.adminHint")}
          disabled={loading}
          on:click={readAsAdmin}
        >
          <ShieldCheck size={13} />
          {$_("disk.admin")}
        </button>
      {/if}
    {/if}

    <div class="ml-auto flex items-center gap-1.5">
      <button
        class="flex items-center gap-1.5 rounded-md border border-border bg-surface-2 px-2.5 py-1 text-[12px] text-text hover:bg-border disabled:opacity-60"
        disabled={loading}
        on:click={() => load(path)}
      >
        <RefreshCw size={13} />
        {$_("disk.reload")}
      </button>
      <button class="flex items-center gap-1.5 rounded-md border border-border bg-surface-2 px-2.5 py-1 text-[12px] text-text hover:bg-border" on:click={copyPath}>
        <Copy size={13} />
        {$_("disk.copyPath")}
      </button>
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-1 text-[12px]">
    <button
      class="flex items-center gap-1 rounded border border-border bg-surface-2 px-2 py-0.5 text-text hover:bg-border disabled:opacity-40"
      disabled={atRoot || loading}
      title={$_("disk.up")}
      aria-label={$_("disk.up")}
      on:click={() => load(parentOf(path))}
    >
      <ArrowUp size={12} />
    </button>
    {#each trail as crumb, index (crumb.path)}
      {#if index > 0}<span class="text-text-muted">/</span>{/if}
      <button
        class="rounded px-1.5 py-0.5 {index === trail.length - 1 ? 'font-semibold text-text-bright' : 'text-text hover:bg-surface-2'}"
        disabled={index === trail.length - 1 || loading}
        on:click={() => load(crumb.path)}
      >
        {crumb.name}
      </button>
    {/each}
  </div>

  {#if listing && listing.fsTotalBytes > 0}
    <div class="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-surface px-3 py-2 text-[12px]">
      <HardDrive size={15} class="shrink-0 text-text-muted" />
      <div class="h-2 min-w-[120px] flex-1 overflow-hidden rounded-full bg-surface-2" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={usedShare}>
        <div class="h-full bg-primary" style="width: {usedShare}%"></div>
      </div>
      <span class="text-text">{$_("disk.free", { values: { free: formatBytes(listing.fsFreeBytes), total: formatBytes(listing.fsTotalBytes) } })}</span>
      <span class="text-text-muted">{$_("disk.folderTotal", { values: { size: formatBytes(listing.totalBytes) } })}</span>
    </div>
  {/if}

  {#if anyUnreadable}
    <p class="text-[12px] text-text-muted">{$_("disk.unreadable")}</p>
  {/if}

  <div class="overflow-x-auto rounded-lg border border-border bg-surface">
    <table class="w-full border-collapse text-[12.5px]">
      <thead>
        <tr class="text-left text-[11px] uppercase tracking-wide text-text-muted">
          <th class="border-b border-border px-3 py-1.5">{$_("disk.colName")}</th>
          <th class="hidden w-[34%] border-b border-border px-3 py-1.5 sm:table-cell">{$_("disk.colShare")}</th>
          <th class="border-b border-border px-3 py-1.5 text-right">{$_("disk.colSize")}</th>
        </tr>
      </thead>
      <tbody>
        {#if loading}
          <tr><td colspan="3" class="p-[18px] text-center text-text-muted">{$_("disk.reading")}</td></tr>
        {:else if failed}
          <tr><td colspan="3" class="p-[18px] text-center text-text-muted">{$_("disk.failed")}</td></tr>
        {:else if !listing || listing.entries.length === 0}
          <tr><td colspan="3" class="p-[18px] text-center text-text-muted">{$_("disk.empty")}</td></tr>
        {:else}
          {#each listing.entries as entry (entry.path)}
            <!-- A folder opens from anywhere on its row; the name is a button so the keyboard reaches it too -->
            <tr class="hover:bg-surface-2 {canOpen(entry) ? 'cursor-pointer' : ''}" on:click={() => canOpen(entry) && load(entry.path)}>
              <td class="border-b border-border px-3 py-1.5">
                <div class="flex items-center gap-2">
                  {#if entry.kind === "dir"}
                    <Folder size={15} class="shrink-0 text-primary" />
                  {:else if entry.kind === "link"}
                    <LinkIcon size={15} class="shrink-0 text-text-muted" />
                  {:else}
                    <FileIcon size={15} class="shrink-0 text-text-muted" />
                  {/if}
                  {#if canOpen(entry)}
                    <button class="break-all text-left font-semibold text-text-bright hover:text-primary" on:click|stopPropagation={() => load(entry.path)}>{entry.name}</button>
                  {:else}
                    <span class="break-all text-text-bright">{entry.name}</span>
                  {/if}
                  {#if entry.otherFs}
                    <span class="shrink-0 rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-muted">{$_("disk.otherFs")}</span>
                  {/if}
                  {#if entry.unreadable}
                    <span class="shrink-0 rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-muted" title={$_("disk.unreadable")}>{$_("disk.partial")}</span>
                  {/if}
                </div>
              </td>
              <td class="hidden border-b border-border px-3 py-1.5 sm:table-cell">
                <div class="flex items-center gap-2">
                  <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-2">
                    <div class="h-full bg-primary" style="width: {largest > 0 ? (entry.sizeBytes / largest) * 100 : 0}%"></div>
                  </div>
                  <span class="w-12 shrink-0 text-right tabular-nums text-text-muted">{percentText(share(entry))}</span>
                </div>
              </td>
              <td class="whitespace-nowrap border-b border-border px-3 py-1.5 text-right tabular-nums">
                <span class="inline-flex items-center gap-1.5">
                  {formatBytes(entry.sizeBytes)}
                  <ChevronRight size={13} class="shrink-0 {canOpen(entry) ? 'text-text-muted' : 'invisible'}" />
                </span>
              </td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>
</div>
