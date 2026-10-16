<script lang="ts">
  import { _, locale } from "svelte-i18n";
  import { ChevronRight, ChevronDown } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import { lastSevenDays } from "../lib/activity";
  import { stats } from "../lib/scan";
  import { showNotes, toast, modalDepth, isTypingTarget } from "../lib/ui";
  import type { ActivityEntry, CleanKind } from "../lib/scanTypes";

  type Filter = "all" | CleanKind;

  let filter: Filter = "all";
  let expanded = new Set<string>();

  const filters: { value: Filter; label: string }[] = [
    { value: "all", label: "page.filterAll" },
    { value: "quick", label: "history.kind.quick" },
    { value: "full", label: "history.kind.full" },
    { value: "manual", label: "history.kind.manual" },
  ];

  $: activity = $stats?.activity ?? [];
  $: succeeded = activity.filter((e) => e.ok);
  $: visible = activity.filter((e) => filter === "all" || e.kind === filter);
  $: total = succeeded.reduce((s, e) => s + e.bytes, 0);
  $: largest = succeeded.reduce<ActivityEntry | null>((best, e) => (!best || e.bytes > best.bytes ? e : best), null);
  $: days = lastSevenDays(succeeded, $locale ?? undefined);
  $: maxDay = Math.max(1, ...days.map((d) => d.bytes));

  function rowKey(entry: ActivityEntry, index: number): string {
    return `${entry.atUnix}-${entry.label}-${index}`;
  }

  function toggle(key: string) {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
    expanded = expanded;
  }

  function formatDate(unix: number): string {
    return new Date(unix * 1000).toLocaleString($locale ?? undefined, {
      day: "numeric",
      month: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function kindLabel(kind: ActivityEntry["kind"]): string {
    return kind ? $_(`history.kind.${kind}`) : "—";
  }

  function formatDuration(ms: number): string {
    if (ms <= 0) return "—";
    if (ms < 1000) return `${ms} ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
    return `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;
  }

  // Ctrl+E exports the history shown in the table.
  function onKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey || e.key.toLowerCase() !== "e" || $modalDepth > 0 || isTypingTarget(e)) return;
    e.preventDefault();
    if (visible.length > 0) exportCsv();
  }

  function csvField(value: string | number): string {
    const text = String(value);
    return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  }

  function pad(n: number): string {
    return String(n).padStart(2, "0");
  }

  // Exports what the table currently shows, with translated labels. Date and
  // time are separate local-time columns (a single date-time value is too wide
  // for a default spreadsheet column and shows as ####), the duration is empty
  // when it is unknown, and a BOM makes Excel read the UTF-8 accents correctly.
  async function exportCsv() {
    const header = ["date", "time", "kind", "category", "result", "items", "bytes", "durationMs", "command"].map((k) => $_(`history.csv.${k}`));
    const lines = visible.map((e) => {
      const d = new Date(e.atUnix * 1000);
      return [
        `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
        `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`,
        e.kind ? $_(`history.kind.${e.kind}`) : "",
        $_(e.label),
        e.ok ? $_("history.ok") : $_("history.failed"),
        e.items,
        e.bytes,
        e.durationMs > 0 ? e.durationMs : "",
        e.command,
      ]
        .map(csvField)
        .join(",");
    });
    const contents = "﻿" + [header.map(csvField).join(","), ...lines].join("\n") + "\n";

    try {
      let path = await save({ defaultPath: "sys-cleaner-history.csv", filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (!path) return;
      if (!path.toLowerCase().endsWith(".csv")) path += ".csv";
      await api.invoke("export_text_file", { path, contents });
      toast($_("history.exported"));
    } catch (error) {
      console.error("Failed to export the history", error);
      toast($_("history.exportFailed"));
    }
  }
</script>

<svelte:window on:keydown={onKeydown} />

<div class="flex flex-col gap-2.5">
  <div class="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-4">
    <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.totalFreed")}</h3>
      <div class="text-[20px] font-bold text-text-bright">{formatBytes(total)}</div>
    </div>
    <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.cleanups")}</h3>
      <div class="text-[20px] font-bold text-text-bright">{succeeded.length}</div>
    </div>
    <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.average")}</h3>
      <div class="text-[20px] font-bold text-text-bright">{formatBytes(succeeded.length > 0 ? total / succeeded.length : 0)}</div>
    </div>
    <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.largest")}</h3>
      <div class="text-[20px] font-bold text-text-bright">{largest ? formatBytes(largest.bytes) : "—"}</div>
    </div>
  </div>

  <div class="grid gap-2.5 {$showNotes ? 'items-start lg:grid-cols-[1fr_290px]' : ''}">
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <div class="mb-2 flex flex-wrap items-center gap-1.5">
        {#each filters as f (f.value)}
          <button
            class="rounded-md border px-[9px] py-1 {filter === f.value ? 'border-primary text-primary' : 'border-border bg-surface-2 text-text'}"
            on:click={() => (filter = f.value)}
          >
            {$_(f.label)}
          </button>
        {/each}
        <button
          class="ml-auto rounded-md border border-border bg-surface-2 px-[9px] py-1 hover:bg-border disabled:cursor-not-allowed disabled:opacity-40"
          disabled={visible.length === 0}
          title="Ctrl+E"
          on:click={exportCsv}
        >
          {$_("history.export")}
        </button>
      </div>

      <div class="overflow-x-auto">
        <table class="w-full border-collapse">
          <thead>
            <tr class="text-[10.5px] font-semibold tracking-[0.05em] text-text-muted uppercase">
              <th class="w-5 border-b border-border px-1.5 py-1"></th>
              <th class="border-b border-border px-1.5 py-1 text-left">{$_("cols.date")}</th>
              <th class="border-b border-border px-1.5 py-1 text-left">{$_("history.colType")}</th>
              <th class="border-b border-border px-1.5 py-1 text-left">{$_("cols.category")}</th>
              <th class="border-b border-border px-1.5 py-1 text-right">{$_("cols.items")}</th>
              <th class="border-b border-border px-1.5 py-1 text-right">{$_("history.colDuration")}</th>
              <th class="border-b border-border px-1.5 py-1 text-right">{$_("cols.freed")}</th>
            </tr>
          </thead>
          <tbody>
            {#if visible.length === 0}
              <tr><td colspan="7" class="p-[18px] text-center text-text-muted">{$_("dashboard.noActivity")}</td></tr>
            {:else}
              {#each visible as entry, i (rowKey(entry, i))}
                {@const key = rowKey(entry, i)}
                <tr class="cursor-pointer hover:bg-surface-2" on:click={() => toggle(key)}>
                  <td class="border-b border-border px-1.5 py-[5px] text-text-muted">
                    <svelte:component this={expanded.has(key) ? ChevronDown : ChevronRight} size={13} />
                  </td>
                  <td class="border-b border-border px-1.5 py-[5px] whitespace-nowrap">{formatDate(entry.atUnix)}</td>
                  <td class="border-b border-border px-1.5 py-[5px] whitespace-nowrap">{kindLabel(entry.kind)}</td>
                  <td class="border-b border-border px-1.5 py-[5px] text-text-bright">
                    {$_(entry.label)}
                    {#if !entry.ok}
                      <span class="ml-1 inline-block rounded-full border border-danger/60 px-[7px] py-px text-[10.5px] text-danger">{$_("history.failed")}</span>
                    {/if}
                  </td>
                  <td class="border-b border-border px-1.5 py-[5px] text-right tabular-nums">{entry.items}</td>
                  <td class="border-b border-border px-1.5 py-[5px] text-right whitespace-nowrap tabular-nums">{formatDuration(entry.durationMs)}</td>
                  <td class="border-b border-border px-1.5 py-[5px] text-right whitespace-nowrap tabular-nums">
                    {entry.ok ? formatBytes(entry.bytes) : "—"}
                  </td>
                </tr>
                {#if expanded.has(key)}
                  <tr>
                    <td colspan="7" class="border-b border-border bg-surface-2/40 px-3 py-2">
                      {#if entry.command}
                        <div class="rounded bg-surface-2 px-2 py-1.5 font-mono text-[11px] break-all text-text-bright">$ {entry.command}</div>
                        <p class="mt-1.5 text-[11px] text-text-muted">
                          {entry.ok ? $_("history.ok") : $_("history.failedDetail")}
                        </p>
                        {#if entry.details.length > 0}
                          <div class="mt-1.5 max-h-[120px] overflow-auto rounded border border-border content-scroll">
                            {#each entry.details as detail}
                              <div class="truncate border-b border-border px-2 py-[2px] text-[11px] last:border-b-0" title={detail}>{detail}</div>
                            {/each}
                          </div>
                        {/if}
                      {:else}
                        <p class="text-[11px] text-text-muted">{$_("history.noDetail")}</p>
                      {/if}
                    </td>
                  </tr>
                {/if}
              {/each}
            {/if}
          </tbody>
        </table>
      </div>
    </div>

    {#if $showNotes}
      <div class="flex flex-col gap-2.5">
        <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
          <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.last7")}</h3>
          <div class="flex h-[92px] items-end gap-1.5">
            {#each days as day, i (i)}
              <div class="flex flex-1 flex-col items-center justify-end gap-0.5 text-[10px] text-text-muted" title={formatBytes(day.bytes)}>
                <span>{day.bytes ? formatBytes(day.bytes) : ""}</span>
                <i
                  class="block w-full rounded-t-[3px] bg-primary {day.bytes ? '' : 'opacity-25'}"
                  style="height: {day.bytes ? Math.max(3, (day.bytes / maxDay) * 58) : 2}px"
                ></i>
                <span>{day.label}</span>
              </div>
            {/each}
          </div>
        </div>
        <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
          <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("history.conceptTitle")}</h3>
          <p class="mb-1.5">{$_("history.conceptBody")}</p>
        </div>
      </div>
    {/if}
  </div>
</div>
