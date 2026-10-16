<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { ScrollText } from "lucide-svelte";
  import { api } from "../lib/api";
  import { runJob } from "../lib/job";
  import { formatBytes } from "../lib/format";
  import type { JournaldInfo } from "../lib/scanTypes";
  import StatHero from "../components/StatHero.svelte";
  import Modal from "../components/Modal.svelte";

  let journald: JournaldInfo | null = null;
  let loading = true;
  let busy = false;
  let confirmClean = false;

  async function load() {
    loading = true;
    try {
      journald = await api.invoke<JournaldInfo>("scan_journald");
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function doClean() {
    confirmClean = false;
    busy = true;
    const bytes = journald?.sizeBytes ?? 0;
    try {
      const success = await runJob("clean_journald");
      if (success) {
        await api.invoke("record_cleanup", { label: "system.logs.title", bytes, items: 0 });
      }
      await load();
    } finally {
      busy = false;
    }
  }
</script>

<StatHero
  icon={ScrollText}
  title={$_("system.logs.title")}
  description={$_("system.logs.description")}
  sizeBytes={loading ? null : (journald?.sizeBytes ?? 0)}
  loading={loading || busy}
  actionLabel={$_("system.logs.action")}
  actionDisabled={loading || busy || !journald || journald.sizeBytes === 0}
  onAction={() => (confirmClean = true)}
/>

{#if confirmClean}
  <Modal
    variant="danger"
    title={$_("system.logs.confirmTitle")}
    confirmLabel={$_("system.logs.action")}
    onClose={() => (confirmClean = false)}
    onConfirm={doClean}
  >
    <p class="text-text-muted">{$_("system.logs.confirmBody", { values: { size: formatBytes(journald?.sizeBytes ?? 0) } })}</p>
  </Modal>
{/if}
