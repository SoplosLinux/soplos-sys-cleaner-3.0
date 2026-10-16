<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { PackageX } from "lucide-svelte";
  import { api } from "../lib/api";
  import { runJob } from "../lib/job";
  import { formatBytes } from "../lib/format";
  import type { AptCacheInfo } from "../lib/scanTypes";
  import StatHero from "../components/StatHero.svelte";
  import Modal from "../components/Modal.svelte";

  let aptCache: AptCacheInfo | null = null;
  let loading = true;
  let busy = false;
  let confirmClean = false;

  async function load() {
    loading = true;
    try {
      aptCache = await api.invoke<AptCacheInfo>("scan_apt_cache");
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function doClean() {
    confirmClean = false;
    busy = true;
    const bytes = aptCache?.sizeBytes ?? 0;
    const items = aptCache?.debCount ?? 0;
    try {
      const success = await runJob("clean_apt_cache");
      if (success) {
        await api.invoke("record_cleanup", { label: "system.aptCache.title", bytes, items });
      }
      await load();
    } finally {
      busy = false;
    }
  }
</script>

<StatHero
  icon={PackageX}
  title={$_("system.aptCache.title")}
  description={$_("system.aptCache.description", { values: { count: aptCache?.debCount ?? 0 } })}
  sizeBytes={loading ? null : (aptCache?.sizeBytes ?? 0)}
  loading={loading || busy}
  actionLabel={$_("system.aptCache.action")}
  actionDisabled={loading || busy || !aptCache || aptCache.debCount === 0}
  onAction={() => (confirmClean = true)}
/>

{#if confirmClean}
  <Modal
    variant="danger"
    title={$_("system.aptCache.confirmTitle")}
    confirmLabel={$_("system.aptCache.action")}
    onClose={() => (confirmClean = false)}
    onConfirm={doClean}
  >
    <p class="text-text-muted">
      {$_("system.aptCache.confirmBody", { values: { count: aptCache?.debCount ?? 0, size: formatBytes(aptCache?.sizeBytes ?? 0) } })}
    </p>
  </Modal>
{/if}
