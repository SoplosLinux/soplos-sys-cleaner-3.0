import { api } from "./api";
import { runJobDetailed } from "./job";

// Disk usage by folder: only reading. The listing of one folder comes from
// commands/disk.rs, either as the user or, when the person asks for it, as
// administrator through the same privileged helper the cleanups use.

export interface DiskEntry {
  name: string;
  path: string;
  sizeBytes: number;
  kind: "dir" | "file" | "link";
  /** A folder that is another filesystem: a mounted partition or a virtual one. */
  otherFs: boolean;
  /** Something inside could not be read, so the size is a minimum. */
  unreadable: boolean;
}

export interface DiskListing {
  path: string;
  entries: DiskEntry[];
  totalBytes: number;
  fsTotalBytes: number;
  fsFreeBytes: number;
}

export type DiskScope = "user" | "system";

/** Folders of the root that are not storage; they are listed but never opened. */
export const VIRTUAL_FOLDERS = ["/proc", "/sys", "/dev", "/run"];

export const diskHome = () => api.invoke<string>("disk_home");

/** Reads one folder. With `admin`, as administrator (asks for the password). */
export async function listFolder(path: string, scope: DiskScope, admin: boolean): Promise<DiskListing> {
  if (!admin) return api.invoke<DiskListing>("scan_disk", { path, scope });
  const status = await runJobDetailed("run_privileged_ops", { ops: [{ op: "disk-list", path }] });
  let listing: DiskListing | null = null;
  let ok = false;
  for (const raw of status.output.split("\n")) {
    const line = raw.trim();
    if (line.startsWith("@@disk ")) {
      try {
        listing = JSON.parse(line.slice("@@disk ".length)) as DiskListing;
      } catch {
        // A line that is not JSON is not a listing.
      }
    } else if (line === "@@op 0 ok") {
      ok = true;
    }
  }
  if (!ok || !listing) throw new Error("The administrator read failed");
  return listing;
}

/** The folders that lead to `path`, from the start of the scope. */
export function crumbs(path: string, root: string): { name: string; path: string }[] {
  const base = root === "/" ? "" : root;
  const rest = path.startsWith(base) ? path.slice(base.length) : "";
  const parts = rest.split("/").filter(Boolean);
  const list = [{ name: root === "/" ? "/" : "~", path: root }];
  let current = base;
  for (const part of parts) {
    current = `${current}/${part}`;
    list.push({ name: part, path: current });
  }
  return list;
}
