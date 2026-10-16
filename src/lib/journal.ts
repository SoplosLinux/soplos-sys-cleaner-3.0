import { writable } from "svelte/store";

export type JournalMode = "time" | "size";

export interface JournalPolicy {
  mode: JournalMode;
  // Days to keep in time mode, megabytes to keep in size mode.
  value: number;
}

// The only values the privileged side accepts for the journal are numbers in
// a fixed range (1-365 days, 16-10240 MB); the UI offers these presets.
export const TIME_PRESETS = [7, 14, 30, 90];
export const SIZE_PRESETS_MB = [100, 250, 500, 1024];

export const MB = 1024 * 1024;

// Default matches what the app always did: keep the last 7 days.
export const journalPolicy = writable<JournalPolicy>({ mode: "time", value: 7 });

export function journalCommand(policy: JournalPolicy): string {
  return policy.mode === "time" ? `journalctl --vacuum-time=${policy.value}d` : `journalctl --vacuum-size=${policy.value}M`;
}
