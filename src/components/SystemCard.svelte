<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { _ } from "svelte-i18n";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import type { SystemInfo } from "../lib/scanTypes";

  // Live hardware summary. Polls while this card is on screen and the window
  // is visible; the timer is cleared when the Summary page is left.
  const REFRESH_MS = 3000;

  const DE_LABELS: Record<string, string> = { gnome: "GNOME", kde: "Plasma", xfce: "Xfce" };
  const PROTO_LABELS: Record<string, string> = { wayland: "Wayland", x11: "X11" };

  let info: SystemInfo | null = null;
  let session = "";
  let timer: ReturnType<typeof setInterval> | undefined;
  let busy = false;

  async function load() {
    if (busy || document.hidden) return;
    busy = true;
    try {
      info = await api.invoke<SystemInfo | null>("get_system_info");
    } catch (error) {
      console.error("Failed to read system info", error);
    } finally {
      busy = false;
    }
  }

  onMount(async () => {
    load();
    timer = setInterval(load, REFRESH_MS);
    try {
      const env = await api.invoke<{ desktopEnvironment: string; displayProtocol: string }>("get_environment_info");
      session = [DE_LABELS[env.desktopEnvironment], PROTO_LABELS[env.displayProtocol]].filter(Boolean).join(" · ");
    } catch (error) {
      console.error("Failed to read environment info", error);
    }
  });

  onDestroy(() => clearInterval(timer));

  // "AMD Ryzen 7 8845HS w/ Radeon 780M Graphics" -> "AMD Ryzen 7 8845HS". The
  // full string stays available as the tooltip.
  function shortCpu(model: string): string {
    return model
      .replace(/\((R|TM)\)/g, "")
      .replace(/\s+w\/.*$/, "")
      .replace(/\s+@.*$/, "")
      .replace(/\s+CPU\b/, "")
      .replace(/\s+/g, " ")
      .trim();
  }

  function temperatureTone(celsius: number): string {
    return celsius >= 85 ? "text-danger" : celsius >= 70 ? "text-warning" : "text-text-bright";
  }

  function uptime(seconds: number): string {
    return $_("sysinfo.uptimeFormat", {
      values: { days: Math.floor(seconds / 86400), hours: Math.floor((seconds % 86400) / 3600), minutes: Math.floor((seconds % 3600) / 60) },
    });
  }

  function percent(part: number, total: number): number {
    return total > 0 ? Math.min(100, (part / total) * 100) : 0;
  }
</script>

<div class="min-w-0 rounded-lg border border-border bg-surface px-3 py-2.5">
  <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("sysinfo.title")}</h3>

  {#if !info}
    <p class="text-text-muted">{$_("page.scanning")}</p>
  {:else}
    <div class="grid gap-x-6 gap-y-1 md:grid-cols-2">
      <!-- System and graphics -->
      <div class="min-w-0">
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.distro")}</span>
          <b class="min-w-0 truncate text-text-bright" title={info.distro}>{info.distro || "—"}</b>
        </div>
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.kernel")}</span>
          <b class="min-w-0 truncate text-text-bright" title={info.kernel}>{info.kernel || "—"}</b>
        </div>
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.session")}</span>
          <b class="text-text-bright">{session || "—"}</b>
        </div>
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.uptime")}</span>
          <b class="text-text-bright">{uptime(info.uptimeSecs)}</b>
        </div>
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.gpu")}</span>
          <b class="min-w-0 truncate text-text-bright" title={info.gpuName ?? ""}>{info.gpuName ?? "—"}</b>
        </div>
        {#if info.gpuTempC !== null}
          <div class="flex items-center justify-between gap-3 py-[3px]">
            <span class="shrink-0">{$_("sysinfo.gpuTemperature")}</span>
            <b class={temperatureTone(info.gpuTempC)}>{Math.round(info.gpuTempC)} °C</b>
          </div>
        {/if}
      </div>

      <!-- CPU and memory -->
      <div class="min-w-0">
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.cpu")}</span>
          <b class="min-w-0 truncate text-text-bright" title={info.cpuModel}>
            {shortCpu(info.cpuModel) || "—"} · {$_("sysinfo.threads", { values: { count: info.cpuThreads } })}
          </b>
        </div>
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.usage")}</span>
          <b class="text-text-bright">{info.cpuUsagePercent === null ? "—" : `${Math.round(info.cpuUsagePercent)} %`}</b>
        </div>
        <div class="mb-1 flex h-[5px] overflow-hidden rounded bg-surface-2">
          <i class="block bg-primary transition-all duration-500" style="width: {info.cpuUsagePercent ?? 0}%"></i>
        </div>
        {#if info.cpuTempC !== null}
          <div class="flex items-center justify-between gap-3 py-[3px]">
            <span class="shrink-0">{$_("sysinfo.cpuTemperature")}</span>
            <b class={temperatureTone(info.cpuTempC)}>{Math.round(info.cpuTempC)} °C</b>
          </div>
        {/if}
        <div class="flex items-center justify-between gap-3 py-[3px]">
          <span class="shrink-0">{$_("sysinfo.memory")}</span>
          <b class="text-text-bright">{formatBytes(info.memUsedBytes)} / {formatBytes(info.memTotalBytes)}</b>
        </div>
        <div class="mb-1 flex h-[5px] overflow-hidden rounded bg-surface-2">
          <i class="block bg-primary transition-all duration-500" style="width: {percent(info.memUsedBytes, info.memTotalBytes)}%"></i>
        </div>
        {#if info.swapTotalBytes > 0}
          <div class="flex items-center justify-between gap-3 py-[3px]">
            <span class="shrink-0">{$_("sysinfo.swap")}</span>
            <b class="text-text-bright">{formatBytes(info.swapUsedBytes)} / {formatBytes(info.swapTotalBytes)}</b>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
