// Health: 100 when nothing safe is waiting to be cleaned, falling as safe junk
// piles up (about 5 GB of safe junk gives roughly 37). It measures that and
// nothing else.
export function healthScore(safeBytes: number): number {
  return Math.round(100 * Math.exp(-safeBytes / (1024 * 1024 * 1024) / 5));
}

export function healthColor(score: number): string {
  return score < 40 ? "#ef4444" : score < 70 ? "#f59e0b" : "#22c55e";
}
