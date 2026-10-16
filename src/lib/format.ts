// Human-readable byte sizes, binary (1024-based) units — matches what `du`
// and every file manager on Soplos shows, so a number here always matches
// what the user sees elsewhere.
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, i);
  return `${i === 0 ? value : value.toFixed(value < 10 ? 2 : 1)} ${units[i]}`;
}

// Relative time ("hace 3 minutos" / "3 minutes ago") for Dashboard timestamps
// (last scan, recent activity) — Intl.RelativeTimeFormat follows the app's
// own locale automatically, so this needs no per-language strings of its own.
export function formatRelativeTime(unixSeconds: number, locale: string): string {
  const diffSeconds = unixSeconds - Math.floor(Date.now() / 1000);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 31536000],
    ["month", 2592000],
    ["week", 604800],
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, secondsInUnit] of units) {
    if (Math.abs(diffSeconds) >= secondsInUnit) {
      return rtf.format(Math.round(diffSeconds / secondsInUnit), unit);
    }
  }
  return rtf.format(diffSeconds, "second");
}
