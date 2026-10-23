import { writable } from "svelte/store";
import { runJobDetailed } from "./job";
import type { Row } from "./categories";
import type { SnapshotEntry } from "./scanTypes";

// The snapshots of Timeshift and Snapper. Listing them needs administrator
// rights, so it is never part of the scan that runs when the window opens: the
// page asks for it, and the rows stay here until the next time it is asked.

export interface SnapshotTarget {
  tool: "timeshift" | "snapper";
  config: string;
}

export interface SnapshotState {
  loaded: boolean;
  loading: boolean;
  failed: boolean;
  /** Epoch milliseconds of the last list. */
  listedAt: number | null;
  rows: Row[];
  /** Where a snapshot can be made. */
  targets: SnapshotTarget[];
}

export const snapshotState = writable<SnapshotState>({ loaded: false, loading: false, failed: false, listedAt: null, rows: [], targets: [] });

function sourceCell(entry: SnapshotEntry): Row["extra"] {
  if (entry.tool === "snapper") return { key: "snapshots.source.snapper", values: { config: entry.config } };
  return { key: entry.kind === "btrfs" ? "snapshots.source.timeshiftBtrfs" : "snapshots.source.timeshiftRsync" };
}

// The newest of each tool (and Snapper configuration) is never offered: it is
// the restore point the machine keeps.
function newestOf(entries: SnapshotEntry[], entry: SnapshotEntry): string {
  const same = entries.filter((e) => e.tool === entry.tool && e.config === entry.config);
  const sorted = same.map((e) => e.id).sort((a, b) => (entry.tool === "snapper" ? Number(a) - Number(b) : a.localeCompare(b)));
  return sorted[sorted.length - 1];
}

function rowsOf(entries: SnapshotEntry[]): Row[] {
  return entries
    .map((entry) => {
      const newest = newestOf(entries, entry) === entry.id;
      const detail = [entry.tool === "snapper" ? entry.kind : "", entry.tags ? `[${entry.tags}]` : "", entry.description].filter(Boolean).join(" ");
      return {
        id: `${entry.tool}:${entry.config}:${entry.id}`,
        name: { text: entry.date },
        detail: { text: detail },
        extra: sourceCell(entry),
        sizeBytes: entry.sizeBytes,
        // Deleting a snapshot cannot be undone: always for review.
        risk: newest ? "protected" : "review",
        noteKey: newest ? "snapshots.noteNewest" : "snapshots.noteReview",
        payload: { tool: entry.tool, config: entry.config, id: entry.id },
      } as Row;
    })
    .sort((a, b) => (b.name.text ?? "").localeCompare(a.name.text ?? ""));
}

/** Asks for the password and lists the snapshots. True when the list was read. */
export async function loadSnapshots(): Promise<boolean> {
  snapshotState.update((s) => ({ ...s, loading: true, failed: false }));
  try {
    const status = await runJobDetailed("run_privileged_ops", { ops: [{ op: "snapshot-list" }] });
    const entries: SnapshotEntry[] = [];
    const targets: SnapshotTarget[] = [];
    let ok = false;
    for (const raw of status.output.split("\n")) {
      const line = raw.trim();
      if (line.startsWith("@@snapshot ")) {
        try {
          entries.push(JSON.parse(line.slice("@@snapshot ".length)) as SnapshotEntry);
        } catch {
          // A line that is not JSON is not a snapshot.
        }
      } else if (line === "@@tool timeshift") {
        targets.push({ tool: "timeshift", config: "" });
      } else if (line.startsWith("@@tool snapper ")) {
        targets.push({ tool: "snapper", config: line.slice("@@tool snapper ".length) });
      } else if (line === "@@op 0 ok") {
        ok = true;
      }
    }
    if (!ok) {
      snapshotState.update((s) => ({ ...s, loading: false, failed: true }));
      return false;
    }
    snapshotState.set({ loaded: true, loading: false, failed: false, listedAt: Date.now(), rows: rowsOf(entries), targets });
    return true;
  } catch (error) {
    console.error("Listing the snapshots failed", error);
    snapshotState.update((s) => ({ ...s, loading: false, failed: true }));
    return false;
  }
}

/** Makes a snapshot now and reads the list again. True when it was made. */
export async function createSnapshot(target: SnapshotTarget, comment: string): Promise<boolean> {
  let ok = false;
  try {
    const status = await runJobDetailed("run_privileged_ops", { ops: [{ op: "snapshot-create", tool: target.tool, config: target.config, comment }] });
    ok = status.output.split("\n").some((line) => line.trim() === "@@op 0 ok");
  } catch (error) {
    console.error("Creating the snapshot failed", error);
  }
  if (ok) await loadSnapshots();
  return ok;
}
