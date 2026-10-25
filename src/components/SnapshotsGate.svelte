<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { RefreshCw, Camera, Lock } from "lucide-svelte";
  import { snapshotState, loadSnapshots, createSnapshot, type SnapshotTarget } from "../lib/snapshots";
  import { syncOne } from "../lib/scan";
  import { toast } from "../lib/ui";

  // The top of the snapshots page. Reading the snapshots needs the password, so
  // it is asked for when the page opens, once: the list is not part of the scan
  // that runs when the window opens. If the password is refused or the read
  // fails, the page keeps the button to try again. Afterwards it offers to read
  // the list again and to make a snapshot now.
  let comment = "";
  let targetIndex = 0;
  let creating = false;

  $: targets = $snapshotState.targets;
  $: target = targets[Math.min(targetIndex, Math.max(0, targets.length - 1))] as SnapshotTarget | undefined;
  $: listedAt = $snapshotState.listedAt
    ? new Date($snapshotState.listedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : "";

  async function show() {
    if (await loadSnapshots()) await syncOne("snapshots");
  }

  // Only the first time: a refused password leaves the button, it does not ask
  // again every time the page is opened.
  onMount(() => {
    if (!$snapshotState.loaded && !$snapshotState.loading && !$snapshotState.failed) void show();
  });

  async function create() {
    if (!target || creating) return;
    creating = true;
    const ok = await createSnapshot(target, comment.trim());
    creating = false;
    if (ok) {
      comment = "";
      // createSnapshot has already read the list again.
      await syncOne("snapshots");
      toast($_("snapshots.created"));
    } else {
      toast($_("snapshots.createFailed"));
    }
  }

  const label = (t: SnapshotTarget) => (t.tool === "snapper" ? `Snapper · ${t.config}` : "Timeshift");
  const validComment = (text: string) => /^[\p{L}\p{N} .,_-]{1,60}$/u.test(text.trim()) && !text.trim().startsWith("-");
</script>

{#if !$snapshotState.loaded}
  <div class="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-surface px-3 py-3">
    <Lock size={16} class="shrink-0 text-text-muted" />
    <p class="min-w-0 flex-1 text-[0.8rem] text-text">{$snapshotState.failed ? $_("snapshots.failed") : $_("snapshots.gate")}</p>
    <button
      class="rounded bg-primary px-3.5 py-1.5 text-[0.8rem] font-semibold text-white hover:bg-primary-hover disabled:opacity-60"
      disabled={$snapshotState.loading}
      on:click={show}
    >
      {$snapshotState.loading ? $_("snapshots.reading") : $_("snapshots.show")}
    </button>
  </div>
{:else}
  <div class="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2.5 text-[0.78rem]">
    <span class="text-text-muted">{$_("snapshots.listedAt", { values: { time: listedAt } })}</span>
    <button
      class="flex items-center gap-1.5 rounded border border-border px-2.5 py-1 text-text hover:bg-surface-2 disabled:opacity-60"
      disabled={$snapshotState.loading}
      on:click={show}
    >
      <RefreshCw size={12} class={$snapshotState.loading ? "animate-spin" : ""} />
      {$_("snapshots.reload")}
    </button>
    <span class="mx-1 hidden h-4 border-l border-border sm:block"></span>
    {#if targets.length === 0}
      <span class="text-text-muted">{$_("snapshots.noTool")}</span>
    {:else}
      {#if targets.length > 1}
        <select class="rounded border border-border bg-surface-2 px-2 py-1 text-text-bright outline-none" bind:value={targetIndex} aria-label={$_("snapshots.where")}>
          {#each targets as t, i}
            <option value={i}>{label(t)}</option>
          {/each}
        </select>
      {:else if target}
        <span class="text-text-muted">{label(target)}</span>
      {/if}
      <input
        type="text"
        class="min-w-[140px] flex-1 rounded border border-border bg-surface-2 px-2 py-1 text-text-bright outline-none placeholder:text-text-muted focus-visible:border-primary sm:max-w-[260px]"
        placeholder={$_("snapshots.comment")}
        maxlength="60"
        bind:value={comment}
        on:keydown={(e) => e.key === "Enter" && validComment(comment) && create()}
      />
      <button
        class="flex items-center gap-1.5 rounded bg-primary px-3 py-1 font-semibold text-white hover:bg-primary-hover disabled:opacity-60"
        disabled={creating || !validComment(comment)}
        on:click={create}
      >
        <Camera size={12} />
        {creating ? $_("snapshots.creating") : $_("snapshots.create")}
      </button>
    {/if}
  </div>
{/if}
