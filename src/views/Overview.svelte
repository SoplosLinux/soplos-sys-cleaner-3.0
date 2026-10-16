<script lang="ts">
  import { getContext, onMount } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { Zap, Shield } from "lucide-svelte";
  import { api } from "../lib/api";
  import { formatBytes, formatRelativeTime } from "../lib/format";
  import { CATEGORIES, bytesOf, itemsOf, type Row } from "../lib/categories";
  import type { CleanResult } from "../lib/clean";
  import { scan, stats, refreshOne, loadStats } from "../lib/scan";
  import { toast } from "../lib/ui";
  import { lastSevenDays } from "../lib/activity";
  import type { DiskInfo } from "../lib/scanTypes";
  import { healthScore } from "../lib/health";
  import CleanDialog from "../components/CleanDialog.svelte";
  import HealthRing from "../components/HealthRing.svelte";
  import SystemCard from "../components/SystemCard.svelte";

  const navigate = getContext<(id: string) => void>("navigate");

  let disks: DiskInfo[] = [];

  // The weekly bars start at zero and grow to their values once the page is
  // on screen (two frames, so the browser has drawn the zero height first).
  let barsIn = false;
  onMount(() => {
    requestAnimationFrame(() => requestAnimationFrame(() => (barsIn = true)));
  });

  onMount(async () => {
    try {
      disks = await api.invoke<DiskInfo[]>("scan_storage");
    } catch (error) {
      console.error("Failed to read storage", error);
    }
  });

  // Per-category totals, split by risk.
  $: perCategory = CATEGORIES.map((def) => {
    const rows = $scan.rows[def.id];
    return {
      def,
      safe: bytesOf(rows, "safe"),
      review: bytesOf(rows, "review"),
      safeRows: rows.filter((r) => r.risk === "safe"),
      protectedCount: rows.filter((r) => r.risk === "protected").length,
    };
  });
  $: safeTotal = perCategory.reduce((s, c) => s + c.safe, 0);
  $: reviewTotal = perCategory.reduce((s, c) => s + c.review, 0);
  $: safeItems = perCategory.reduce((s, c) => s + c.safeRows.length, 0);
  $: protectedItems = perCategory.reduce((s, c) => s + c.protectedCount, 0);
  $: maxCategory = Math.max(1, ...perCategory.map((c) => c.safe + c.review));

  // Documented in the card itself; see lib/health.ts.
  $: score = healthScore(safeTotal);

  $: days = lastSevenDays($stats?.activity ?? [], $locale ?? undefined);
  $: maxDay = Math.max(1, ...days.map((d) => d.bytes));
  $: week = ($stats?.activity ?? []).filter((e) => e.ok && e.atUnix >= Math.floor(Date.now() / 1000) - 7 * 86400);
  $: weekBytes = week.reduce((s, e) => s + e.bytes, 0);

  // Quick clean: safe rows of trash and user cache. Full clean: safe rows of
  // every category (Review rows are never part of either).
  $: quickGroups = perCategory
    .filter((c) => (c.def.id === "trash" || c.def.id === "cache") && c.safeRows.length > 0)
    .map((c) => ({ id: c.def.id, rows: c.safeRows }));
  $: fullGroups = perCategory.filter((c) => c.safeRows.length > 0).map((c) => ({ id: c.def.id, rows: c.safeRows }));
  $: quickRows = quickGroups.flatMap((g) => g.rows);
  $: fullRows = fullGroups.flatMap((g) => g.rows);

  let dialog: "quick" | "full" | null = null;

  async function onDone(results: CleanResult[]) {
    const groups = dialog === "quick" ? quickGroups : fullGroups;
    const done = groups.filter((g) => results.find((r) => r.id === g.id)?.ok);
    const rows: Row[] = done.flatMap((g) => g.rows);
    const freed = results.filter((r) => r.ok).reduce((sum, r) => sum + r.freedBytes, 0);
    await Promise.all(results.map((r) => refreshOne(r.id)));
    await loadStats();
    if (results.every((r) => r.ok)) {
      toast($_("page.cleaned", { values: { size: formatBytes(freed), count: itemsOf(rows) } }));
    } else {
      const failed = results.filter((r) => !r.ok).map((r) => $_(CATEGORIES.find((c) => c.id === r.id)!.titleKey));
      toast($_("page.failed", { values: { name: failed.join(", ") } }));
    }
  }

  // Storage after cleaning: Trash and user cache live in the home mount, the
  // rest of the safe junk lives on the root filesystem.
  $: homeMount = disks.some((d) => d.mount === "/home") ? "/home" : "/";
  $: homeFreed = (perCategory.find((c) => c.def.id === "trash")?.safe ?? 0) + (perCategory.find((c) => c.def.id === "cache")?.safe ?? 0);
  $: restFreed = safeTotal - homeFreed;

  function freedOn(mount: string): number {
    if (mount === homeMount) return homeFreed + (homeMount === "/" ? restFreed : 0);
    return mount === "/" ? restFreed : 0;
  }

  $: homeDisk = disks.find((d) => d.mount === homeMount);
  $: homePercent = homeDisk && homeDisk.totalBytes > 0 ? Math.round((homeDisk.usedBytes / homeDisk.totalBytes) * 100) : 0;
</script>

<div class="flex flex-col gap-2.5">
  <div class="grid gap-2.5 lg:grid-cols-[1fr_1fr_1.25fr]">
    <!-- Health -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.health")}</h3>
      <div class="flex items-center gap-3">
        <HealthRing score={score} loading={$scan.loading} />
        <div class="min-w-0 flex-1">
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <span>{$_("dashboard.safeToClean")}</span><b class="text-text-bright">{formatBytes(safeTotal)}</b>
          </div>
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <span>{$_("dashboard.toReview")}</span><b class="text-warning">{formatBytes(reviewTotal)}</b>
          </div>
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <span>{$_("dashboard.safeItems")}</span><b class="text-text-bright">{safeItems}</b>
          </div>
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <span>{$_("dashboard.protectedItems")}</span><b class="text-text-bright">{protectedItems}</b>
          </div>
        </div>
      </div>
      <div class="mt-1.5 text-[11px] text-text-muted">
        {$_("dashboard.healthHint", {
          values: { when: $stats?.lastScanUnix ? formatRelativeTime($stats.lastScanUnix, $locale ?? "en") : $_("dashboard.never") },
        })}
      </div>
    </div>

    <!-- Space recovered, 7 days -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.recovered7d")}</h3>
      <div class="flex items-center justify-between gap-2 py-[3px]">
        <span class="text-[20px] font-bold text-text-bright">{formatBytes(weekBytes)}</span>
        <span class="text-[11px] text-text-muted">
          {$_("dashboard.cleanupsAverage", {
            values: { count: week.length, size: formatBytes(week.length > 0 ? weekBytes / week.length : 0) },
          })}
        </span>
      </div>
      <div class="flex h-[92px] items-end gap-1.5">
        {#each days as day, i (i)}
          <div class="flex flex-1 flex-col items-center justify-end gap-0.5 text-[10px] text-text-muted" title={formatBytes(day.bytes)}>
            <span>{day.bytes ? formatBytes(day.bytes) : ""}</span>
            <i
              class="block w-full rounded-t-[3px] bg-primary transition-[height] duration-700 ease-out motion-reduce:transition-none {day.bytes ? '' : 'opacity-25'}"
              style="height: {barsIn ? (day.bytes ? Math.max(3, (day.bytes / maxDay) * 58) : 2) : 0}px; transition-delay: {i * 70}ms"
            ></i>
            <span>{day.label}</span>
          </div>
        {/each}
      </div>
    </div>

    <!-- Actions -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.actions")}</h3>
      <div class="mb-2 flex items-center gap-2.5 rounded-lg border border-border p-2">
        <Zap size={20} class="shrink-0 text-primary" />
        <div class="min-w-0 flex-1">
          <b class="block text-text-bright">{$_("dashboard.quickClean.title")}</b>
          <span class="text-[11px] text-text-muted">
            {$_("dashboard.quickDetail", { values: { count: quickRows.length, size: formatBytes(bytesOf(quickRows)) } })}
          </span>
        </div>
        <button
          class="shrink-0 rounded-md border border-primary bg-primary px-[9px] py-1 font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={quickRows.length === 0}
          on:click={() => (dialog = "quick")}
        >
          {$_("common.run")}
        </button>
      </div>
      <div class="flex items-center gap-2.5 rounded-lg border border-border p-2">
        <Shield size={20} class="shrink-0 text-primary" />
        <div class="min-w-0 flex-1">
          <b class="block text-text-bright">{$_("dashboard.fullClean.title")}</b>
          <span class="text-[11px] text-text-muted">
            {$_("dashboard.fullDetail", {
              values: { categories: fullGroups.length, count: fullRows.length, size: formatBytes(bytesOf(fullRows)) },
            })}
          </span>
        </div>
        <button
          class="shrink-0 rounded-md border border-primary bg-primary px-[9px] py-1 font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={fullRows.length === 0}
          on:click={() => (dialog = "full")}
        >
          {$_("common.run")}
        </button>
      </div>
    </div>
  </div>

  <div class="grid gap-2.5 lg:grid-cols-[1.7fr_1fr]">
    <!-- Where the space is -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.spaceByCategory")}</h3>
      <div class="overflow-x-auto">
        <table class="w-full border-collapse">
          <thead>
            <tr class="text-[10.5px] font-semibold tracking-[0.05em] text-text-muted uppercase">
              <th class="border-b border-border px-1.5 py-1 text-left">{$_("dashboard.colCategory")}</th>
              <th class="w-[30%] border-b border-border px-1.5 py-1 text-left">{$_("dashboard.colSafeReview")}</th>
              <th class="border-b border-border px-1.5 py-1 text-right">{$_("dashboard.colSafe")}</th>
              <th class="border-b border-border px-1.5 py-1 text-right">{$_("dashboard.colReview")}</th>
              <th class="border-b border-border px-1.5 py-1 text-left">{$_("dashboard.colPermissions")}</th>
            </tr>
          </thead>
          <tbody>
            {#each perCategory as c (c.def.id)}
              <tr class="cursor-pointer hover:bg-surface-2" on:click={() => navigate(c.def.id)}>
                <td class="border-b border-border px-1.5 py-[5px] whitespace-nowrap">
                  <svelte:component this={c.def.icon} size={13} class="mr-1 inline text-text-muted" />
                  {$_(c.def.labelKey)}
                </td>
                <td class="border-b border-border px-1.5 py-[5px]">
                  <div class="flex h-[7px] overflow-hidden rounded bg-surface-2">
                    <i class="block bg-primary" style="width: {(c.safe / maxCategory) * 100}%"></i>
                    <u class="block bg-warning no-underline" style="width: {(c.review / maxCategory) * 100}%"></u>
                  </div>
                </td>
                <td class="border-b border-border px-1.5 py-[5px] text-right whitespace-nowrap tabular-nums">{formatBytes(c.safe)}</td>
                <td class="border-b border-border px-1.5 py-[5px] text-right whitespace-nowrap text-text-muted tabular-nums">
                  {c.review ? formatBytes(c.review) : "—"}
                </td>
                <td class="border-b border-border px-1.5 py-[5px] text-[11px] whitespace-nowrap text-text-muted">
                  {c.def.admin ? $_("dashboard.needsAdmin") : $_("dashboard.noAdmin")}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>

    <!-- Storage after cleaning -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.storageAfter")}</h3>
      {#each disks as disk (disk.mount)}
        {@const freed = Math.min(freedOn(disk.mount), disk.usedBytes)}
        <div class="mb-2">
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <b class="text-text-bright">{disk.mount}</b>
            <span class="text-[11px] text-text-muted">
              {formatBytes(disk.usedBytes)} / {formatBytes(disk.totalBytes)} · {disk.totalBytes > 0
                ? Math.round((disk.usedBytes / disk.totalBytes) * 100)
                : 0}%{freed > 0 ? ` · −${formatBytes(freed)}` : ""}
            </span>
          </div>
          <div class="flex h-[7px] overflow-hidden rounded bg-surface-2">
            <i class="block bg-primary/55" style="width: {disk.totalBytes > 0 ? ((disk.usedBytes - freed) / disk.totalBytes) * 100 : 0}%"></i>
            <u
              class="block no-underline"
              style="width: {disk.totalBytes > 0 ? (freed / disk.totalBytes) * 100 : 0}%; background: repeating-linear-gradient(45deg, #ff7d2e 0 3px, transparent 3px 6px)"
            ></u>
          </div>
        </div>
      {/each}
      <div class="text-[11px] text-text-muted">
        {$_("dashboard.storageHint")}
        {#if homeDisk && homePercent > 85}
          {$_("dashboard.homeWarning", { values: { mount: homeMount, percent: homePercent, size: formatBytes(homeFreed) } })}
        {/if}
      </div>
    </div>
  </div>

  <div class="grid gap-2.5 lg:grid-cols-[1fr_1.4fr]">
    <!-- Recent activity -->
    <div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
      <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("dashboard.recentActivity")}</h3>
      {#if !$stats || $stats.activity.length === 0}
        <p class="text-text-muted">{$_("dashboard.noActivity")}</p>
      {:else}
        {#each $stats.activity.slice(0, 5) as entry, i (entry.atUnix + "-" + i)}
          <div class="flex items-center justify-between gap-2 py-[3px]">
            <span class="min-w-0 truncate">
              <b class="text-text-bright">{$_(entry.label)}</b> · {$_("dashboard.itemsCount", { values: { count: entry.items } })}
              {#if !entry.ok}<span class="text-danger"> · {$_("history.failed")}</span>{/if}
            </span>
            <span class="shrink-0 text-[11px] text-text-muted">
              {entry.bytes > 0 ? `${formatBytes(entry.bytes)} · ` : ""}{formatRelativeTime(entry.atUnix, $locale ?? "en")}
            </span>
          </div>
        {/each}
        <button class="mt-1.5 rounded-md border border-border bg-surface-2 px-[9px] py-1 hover:bg-border" on:click={() => navigate("history")}>
          {$_("dashboard.viewAllActivity")} →
        </button>
      {/if}
    </div>

    <SystemCard />
  </div>
</div>

{#if dialog}
  <CleanDialog
    name={dialog === "quick" ? $_("dashboard.quickClean.title") : $_("dashboard.fullClean.title")}
    kind={dialog === "quick" ? "quick" : "full"}
    groups={dialog === "quick" ? quickGroups : fullGroups}
    onClose={() => (dialog = null)}
    {onDone}
  />
{/if}
