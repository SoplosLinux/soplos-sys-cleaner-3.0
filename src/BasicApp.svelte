<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getVersion } from "@tauri-apps/api/app";
  import { _, locale } from "svelte-i18n";
  import { RefreshCw, Sun, Moon, Globe, ExternalLink, ChevronDown } from "lucide-svelte";
  import { api } from "./lib/api";
  import { envLabel, cleanVersion, type EnvInfo } from "./lib/env";
  import { CATEGORIES, bytesOf, itemsOf } from "./lib/categories";
  import { scan, stats, refresh, refreshOne, loadStats } from "./lib/scan";
  import type { CleanResult } from "./lib/clean";
  import { healthScore } from "./lib/health";
  import { formatBytes, formatRelativeTime } from "./lib/format";
  import { toast, modalDepth } from "./lib/ui";
  import { theme, toggleTheme } from "./lib/theme";
  import { SUPPORTED_LOCALES, LOCALE_NAMES, setLocale, type SupportedLocale } from "./i18n";
  import HealthRing from "./components/HealthRing.svelte";
  import CleanDialog from "./components/CleanDialog.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import Toast from "./components/Toast.svelte";

  // The basic program: the health ring and one standard cleanup. The standard
  // cleanup is the safe part of the full one (one password at most) and never
  // touches anything marked "review". It records in the same history as the
  // advanced program.
  let dialog = false;
  let aboutOpen = false;
  let shortcut = false;
  let menuOpen = false;
  // Desktop, display protocol and version, as in the advanced program's footer.
  let version = "";
  let envInfo: EnvInfo | null = null;
  $: footerLabel = [envLabel(envInfo), version].filter(Boolean).join(" · ") || "Soplos Sys Cleaner";

  // Safe rows of every category, as the advanced program's full cleanup.
  $: groups = CATEGORIES.map((def) => ({ id: def.id, rows: $scan.rows[def.id].filter((r) => r.risk === "safe") })).filter((g) => g.rows.length > 0);
  $: rows = groups.flatMap((g) => g.rows);
  $: totalBytes = bytesOf(rows);
  $: score = healthScore(totalBytes);
  $: lastClean = ($stats?.activity ?? []).find((e) => e.ok) ?? null;
  // The cleanup is running (the dialog is open) or a scan is: no buttons.
  $: busy = dialog || $scan.loading;

  onMount(() => {
    refresh();
    loadStats();
    getVersion().then((v) => (version = cleanVersion(v))).catch(() => {});
    api.invoke<EnvInfo>("get_environment_info").then((info) => (envInfo = info)).catch(() => {});
    api.invoke<boolean>("advanced_shortcut_exists").then((exists) => (shortcut = exists)).catch(() => {});
    let unlisten: (() => void) | undefined;
    // Another version of the program was started while this one is open.
    listen("another-instance", () => toast($_("basic.otherVersion"))).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  });

  async function onDone(results: CleanResult[]) {
    const freed = results.filter((r) => r.ok).reduce((sum, r) => sum + r.freedBytes, 0);
    const itemsDone = itemsOf(groups.filter((g) => results.find((r) => r.id === g.id)?.ok).flatMap((g) => g.rows));
    await Promise.all(results.map((r) => refreshOne(r.id)));
    await loadStats();
    if (results.every((r) => r.ok)) {
      toast($_("page.cleaned", { values: { size: formatBytes(freed), count: itemsDone } }));
    } else {
      const failed = results.filter((r) => !r.ok).map((r) => $_(CATEGORIES.find((c) => c.id === r.id)!.titleKey));
      toast($_("page.failed", { values: { name: failed.join(", ") } }));
    }
  }

  async function openAdvanced() {
    try {
      await api.invoke("open_advanced");
    } catch {
      toast($_("basic.advancedMissing"));
    }
  }

  async function toggleShortcut() {
    menuOpen = false;
    const next = !shortcut;
    try {
      await api.invoke("set_advanced_shortcut", { enabled: next });
      shortcut = next;
      toast($_(next ? "basic.shortcutAdded" : "basic.shortcutRemoved"));
    } catch (error) {
      console.error("Failed to change the launcher", error);
    }
  }

  function onLanguageChange(e: Event) {
    setLocale((e.currentTarget as HTMLSelectElement).value as SupportedLocale);
  }

  // F1 -> About, Ctrl+Q -> quit (same shortcuts as the other Soplos 3.0 apps).
  // Esc closes the menu. Nothing quits or opens while a cleanup is running.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      menuOpen = false;
      return;
    }
    // Quiet while any dialog is open (a cleanup in progress included).
    if ($modalDepth > 0) return;
    const key = e.key.toLowerCase();
    if (e.key === "F1") {
      e.preventDefault();
      aboutOpen = true;
    } else if (e.key === "F5" || (e.ctrlKey && key === "r")) {
      e.preventDefault();
      if (!busy) refresh();
    } else if (e.ctrlKey && key === "q") {
      e.preventDefault();
      getCurrentWindow().close();
    }
  }
</script>

<svelte:window on:click={() => (menuOpen = false)} on:keydown={onKeydown} />

<div class="flex h-screen flex-col overflow-hidden">
  <div class="shrink-0 px-4 pt-3">
    <div
      class="relative flex min-h-[100px] items-end gap-3 overflow-hidden rounded border border-border p-4"
      style="background: linear-gradient(rgba(20, 20, 22, 0.25), rgba(20, 20, 22, 0.82)), url('/assets/hero/sys-cleaner-background.jpg'); background-size: cover; background-position: center;"
    >
      <div class="min-w-0">
        <p class="text-[15px] font-semibold text-white">Soplos Sys Cleaner</p>
        <p class="mt-0.5 text-[12px] text-white/70">{$_("basic.subtitle")}</p>
      </div>
      <button
        class="ml-auto flex shrink-0 items-center gap-1.5 rounded-md border border-white/25 bg-black/45 px-[9px] py-1 text-white hover:bg-black/60 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={busy}
        title="{$_('shell.rescan')} (F5)"
        on:click={() => refresh()}
      >
        <RefreshCw size={13} class={$scan.loading ? "animate-spin" : ""} />
        {$_("shell.rescan")}
      </button>
    </div>
  </div>

  <main class="content-scroll flex flex-1 items-center justify-center gap-8 overflow-y-auto p-4">
    <HealthRing {score} loading={$scan.loading} size={200} thickness={5} radius={46} label={$_("dashboard.health")} />

    <div class="flex min-w-0 flex-col items-start gap-2.5">
      <div>
        {#if $scan.loading && rows.length === 0}
          <p class="text-[15px] font-semibold text-text-bright">{$_("page.scanning")}</p>
        {:else if rows.length === 0}
          <p class="text-[15px] font-semibold text-text-bright">{$_("basic.nothing")}</p>
        {:else}
          <p class="text-[15px] font-semibold text-text-bright">{$_("basic.ready", { values: { size: formatBytes(totalBytes) } })}</p>
          <p class="mt-0.5 text-[11px] text-text-muted">{$_("basic.items", { values: { count: itemsOf(rows), categories: groups.length } })}</p>
        {/if}
      </div>

      <button
        class="rounded-lg border border-primary bg-primary px-8 py-2.5 text-[14px] font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
        disabled={busy || rows.length === 0}
        on:click={() => (dialog = true)}
      >
        {$_("basic.clean")}
      </button>

      <p class="text-[11px] text-text-muted">
        {#if lastClean}
          {$_("basic.lastClean", { values: { when: formatRelativeTime(lastClean.atUnix, $locale ?? "en"), size: formatBytes(lastClean.bytes) } })}
        {:else}
          {$_("basic.neverCleaned")}
        {/if}
      </p>
    </div>
  </main>

  <footer class="shrink-0 border-t border-border bg-panel px-4 py-2.5">
    <div class="flex items-center gap-2">
      <!-- The theme button is always at the far left. -->
      <button
        class="shrink-0 rounded p-1 text-text-muted hover:bg-surface hover:text-text-bright"
        on:click={toggleTheme}
        title={$theme === "dark" ? $_("sidebar.switchToLight") : $_("sidebar.switchToDark")}
      >
        <svelte:component this={$theme === "dark" ? Moon : Sun} size={14} />
      </button>
      <Globe size={13} class="shrink-0 text-text-muted" />
      <select
        class="min-w-0 appearance-none rounded border border-border bg-surface-2 px-1.5 py-1 text-[0.68rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
        value={$locale}
        title={$_("sidebar.language")}
        on:change={onLanguageChange}
      >
        {#each SUPPORTED_LOCALES as code}
          <option value={code}>{LOCALE_NAMES[code]}</option>
        {/each}
      </select>

      <button
        class="shrink-0 rounded px-1.5 py-1 text-[0.68rem] text-text-muted hover:bg-surface hover:text-text-bright"
        title="{$_('shortcuts.about')} (F1)  ·  {$_('shortcuts.rescan')} (F5)  ·  {$_('shortcuts.quit')} (Ctrl+Q)"
        on:click={() => (aboutOpen = true)}
      >
        {footerLabel}
      </button>

      <div class="relative ml-auto flex">
        <button
          class="flex items-center gap-1.5 rounded-l-md border border-border bg-surface-2 px-[9px] py-1 text-text-bright hover:bg-border disabled:cursor-not-allowed disabled:opacity-40"
          disabled={busy}
          on:click={openAdvanced}
        >
          <ExternalLink size={13} />
          {$_("basic.openAdvanced")}
        </button>
        <button
          class="rounded-r-md border border-l-0 border-border bg-surface-2 px-1.5 py-1 text-text-bright hover:bg-border disabled:cursor-not-allowed disabled:opacity-40"
          disabled={busy}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          on:click|stopPropagation={() => (menuOpen = !menuOpen)}
        >
          <ChevronDown size={13} />
        </button>
        {#if menuOpen}
          <div class="absolute right-0 bottom-full z-10 mb-1 min-w-[280px] rounded-md border border-border bg-surface py-1 shadow-lg" role="menu">
            <button
              class="block w-full px-3 py-1.5 text-left text-[12px] text-text-bright hover:bg-surface-2"
              role="menuitem"
              on:click|stopPropagation={toggleShortcut}
            >
              {shortcut ? $_("basic.shortcutRemove") : $_("basic.shortcutAdd")}
            </button>
          </div>
        {/if}
      </div>
    </div>
  </footer>
</div>

{#if dialog}
  <CleanDialog simple name={$_("basic.confirmTitle")} kind="full" {groups} onClose={() => (dialog = false)} {onDone} />
{/if}

{#if aboutOpen}
  <AboutDialog mode="basic" onClose={() => (aboutOpen = false)} />
{/if}

<Toast />
