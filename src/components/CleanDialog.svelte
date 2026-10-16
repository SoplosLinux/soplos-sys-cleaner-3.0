<script lang="ts">
  import { onMount } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import { cleanGroups, type CleanResult } from "../lib/clean";
  import { journalPolicy } from "../lib/journal";
  import { categoryById, commandFor, bytesOf, itemsOf, cellText, type CategoryId, type Row } from "../lib/categories";
  import type { CleanKind, Simulation } from "../lib/scanTypes";
  import Modal from "./Modal.svelte";

  // One confirmation + progress dialog for every cleanup: a single category
  // from its page, or several at once from the Summary page. All the
  // categories that need privileges run in one batch, so the password is
  // asked once.
  export let name: string;
  export let kind: CleanKind = "manual";
  export let groups: { id: CategoryId; rows: Row[] }[];
  export let onClose: () => void;
  export let onDone: (results: CleanResult[]) => void;
  // The basic program shows a short summary; the list of items and the
  // commands stay one click away.
  export let simple = false;

  let busy = false;
  let showDetails = !simple;

  // Dry run of the package removals (kernels and drivers are purged, apps are
  // removed). The cleanup cannot start until it is shown, and it is blocked
  // when it would also remove something the system needs.
  interface SimState {
    loading: boolean;
    removes: string[];
    blocked: string | null;
    error: string | null;
  }
  let sim: SimState | null = null;

  async function simulate() {
    const purge = groups.filter((g) => g.id === "kernels" || g.id === "drivers").flatMap((g) => g.rows.flatMap((r) => r.packages ?? []));
    const remove = groups.filter((g) => g.id === "apps" || g.id === "orphans").flatMap((g) => g.rows.flatMap((r) => r.packages ?? []));
    if (purge.length === 0 && remove.length === 0) return;
    sim = { loading: true, removes: [], blocked: null, error: null };
    const results: Simulation[] = [];
    try {
      if (purge.length > 0) results.push(await api.invoke<Simulation>("simulate_removal", { packages: purge, purge: true }));
      if (remove.length > 0) results.push(await api.invoke<Simulation>("simulate_removal", { packages: remove, purge: false }));
    } catch (error) {
      results.push({ removes: [], blocked: null, error: String(error) });
    }
    sim = {
      loading: false,
      removes: [...new Set(results.flatMap((r) => r.removes))],
      blocked: results.find((r) => r.blocked)?.blocked ?? null,
      error: results.find((r) => r.error)?.error ?? null,
    };
  }

  onMount(simulate);

  $: canClean = !sim || (!sim.loading && !sim.blocked && !sim.error);

  $: allRows = groups.flatMap((g) => g.rows);
  $: totalBytes = bytesOf(allRows);
  $: totalItems = itemsOf(allRows);
  $: needsRoot = groups.some((g) => categoryById(g.id).admin);

  function requestClose() {
    if (!busy) onClose();
  }

  async function start() {
    busy = true;
    const results = await cleanGroups(groups, kind);
    onDone(results);
    onClose();
  }
</script>

<Modal variant="danger" title={name} maxWidth={540} showFooter={false} onClose={requestClose}>
  {#if !busy}
    {#if simple}
      <p class="mb-2 text-text-bright">
        {$_("basic.confirmBody", { values: { size: formatBytes(totalBytes), count: totalItems } })}
      </p>
      <button class="mb-2 text-[11px] text-primary hover:underline" on:click={() => (showDetails = !showDetails)}>
        {showDetails ? $_("basic.hideDetails") : $_("basic.showDetails")}
      </button>
    {:else}
      <p class="mb-2 text-text-bright">
        {$_("dashboard.itemsCount", { values: { count: totalItems } })} · <b>{formatBytes(totalBytes)}</b> ·
        {$_("page.categoriesCount", { values: { count: groups.length } })}
      </p>
    {/if}

    {#if showDetails}
      <div class="max-h-[180px] overflow-auto rounded border border-border content-scroll">
        {#each groups as group (group.id)}
          {#each group.rows as row (group.id + row.id)}
            <div class="flex gap-2 border-b border-border px-2 py-[3px] text-[12px] last:border-b-0">
              <span class="w-[120px] shrink-0 truncate text-text-muted">{$_(categoryById(group.id).labelKey)}</span>
              <span class="min-w-0 flex-1 truncate text-text-bright">{cellText(row.name, $_, $locale)}</span>
              <b class="shrink-0 text-text-bright">{formatBytes(row.sizeBytes)}</b>
            </div>
          {/each}
        {/each}
      </div>

      <div class="mt-2 rounded bg-surface-2 px-2 py-1.5 font-mono text-[11px] break-all text-text-bright">
        {#each groups as group (group.id)}
          <div>$ {commandFor(categoryById(group.id), $journalPolicy)}</div>
        {/each}
      </div>
    {/if}

    {#if sim}
      <div class="mt-2 rounded border border-border px-2 py-1.5">
        {#if sim.loading}
          <p class="text-[11px] text-text-muted">{$_("simulation.simulating")}</p>
        {:else}
          <p class="text-[11px] font-semibold text-text-bright">{$_("simulation.packages", { values: { count: sim.removes.length } })}</p>
          {#if sim.removes.length > 0}
            <div class="mt-1 max-h-[100px] overflow-auto font-mono text-[11px] text-text content-scroll">
              {#each sim.removes as name (name)}
                <div class="truncate">{name}</div>
              {/each}
            </div>
          {/if}
          {#if sim.blocked}
            <p class="mt-1 text-[11px] font-semibold text-danger">{$_("simulation.blocked", { values: { name: sim.blocked } })}</p>
          {/if}
          {#if sim.error}
            <p class="mt-1 text-[11px] font-semibold text-danger">{$_("simulation.error", { values: { error: sim.error } })}</p>
          {/if}
        {/if}
      </div>
    {/if}

    <p class="mt-2 text-[11px] text-text-muted">
      {needsRoot ? $_("page.rootNote") : $_("page.userNote")}
      {$_("page.irreversible")}
    </p>

    <div class="mt-3 flex justify-end gap-2">
      <button class="rounded border border-border bg-surface-2 px-3 py-1.5 text-text-bright hover:bg-border" on:click={requestClose}>
        {$_("common.cancel")}
      </button>
      <button
        class="rounded border border-primary bg-primary px-3 py-1.5 font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
        disabled={!canClean}
        on:click={start}
      >
        {$_("page.cleanNow")}
      </button>
    </div>
  {:else}
    <div class="h-2.5 overflow-hidden rounded bg-surface-2">
      <div class="h-full w-full animate-pulse rounded bg-primary"></div>
    </div>
    <p class="mt-2 text-text-muted">{$_("page.cleaning")}</p>
  {/if}
</Modal>
