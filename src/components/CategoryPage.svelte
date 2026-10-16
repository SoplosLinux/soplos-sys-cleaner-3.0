<script lang="ts">
  import { _, locale } from "svelte-i18n";
  import { Check, Search, Download, X } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { buildReinstallScript } from "../lib/reinstall";
  import { formatBytes } from "../lib/format";
  import { categoryById, commandFor, bytesOf, itemsOf, cellText, type CategoryId, type Row } from "../lib/categories";
  import { scan, refreshOne, loadStats } from "../lib/scan";
  import { journalPolicy } from "../lib/journal";
  import type { CleanResult } from "../lib/clean";
  import { showNotes, toast, modalDepth, isTypingTarget } from "../lib/ui";
  import RiskBadge from "./RiskBadge.svelte";
  import CleanDialog from "./CleanDialog.svelte";
  import JournalRetention from "./JournalRetention.svelte";

  // The single page template every cleaning category uses: filter chips,
  // selectable table with risk labels, selection footer, and the notes panel.
  export let id: CategoryId;

  type Filter = "all" | "safe" | "review";

  let filter: Filter = "all";
  let selected = new Set<string>();
  let seenRows: Row[] | null = null;
  let dialog = false;

  $: def = categoryById(id);
  $: rows = $scan.rows[id];
  // A new scan result resets the selection to the safe rows.
  $: if (rows !== seenRows) {
    seenRows = rows;
    selected = new Set(rows.filter((r) => r.risk === "safe").map((r) => r.id));
  }
  $: selectable = rows.filter((r) => r.risk !== "protected");
  $: needle = query.trim().toLowerCase();
  $: filtered = rows.filter((r) => (filter === "all" || r.risk === filter) && matches(r, needle));
  // The locale is read here so the text order follows a language switch.
  $: visible = sortRows(filtered, sortKey, sortDir, $locale);
  $: selectedRows = rows.filter((r) => selected.has(r.id) && r.risk !== "protected");
  $: selectedBytes = bytesOf(selectedRows);
  $: allChecked = selectable.length > 0 && selectable.every((r) => selected.has(r.id));
  $: safeCount = rows.filter((r) => r.risk === "safe").length;
  $: reviewCount = rows.filter((r) => r.risk === "review").length;
  $: columns = def.headers.length;

  const filters: { value: Filter; label: string }[] = [
    { value: "all", label: "page.filterAll" },
    { value: "safe", label: "page.risk.safe" },
    { value: "review", label: "page.risk.review" },
  ];

  // Search and column sorting only change what is listed; the selection and
  // the cleanup always work on the full set of rows.
  type SortKey = "name" | "detail" | "extra" | "size" | "risk";
  const RISK_ORDER = { safe: 0, review: 1, protected: 2 };
  let query = "";
  let sortKey: SortKey | null = null;
  let sortDir: 1 | -1 = 1;

  // The text columns, in the order the table shows them.
  const TEXT_KEYS: ("name" | "detail" | "extra")[] = ["name", "detail", "extra"];

  function textOf(row: Row, key: "name" | "detail" | "extra"): string {
    return cellText(row[key], $_, $locale).toLowerCase();
  }

  function matches(row: Row, text: string): boolean {
    if (text === "") return true;
    return TEXT_KEYS.some((key) => textOf(row, key).includes(text));
  }

  function sortRows(list: Row[], key: SortKey | null, dir: 1 | -1, _locale: string | null | undefined): Row[] {
    if (key === null) return list;
    const compare = (a: Row, b: Row) => {
      if (key === "size") return a.sizeBytes - b.sizeBytes;
      if (key === "risk") return RISK_ORDER[a.risk] - RISK_ORDER[b.risk];
      return textOf(a, key).localeCompare(textOf(b, key), undefined, { numeric: true });
    };
    return [...list].sort((a, b) => compare(a, b) * dir);
  }

  function sortBy(key: SortKey) {
    if (sortKey === key) sortDir = sortDir === 1 ? -1 : 1;
    else {
      sortKey = key;
      // Sizes read best biggest first; text, A to Z.
      sortDir = key === "size" ? -1 : 1;
    }
  }

  function ariaSort(key: SortKey): "ascending" | "descending" | "none" {
    if (sortKey !== key) return "none";
    return sortDir === 1 ? "ascending" : "descending";
  }

  // The installed apps as a script that installs them again after a format.
  // Always the whole list, not only what the search shows.
  async function exportList() {
    const contents = buildReinstallScript(rows, new Date());
    try {
      let path = await save({ defaultPath: "reinstall-apps.sh", filters: [{ name: "Shell script", extensions: ["sh"] }] });
      if (!path) return;
      if (!path.toLowerCase().endsWith(".sh")) path += ".sh";
      await api.invoke("export_text_file", { path, contents });
      toast($_("page.exportListDone", { values: { count: rows.length } }));
    } catch (error) {
      console.error("Failed to export the list of apps", error);
      toast($_("page.exportListFailed"));
    }
  }

  function toggle(rowId: string) {
    if (selected.has(rowId)) selected.delete(rowId);
    else selected.add(rowId);
    selected = selected;
  }

  function toggleAll() {
    selected = allChecked ? new Set() : new Set(selectable.map((r) => r.id));
  }

  function onlySafe() {
    selected = new Set(rows.filter((r) => r.risk === "safe").map((r) => r.id));
  }

  // Ctrl+A selects all or none, Ctrl+Shift+A only the safe ones, Ctrl+Enter
  // opens the cleanup confirmation. Quiet while a dialog is open or while the
  // focus is in a text field. Enter alone never confirms anything.
  function onKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey || $modalDepth > 0 || isTypingTarget(e)) return;
    if (e.key.toLowerCase() === "a") {
      e.preventDefault();
      if (e.shiftKey) onlySafe();
      else toggleAll();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (selectedRows.length > 0 && !$scan.loading) dialog = true;
    }
  }

  async function onDone(results: CleanResult[]) {
    const bytes = results.reduce((sum, r) => sum + r.freedBytes, 0);
    const items = itemsOf(selectedRows);
    await refreshOne(id);
    await loadStats();
    if (results.every((r) => r.ok)) {
      toast($_("page.cleaned", { values: { size: formatBytes(bytes), count: items } }));
    } else {
      toast($_("page.failed", { values: { name: $_(def.titleKey) } }));
    }
  }
</script>

<svelte:window on:keydown={onKeydown} />

<div class="grid gap-2.5 {$showNotes ? 'items-start lg:grid-cols-[1fr_290px]' : ''}">
  <div class="flex min-w-0 flex-col gap-2.5">
  {#if id === "logs"}
    <JournalRetention />
  {/if}
  <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
    <!-- Stays at the top of the page while the list scrolls. The strip above
         it (before:) has the page colour, so the page padding over it never
         shows rows passing between this bar and the header. -->
    <div
      class="sticky top-0 z-10 -mx-3 -mt-2.5 mb-2 flex flex-wrap items-center gap-1.5 rounded-t-lg border-b border-border bg-surface px-3 py-2 before:absolute before:-top-4 before:-right-[calc(0.75rem+1px)] before:-left-[calc(0.75rem+1px)] before:h-4 before:bg-dark"
    >
      {#each filters as f (f.value)}
        <button
          class="rounded-md border px-[9px] py-1 {filter === f.value ? 'border-primary text-primary' : 'border-border bg-surface-2 text-text'}"
          on:click={() => (filter = f.value)}
        >
          {$_(f.label)}
        </button>
      {/each}
      <div class="relative ml-1.5 min-w-[140px] flex-1 sm:max-w-[240px]">
        <Search size={13} class="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-text-muted" />
        <input
          type="text"
          class="w-full rounded-md border border-border bg-surface-2 py-1 pr-7 pl-7 text-text-bright outline-none placeholder:text-text-muted focus-visible:ring-2 focus-visible:ring-primary/60"
          placeholder={$_("page.search")}
          aria-label={$_("page.search")}
          bind:value={query}
          on:keydown={(e) => {
            if (e.key === "Escape" && query !== "") {
              query = "";
              e.stopPropagation();
            }
          }}
        />
        {#if query !== ""}
          <button
            class="absolute top-1/2 right-1.5 flex h-4 w-4 -translate-y-1/2 items-center justify-center rounded text-text-muted hover:bg-border hover:text-text-bright"
            title={$_("page.clearSearch")}
            aria-label={$_("page.clearSearch")}
            on:click={() => (query = "")}
          >
            <X size={12} />
          </button>
        {/if}
      </div>
      <span class="ml-auto text-[11px] text-text-muted">
        {$_("page.summary", { values: { count: rows.length, safe: safeCount, review: reviewCount } })}
      </span>
    </div>

    <div class="overflow-x-auto">
      <table class="w-full border-collapse">
        <thead>
          <tr>
            <th class="w-7 border-b border-border px-1.5 py-1 text-left">
              <input
                type="checkbox"
                class="accent-primary"
                checked={allChecked}
                disabled={selectable.length === 0}
                on:change={toggleAll}
                aria-label={$_("common.selectAll")}
              />
            </th>
            {#each def.headers as header, index (header)}
              {@const key = TEXT_KEYS[index]}
              <th class="border-b border-border px-1.5 py-1 text-left" aria-sort={ariaSort(key)}>
                <button class="sort {sortKey === key ? 'text-primary' : ''}" on:click={() => sortBy(key)}>
                  {$_(header)}<span class="arrow">{sortKey === key ? (sortDir === 1 ? "▲" : "▼") : ""}</span>
                </button>
              </th>
            {/each}
            <th class="border-b border-border px-1.5 py-1 text-right" aria-sort={ariaSort("size")}>
              <button class="sort {sortKey === 'size' ? 'text-primary' : ''}" on:click={() => sortBy("size")}>
                {$_("cols.size")}<span class="arrow">{sortKey === "size" ? (sortDir === 1 ? "▲" : "▼") : ""}</span>
              </button>
            </th>
            <th class="border-b border-border px-1.5 py-1 text-left" aria-sort={ariaSort("risk")}>
              <button class="sort {sortKey === 'risk' ? 'text-primary' : ''}" on:click={() => sortBy("risk")}>
                {$_("cols.risk")}<span class="arrow">{sortKey === "risk" ? (sortDir === 1 ? "▲" : "▼") : ""}</span>
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#if $scan.loading && rows.length === 0}
            <tr><td colspan={columns + 3} class="p-[18px] text-center text-text-muted">{$_("page.scanning")}</td></tr>
          {:else if visible.length === 0}
            <tr>
              <td colspan={columns + 3} class="p-[18px] text-center text-text-muted">
                <Check size={14} class="mr-1 inline" />{$_("page.nothingHere")}
              </td>
            </tr>
          {:else}
            {#each visible as row (row.id)}
              <tr class="hover:bg-surface-2 {row.risk === 'protected' ? 'opacity-60' : ''}">
                <td class="border-b border-border px-1.5 py-[5px] align-top">
                  <input
                    type="checkbox"
                    class="accent-primary"
                    checked={selected.has(row.id)}
                    disabled={row.risk === 'protected'}
                    on:change={() => toggle(row.id)}
                  />
                </td>
                <td class="border-b border-border px-1.5 py-[5px] align-top">
                  <b class="font-semibold text-text-bright">{cellText(row.name, $_, $locale)}</b>
                  {#if row.noteKey}
                    <div class="text-[11px] font-normal text-text-muted">{$_(row.noteKey)}</div>
                  {/if}
                </td>
                {#if columns > 1}
                  <td class="border-b border-border px-1.5 py-[5px] align-top">{cellText(row.detail, $_, $locale)}</td>
                {/if}
                {#if columns > 2}
                  <td class="border-b border-border px-1.5 py-[5px] align-top">{cellText(row.extra, $_, $locale)}</td>
                {/if}
                <td class="border-b border-border px-1.5 py-[5px] text-right align-top whitespace-nowrap tabular-nums">
                  {formatBytes(row.sizeBytes)}
                </td>
                <td class="border-b border-border px-1.5 py-[5px] align-top"><RiskBadge risk={row.risk} /></td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>

    <div class="mt-2 flex flex-wrap items-center gap-2.5">
      <span>
        {$_("page.selectedOf", { values: { count: selectedRows.length, total: selectable.length } })} ·
        <b class="text-text-bright">{formatBytes(selectedBytes)}</b>
      </span>
      <div class="ml-auto flex flex-wrap gap-1.5">
        {#if id === "apps"}
          <button
            class="flex items-center gap-1.5 rounded-md border border-border bg-surface-2 px-[9px] py-1 hover:bg-border disabled:cursor-not-allowed disabled:opacity-40"
            title={$_("page.exportListHint")}
            disabled={rows.length === 0}
            on:click={exportList}
          >
            <Download size={13} />
            {$_("page.exportList")}
          </button>
        {/if}
        <button class="rounded-md border border-border bg-surface-2 px-[9px] py-1 hover:bg-border" title="Ctrl+Shift+A" on:click={onlySafe}>
          {$_("page.onlySafe")}
        </button>
        <button class="rounded-md border border-border bg-surface-2 px-[9px] py-1 hover:bg-border" on:click={() => (selected = new Set())}>
          {$_("page.selectNone")}
        </button>
        <button
          class="rounded-md border border-primary bg-primary px-[9px] py-1 font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={selectedRows.length === 0}
          title="Ctrl+Enter"
          on:click={() => (dialog = true)}
        >
          {$_("page.cleanSelection")}
        </button>
      </div>
    </div>
  </div>
  </div>

  {#if $showNotes}
    <div class="flex flex-col gap-2.5">
      <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
        <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("page.conceptTitle")}</h3>
        <p class="mb-1.5">{$_(def.conceptKey)}</p>
        <RiskBadge risk="safe" />
        <span class="ml-1 text-[11px] text-text-muted">{def.admin ? $_("page.permAdmin") : $_("page.permUser")}</span>
      </div>
      <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
        <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("page.keepsTitle")}</h3>
        <p class="text-[11px]">{$_(def.keepKey)}</p>
      </div>
      <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
        <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("page.equivalentTitle")}</h3>
        <div class="my-1 rounded bg-surface-2 px-2 py-1.5 font-mono text-[11px] break-all text-text-bright">$ {commandFor(def, $journalPolicy)}</div>
      </div>
    </div>
  {/if}
</div>

{#if dialog}
  <CleanDialog
    name={$_("page.confirmTitle", { values: { name: $_(def.titleKey) } })}
    groups={[{ id, rows: selectedRows }]}
    onClose={() => (dialog = false)}
    {onDone}
  />
{/if}

<style>
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: inherit;
    cursor: pointer;
  }
  th:not([aria-sort="none"]) .sort {
    color: rgb(var(--color-primary));
  }
  th[aria-sort="none"] .sort {
    color: rgb(var(--color-text-muted));
  }
  .sort:hover {
    color: rgb(var(--color-text-bright));
  }
  .arrow {
    display: inline-block;
    min-width: 8px;
    font-size: 8px;
  }
</style>
