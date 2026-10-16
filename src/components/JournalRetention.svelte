<script lang="ts">
  import { _ } from "svelte-i18n";
  import { formatBytes } from "../lib/format";
  import { journalPolicy, TIME_PRESETS, SIZE_PRESETS_MB, MB, type JournalMode } from "../lib/journal";
  import { scan, refreshOne } from "../lib/scan";

  // Retention of the systemd journal: by time or by size. Changing it
  // rescans the logs row, whose size is the estimate for that setting.
  const modes: { mode: JournalMode; label: string }[] = [
    { mode: "time", label: "journal.byTime" },
    { mode: "size", label: "journal.bySize" },
  ];

  $: row = $scan.rows.logs[0];
  $: presets = $journalPolicy.mode === "time" ? TIME_PRESETS : SIZE_PRESETS_MB;

  function setMode(mode: JournalMode) {
    if (mode === $journalPolicy.mode) return;
    journalPolicy.set({ mode, value: mode === "time" ? TIME_PRESETS[0] : SIZE_PRESETS_MB[1] });
    refreshOne("logs");
  }

  function setValue(value: number) {
    journalPolicy.update((policy) => ({ ...policy, value }));
    refreshOne("logs");
  }

  function label(value: number): string {
    return $journalPolicy.mode === "time" ? $_("journal.days", { values: { count: value } }) : formatBytes(value * MB);
  }
</script>

<div class="rounded-lg border border-border bg-surface px-3 py-2.5">
  <div class="flex flex-wrap items-center gap-1.5">
    <h3 class="mr-2 text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("journal.title")}</h3>
    {#each modes as m (m.mode)}
      <button
        class="rounded-md border px-[9px] py-1 {$journalPolicy.mode === m.mode ? 'border-primary text-primary' : 'border-border bg-surface-2 text-text'}"
        on:click={() => setMode(m.mode)}
      >
        {$_(m.label)}
      </button>
    {/each}
    <span class="mx-1 h-4 w-px bg-border"></span>
    {#each presets as value (value)}
      <button
        class="rounded-md border px-[9px] py-1 {$journalPolicy.value === value ? 'border-primary text-primary' : 'border-border bg-surface-2 text-text'}"
        on:click={() => setValue(value)}
      >
        {label(value)}
      </button>
    {/each}
    <span class="ml-auto text-[11px] text-text-muted">
      {#if $scan.loading}
        {$_("page.scanning")}
      {:else if !row}
        {$_("journal.nothing")}
      {:else if row.risk === "safe"}
        {$_("journal.estimate", { values: { size: formatBytes(row.sizeBytes) } })}
      {/if}
    </span>
  </div>
</div>
