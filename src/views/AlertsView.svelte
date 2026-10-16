<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { api } from "../lib/api";
  import { toast } from "../lib/ui";
  import NumberField from "../components/NumberField.svelte";
  import { alertTexts, setAlertsEnabled, type AlertsConfig } from "../lib/alerts";

  // Nothing here has a default: the user picks the rules and how often to
  // look, then turns the service on. Until then no service exists.
  let enabled = false;
  let active = false;
  let busy = false;

  let byTime = false;
  let timeValue: number | null = null;
  let timeUnit = "";
  let byWeight = false;
  let weightValue: number | null = null;
  let weightUnit = "";
  let checkValue: number | null = null;
  let checkUnit = "";

  const isPositive = (value: number | null) => value !== null && Number.isFinite(value) && value >= 1;

  $: timeOk = !byTime || (isPositive(timeValue) && timeUnit !== "");
  $: weightOk = !byWeight || (isPositive(weightValue) && weightUnit !== "");
  $: checkOk = isPositive(checkValue) && checkUnit !== "";
  $: valid = (byTime || byWeight) && timeOk && weightOk && checkOk;

  function load(config: AlertsConfig) {
    enabled = config.enabled;
    setAlertsEnabled(config.enabled);
    // Turning the alerts off clears every value (the backend empties them),
    // so what shows here is exactly what is stored: nothing, until the user
    // chooses and turns them on.
    byTime = config.byTime;
    timeValue = config.timeValue > 0 ? config.timeValue : null;
    timeUnit = config.timeUnit;
    byWeight = config.byWeight;
    weightValue = config.weightValue > 0 ? config.weightValue : null;
    weightUnit = config.weightUnit;
    checkValue = config.checkValue > 0 ? config.checkValue : null;
    checkUnit = config.checkUnit;
  }

  async function refreshActive() {
    try {
      active = await api.invoke<boolean>("alerts_service_active");
    } catch {
      active = false;
    }
  }

  onMount(async () => {
    try {
      load(await api.invoke<AlertsConfig>("get_alerts"));
    } catch (error) {
      console.error("Failed to read the alerts", error);
    }
    await refreshActive();
  });

  async function apply() {
    if (!valid || busy) return;
    busy = true;
    try {
      const config = await api.invoke<AlertsConfig>("set_alerts", {
        settings: {
          byTime,
          timeValue: byTime ? timeValue : 0,
          timeUnit: byTime ? timeUnit : "",
          byWeight,
          weightValue: byWeight ? weightValue : 0,
          weightUnit: byWeight ? weightUnit : "",
          checkValue,
          checkUnit,
        },
        texts: alertTexts(),
      });
      load(config);
      await refreshActive();
      toast($_("alerts.applied"));
    } catch (error) {
      toast($_("alerts.error", { values: { error: String(error) } }));
    } finally {
      busy = false;
    }
  }

  async function turnOff() {
    if (busy) return;
    busy = true;
    try {
      load(await api.invoke<AlertsConfig>("disable_alerts"));
      await refreshActive();
      toast($_("alerts.turnedOff"));
    } catch (error) {
      toast($_("alerts.error", { values: { error: String(error) } }));
    } finally {
      busy = false;
    }
  }

  async function sendTest() {
    if (busy) return;
    try {
      await api.invoke("send_test_alert", { title: $_("alerts.notifyTitle"), body: $_("alerts.notifyTest") });
      toast($_("alerts.testSent"));
    } catch (error) {
      toast($_("alerts.testFailed", { values: { error: String(error) } }));
    }
  }

  const select = "rounded border border-border bg-surface-2 px-1.5 py-1 text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 disabled:opacity-40";
</script>

<div class="mx-auto flex max-w-[760px] flex-col gap-2.5">
  <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
    <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("alerts.serviceTitle")}</h3>
    <div class="flex items-center gap-2">
      <span class="h-2 w-2 shrink-0 rounded-full {enabled && active ? 'bg-success' : 'bg-text-muted'}"></span>
      <p class="text-text-bright">{enabled ? $_("alerts.stateOn") : $_("alerts.stateOff")}</p>
    </div>
    <div class="mt-2.5 flex flex-wrap gap-1.5">
      <button
        class="rounded-md border border-primary bg-primary px-3 py-1.5 font-semibold text-white hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
        disabled={!valid || busy}
        on:click={apply}
      >
        {$_("alerts.apply")}
      </button>
      {#if enabled}
        <button class="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-text-bright hover:bg-border disabled:opacity-40" disabled={busy} on:click={turnOff}>
          {$_("alerts.turnOff")}
        </button>
      {/if}
      <button class="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-text-bright hover:bg-border disabled:opacity-40" disabled={busy} on:click={sendTest}>
        {$_("alerts.test")}
      </button>
    </div>
    {#if !valid}
      <p class="mt-2 text-[11px] text-text-muted">{$_("alerts.invalid")}</p>
    {/if}
  </div>

  <div class="rounded-lg border border-border bg-surface px-3 py-2.5">
    <h3 class="mb-[7px] text-[10.5px] font-semibold tracking-[0.07em] text-text-muted uppercase">{$_("alerts.rulesTitle")}</h3>

    <div class="border-b border-border pb-2.5">
      <label class="flex items-center gap-2">
        <input type="checkbox" class="accent-primary" bind:checked={byTime} />
        <b class="text-text-bright">{$_("alerts.byTime")}</b>
      </label>
      <p class="mt-0.5 ml-6 text-[11px] text-text-muted">{$_("alerts.byTimeHint")}</p>
      <div class="mt-1.5 ml-6 flex flex-wrap items-center gap-2">
        <span class="text-text-muted">{$_("alerts.afterLabel")}</span>
        <NumberField bind:value={timeValue} disabled={!byTime} />
        <select class={select} bind:value={timeUnit} disabled={!byTime}>
          <option value="">—</option>
          <option value="days">{$_("alerts.unitDays")}</option>
          <option value="weeks">{$_("alerts.unitWeeks")}</option>
        </select>
      </div>
    </div>

    <div class="border-b border-border py-2.5">
      <label class="flex items-center gap-2">
        <input type="checkbox" class="accent-primary" bind:checked={byWeight} />
        <b class="text-text-bright">{$_("alerts.byWeight")}</b>
      </label>
      <p class="mt-0.5 ml-6 text-[11px] text-text-muted">{$_("alerts.byWeightHint")}</p>
      <div class="mt-1.5 ml-6 flex flex-wrap items-center gap-2">
        <span class="text-text-muted">{$_("alerts.aboveLabel")}</span>
        <NumberField bind:value={weightValue} disabled={!byWeight} />
        <select class={select} bind:value={weightUnit} disabled={!byWeight}>
          <option value="">—</option>
          <option value="MB">MB</option>
          <option value="GB">GB</option>
        </select>
      </div>
    </div>

    <div class="pt-2.5">
      <b class="text-text-bright">{$_("alerts.checkTitle")}</b>
      <p class="mt-0.5 text-[11px] text-text-muted">{$_("alerts.checkHint")}</p>
      <div class="mt-1.5 flex flex-wrap items-center gap-2">
        <NumberField bind:value={checkValue} />
        <select class={select} bind:value={checkUnit}>
          <option value="">—</option>
          <option value="hours">{$_("alerts.unitHours")}</option>
          <option value="days">{$_("alerts.unitDays")}</option>
        </select>
      </div>
    </div>
  </div>

  <p class="px-1 text-[11px] text-text-muted">{$_("alerts.desktopNote")}</p>
</div>
