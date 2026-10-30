<script lang="ts">
  import { setContext, onMount, type ComponentType } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { _ } from "svelte-i18n";
  import { Eye, RefreshCw, LayoutDashboard, History, Bell, HardDrive } from "lucide-svelte";
  import { api } from "./lib/api";
  import { CATEGORIES, bytesOf, type CategoryId } from "./lib/categories";
  import { scan, stats, refresh, loadStats } from "./lib/scan";
  import { showNotes, toast, modalDepth } from "./lib/ui";
  import { formatBytes } from "./lib/format";
  import Sidebar from "./components/Sidebar.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import CategoryPage from "./components/CategoryPage.svelte";
  import Toast from "./components/Toast.svelte";
  import Overview from "./views/Overview.svelte";
  import HistoryView from "./views/HistoryView.svelte";
  import AlertsView from "./views/AlertsView.svelte";
  import DiskView from "./views/DiskView.svelte";
  import { initAlerts } from "./lib/alerts";

  interface Tab {
    id: string;
    icon: ComponentType;
    label: string;
    title: string;
    description: string;
    group?: string;
  }

  $: tabs = [
    {
      id: "overview",
      icon: LayoutDashboard,
      label: $_("app.tabs.overview.label"),
      title: $_("app.tabs.overview.title"),
      description: $_("app.tabs.overview.description"),
    },
    ...CATEGORIES.map((c) => ({
      id: c.id,
      icon: c.icon,
      label: $_(c.labelKey),
      title: $_(c.titleKey),
      description: $_(c.descriptionKey),
      group: $_("app.groups.cleaning"),
    })),
    {
      id: "disk",
      icon: HardDrive,
      label: $_("disk.title"),
      title: $_("disk.title"),
      description: $_("disk.description"),
      group: $_("app.groups.analysis"),
    },
    {
      id: "history",
      icon: History,
      label: $_("app.tabs.history.label"),
      title: $_("app.tabs.history.title"),
      description: $_("app.tabs.history.description"),
      group: $_("app.groups.activity"),
    },
    {
      id: "alerts",
      icon: Bell,
      label: $_("alerts.title"),
      title: $_("alerts.title"),
      description: $_("alerts.description"),
      group: $_("app.groups.activity"),
    },
  ] as Tab[];

  // Sidebar badges: what is safe to clean in each category, and the number
  // of recorded cleanups next to History.
  $: badges = {
    ...Object.fromEntries(
      CATEGORIES.map((c) => {
        const safe = bytesOf($scan.rows[c.id], "safe");
        return [c.id, $scan.loading ? "" : safe > 0 ? formatBytes(safe) : "—"];
      }),
    ),
    history: String($stats?.activity.length ?? 0),
  } as Record<string, string>;

  let active = "overview";
  let appVersion = "";
  let envInfo = { desktopEnvironment: "unknown", displayProtocol: "unknown" };

  onMount(async () => {
    refresh();
    loadStats();
    // The basic program was started while this one is open.
    listen("another-instance", () => toast($_("basic.otherVersion")));
    // Keeps the stored notification texts in the interface language.
    initAlerts();
    try {
      appVersion = await getVersion();
    } catch {
      appVersion = "";
    }
    try {
      envInfo = await api.invoke<{ desktopEnvironment: string; displayProtocol: string }>("get_environment_info");
    } catch (error) {
      console.error("Failed to read environment info", error);
    }
  });

  function select(id: string) {
    active = id;
    document.getElementById("main-scroll")?.scrollTo({ top: 0 });
  }

  setContext("navigate", select);
  setContext("openAbout", () => (aboutOpen = true));

  $: activeTab = tabs.find((t) => t.id === active)!;
  // Every tab that is not one of the three pages of its own is a category.
  $: categoryId = active as CategoryId;

  let aboutOpen = false;

  async function rescan() {
    toast($_("page.scanning"));
    await refresh();
    toast($_("shell.scanDone"));
  }

  // F1 -> About, Ctrl+Q -> quit, Ctrl+Tab / Ctrl+Shift+Tab -> cycle sections
  // (same shortcuts as Welcome/Repo Selector), plus F5 / Ctrl+R to rescan and
  // Ctrl+I for the notes. The page-specific ones (select, clean, export) live
  // in their own components. Everything is quiet while a dialog is open, so a
  // cleanup in progress cannot be quit or rescanned by accident.
  function handleKeydown(e: KeyboardEvent) {
    if ($modalDepth > 0) return;
    const key = e.key.toLowerCase();
    if (e.key === "F1") {
      e.preventDefault();
      aboutOpen = true;
      return;
    }
    if (e.key === "F5") {
      e.preventDefault();
      if (!$scan.loading) rescan();
      return;
    }
    if (!e.ctrlKey) return;
    if (key === "q") {
      e.preventDefault();
      getCurrentWindow().close();
    } else if (key === "r") {
      e.preventDefault();
      if (!$scan.loading) rescan();
    } else if (key === "i") {
      e.preventDefault();
      if (active !== "overview" && active !== "alerts" && active !== "disk") showNotes.update((v) => !v);
    } else if (e.code === "Tab") {
      e.preventDefault();
      const dir = e.shiftKey ? -1 : 1;
      const idx = tabs.findIndex((t) => t.id === active);
      select(tabs[(idx + dir + tabs.length) % tabs.length].id);
    }
  }
  import WindowControls from "./components/WindowControls.svelte";
  import { frameless, maximized } from "./lib/window";
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="h-screen bg-dark {$frameless && !$maximized ? 'window-card' : ''}">
<div class="flex h-full overflow-hidden">
  <Sidebar {tabs} {active} onSelect={select} {badges} {appVersion} {envInfo} />

  <div class="flex min-w-0 flex-1 flex-col overflow-hidden">
    {#if $frameless}
      <div class="relative h-7 shrink-0" data-tauri-drag-region>
        <div class="absolute right-2 top-[3px]"><WindowControls /></div>
      </div>
    {/if}
    <div class="shrink-0 px-4 {$frameless ? 'pt-0' : 'pt-3'}" data-tauri-drag-region="deep">
      <div
        class="relative flex min-h-[100px] items-end gap-3 overflow-hidden rounded border border-border p-4"
        style="background: linear-gradient(rgba(20, 20, 22, 0.25), rgba(20, 20, 22, 0.82)), url('/assets/hero/sys-cleaner-background.jpg'); background-size: cover; background-position: center;"
      >
        <div class="min-w-0">
          <p class="text-[15px] font-semibold text-white">{activeTab.title}</p>
          <p class="mt-0.5 text-[12px] text-white/70">{activeTab.description}</p>
        </div>
        <div class="ml-auto flex shrink-0 flex-wrap gap-1.5">
          {#if active !== "overview" && active !== "alerts" && active !== "disk"}
            <button
              class="flex items-center gap-1.5 rounded-md border bg-black/45 px-[9px] py-1 hover:bg-black/60 {$showNotes
                ? 'border-primary text-primary'
                : 'border-white/25 text-white'}"
              aria-pressed={$showNotes}
              title="{$_('shell.notes')} (Ctrl+I)"
              on:click={() => showNotes.update((v) => !v)}
            >
              <Eye size={13} />
              {$_("shell.notes")}
            </button>
          {/if}
          {#if active !== "disk"}
          <button
            class="flex items-center gap-1.5 rounded-md border border-white/25 bg-black/45 px-[9px] py-1 text-white hover:bg-black/60 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={$scan.loading}
            title="{$_('shell.rescan')} (F5)"
            on:click={rescan}
          >
            <RefreshCw size={13} class={$scan.loading ? "animate-spin" : ""} />
            {$_("shell.rescan")}
          </button>
          {/if}
        </div>
      </div>
    </div>
    <div id="main-scroll" class="content-scroll flex-1 overflow-y-auto p-4">
      {#key active}
        {#if active === "overview"}
          <Overview />
        {:else if active === "history"}
          <HistoryView />
        {:else if active === "alerts"}
          <AlertsView />
        {:else if active === "disk"}
          <DiskView />
        {:else}
          <CategoryPage id={categoryId} />
        {/if}
      {/key}
    </div>
  </div>
</div>

{#if aboutOpen}
  <AboutDialog mode="advanced" onClose={() => (aboutOpen = false)} />
{/if}

<Toast />
</div>
