import { get } from "svelte/store";
import { _, locale } from "svelte-i18n";
import { api } from "./api";

// Mirrors src-tauri/src/commands/alerts.rs.
export interface AlertsSettings {
  byTime: boolean;
  timeValue: number;
  timeUnit: string;
  byWeight: boolean;
  weightValue: number;
  weightUnit: string;
  checkValue: number;
  checkUnit: string;
}

export interface AlertTexts {
  title: string;
  time: string;
  weight: string;
}

export interface AlertsConfig extends AlertsSettings {
  enabled: boolean;
  enabledAtUnix: number;
  lastNotifiedUnix: number;
  texts: AlertTexts;
}

// The notification texts translated now. The values stay as {days} and {size}
// placeholders: the check fills them in when it sends the notification.
export function alertTexts(): AlertTexts {
  const t = get(_);
  return {
    title: t("alerts.notifyTitle"),
    time: t("alerts.notifyTime", { values: { days: "{days}" } }),
    weight: t("alerts.notifyWeight", { values: { size: "{size}" } }),
  };
}

let enabled = false;

export function setAlertsEnabled(value: boolean) {
  enabled = value;
}

// The check runs without the interface, so the stored texts have to follow
// the interface language: they are refreshed whenever it changes.
export async function syncAlertTexts(): Promise<void> {
  if (!enabled) return;
  try {
    await api.invoke("set_alert_texts", { texts: alertTexts() });
  } catch (error) {
    console.error("Failed to refresh the alert texts", error);
  }
}

export async function initAlerts(): Promise<void> {
  try {
    enabled = (await api.invoke<AlertsConfig>("get_alerts")).enabled;
  } catch {
    enabled = false;
  }
  locale.subscribe(() => {
    void syncAlertTexts();
  });
}
