import type { ActivityEntry } from "./scanTypes";

export interface DayBucket {
  bytes: number;
  label: string;
}

// The last seven local days (today last) bucketed from the recorded history,
// with a short weekday label per bucket. Nothing is estimated.
export function lastSevenDays(activity: ActivityEntry[], locale: string | undefined): DayBucket[] {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const todayStart = Math.floor(start.getTime() / 1000);
  return Array.from({ length: 7 }, (_, i) => {
    const dayStart = todayStart - (6 - i) * 86400;
    const entries = activity.filter((e) => e.atUnix >= dayStart && e.atUnix < dayStart + 86400);
    return {
      bytes: entries.reduce((sum, e) => sum + e.bytes, 0),
      label: new Date(dayStart * 1000).toLocaleDateString(locale, { weekday: "short" }),
    };
  });
}
