<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Trash2 } from "lucide-svelte";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import type { TrashInfo } from "../lib/scanTypes";
  import StatHero from "../components/StatHero.svelte";
  import Modal from "../components/Modal.svelte";

  let trash: TrashInfo | null = null;
  let loading = true;
  let confirmEmpty = false;

  async function load() {
    loading = true;
    try {
      trash = await api.invoke<TrashInfo>("scan_trash");
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function doEmpty() {
    confirmEmpty = false;
    const bytes = trash?.sizeBytes ?? 0;
    const items = trash?.itemCount ?? 0;
    await api.invoke("empty_trash");
    await api.invoke("record_cleanup", { label: "overview.trash.title", bytes, items });
    await load();
  }
</script>

<StatHero
  icon={Trash2}
  title={$_("overview.trash.title")}
  description={$_("overview.trash.description", { values: { count: trash?.itemCount ?? 0 } })}
  sizeBytes={loading ? null : (trash?.sizeBytes ?? 0)}
  {loading}
  actionLabel={$_("overview.trash.action")}
  actionDisabled={loading || !trash || trash.itemCount === 0}
  danger
  onAction={() => (confirmEmpty = true)}
/>

{#if !loading && trash && trash.itemCount > 0}
  <div class="border border-border rounded-lg p-5 bg-surface mt-5 max-w-lg">
    <p class="text-[0.72rem] font-semibold text-text-muted mb-2 uppercase tracking-wide">{$_("common.about")}</p>
    <p class="text-[0.82rem] text-text-muted">
      {$_("overview.trash.confirmBody", { values: { count: trash.itemCount, size: formatBytes(trash.sizeBytes) } })}
    </p>
  </div>
{/if}

{#if confirmEmpty}
  <Modal
    variant="danger"
    title={$_("overview.trash.confirmTitle")}
    confirmLabel={$_("overview.trash.action")}
    onClose={() => (confirmEmpty = false)}
    onConfirm={doEmpty}
  >
    <p class="text-text-muted">
      {$_("overview.trash.confirmBody", { values: { count: trash?.itemCount ?? 0, size: formatBytes(trash?.sizeBytes ?? 0) } })}
    </p>
  </Modal>
{/if}
