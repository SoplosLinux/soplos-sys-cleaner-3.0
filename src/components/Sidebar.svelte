<script lang="ts">
  import type { ComponentType } from "svelte";
  import { getContext } from "svelte";
  import { theme, toggleTheme } from "../lib/theme";
  import { Sun, Moon, Globe } from "lucide-svelte";
  import { _, locale } from "svelte-i18n";
  import { SUPPORTED_LOCALES, LOCALE_NAMES, setLocale, type SupportedLocale } from "../i18n";

  function onLanguageChange(e: Event) {
    setLocale((e.currentTarget as HTMLSelectElement).value as SupportedLocale);
  }

  export let tabs: { id: string; icon: ComponentType; label: string; group?: string }[];
  export let active: string;
  export let onSelect: (id: string) => void;
  // Right-aligned text per tab (safe size of a category, history count).
  export let badges: Record<string, string> = {};
  export let appVersion: string = "";
  export let envInfo: { desktopEnvironment: string; displayProtocol: string } | null = null;

  const openAbout = getContext<() => void>("openAbout");

  // Same mapping/pattern as Welcome's and Repo Selector's Sidebar: the
  // desktop/protocol info rides in the same footer button as the version,
  // not as a separate bar under the main content.
  const DE_LABELS: Record<string, string> = {
    gnome: "GNOME",
    kde: "Plasma",
    xfce: "Xfce",
    unknown: "",
  };
  const PROTO_LABELS: Record<string, string> = {
    wayland: "Wayland",
    x11: "X11",
    unknown: "",
  };

  $: deLabel = envInfo ? (DE_LABELS[envInfo.desktopEnvironment] ?? envInfo.desktopEnvironment) : "";
  $: protoLabel = envInfo ? (PROTO_LABELS[envInfo.displayProtocol] ?? "") : "";
  $: envLabel = [deLabel, protoLabel].filter(Boolean).join(" · ");
  // "3.0.0-1-beta" is shown as "3.0.0-beta": the Debian revision never
  // appears in the interface.
  $: cleanVersion = appVersion.replace(/-\d+-/, "-");
  $: footerLabel = [envLabel, cleanVersion].filter(Boolean).join(" · ") || "Soplos Sys Cleaner";
</script>

<aside class="flex w-[258px] shrink-0 flex-col gap-px overflow-hidden border-r border-border bg-panel px-2 py-2.5">
  <div class="flex items-center gap-2 px-2 pb-2 pt-1 text-[15px] font-bold text-text-bright">
    <img src="/assets/icons/org.soplos.syscleaner.png" alt="Soplos" class="h-[22px] w-[22px] shrink-0 object-contain" />
    <span class="whitespace-nowrap">Sys Cleaner</span>
  </div>

  <!-- The list is what scrolls, so the title and the footer stay in view; its bar keeps its own room to the right of the rows. -->
  <nav class="content-scroll sidebar-scroll flex min-h-0 flex-1 flex-col gap-px overflow-y-auto pr-1.5">
    {#each tabs as tab, i}
      {#if tab.group && tab.group !== tabs[i - 1]?.group}
        <p class="px-2 pb-[3px] pt-2.5 text-[10px] tracking-[0.08em] text-text-muted uppercase">{tab.group}</p>
      {/if}
      <button
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors
          {active === tab.id ? 'bg-primary/15 font-semibold text-primary' : 'text-text-muted hover:bg-surface-2'}"
        on:click={() => onSelect(tab.id)}
      >
        <svelte:component this={tab.icon} size={15} class="shrink-0" />
        <span class="min-w-0 flex-1 leading-tight">{tab.label}</span>
        {#if badges[tab.id]}
          <span class="shrink-0 text-[10.5px] font-medium text-text-muted">{badges[tab.id]}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="mt-3 flex flex-col gap-2.5">
    <div class="flex items-center gap-1.5 px-1">
      <Globe size={13} class="text-text-muted shrink-0" />
      <select
        class="appearance-none flex-1 min-w-0 bg-surface-2 border border-border rounded px-1.5 py-1 text-[0.68rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
        value={$locale}
        title={$_("sidebar.language")}
        on:change={onLanguageChange}
      >
        {#each SUPPORTED_LOCALES as code}
          <option value={code}>{LOCALE_NAMES[code]}</option>
        {/each}
      </select>
    </div>
    <div class="pt-2.5 border-t border-border flex items-center gap-2 px-1">
      <button
        class="p-1 rounded text-text-muted hover:bg-surface hover:text-text-bright shrink-0"
        on:click={toggleTheme}
        title={$theme === "dark" ? $_("sidebar.switchToLight") : $_("sidebar.switchToDark")}
      >
        <svelte:component this={$theme === "dark" ? Moon : Sun} size={14} />
      </button>
      <button
        class="text-[0.62rem] text-text-muted hover:text-text-bright hover:underline"
        on:click={openAbout}
        title={$_("sidebar.aboutTooltip")}
      >
        {footerLabel}
      </button>
    </div>
  </div>
</aside>
