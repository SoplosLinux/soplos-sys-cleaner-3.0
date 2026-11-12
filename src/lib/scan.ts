import { writable, get } from "svelte/store";
import { api } from "./api";
import { CATEGORIES, type CategoryId, type Row } from "./categories";
import { journalPolicy, MB, type JournalPolicy } from "./journal";
import { formatBytes } from "./format";
import { snapshotState, loadSnapshots } from "./snapshots";
import type {
  TrashInfo,
  CacheEntry,
  FlatpakEntry,
  SnapRevision,
  AppInfo,
  FlatpakApp,
  SnapApp,
  AppImageApp,
  OrphanPackage,
  ResidualPackage,
  LeftoverEntry,
  AptDeb,
  TempEntry,
  JournaldInfo,
  KernelScan,
  DriverScan,
  FirmwareEntry,
  LanguageEntry,
  DocEntry,
  Stats,
} from "./scanTypes";

// Cache folders that cost time to rebuild (shaders, fonts) are listed as
// "review" instead of "safe": they are regenerable, but never part of the
// one-click cleanups.
const SLOW_REBUILD = /shader|nvidia|dxvk|vkd3d|mesa|fontconfig/i;

// "image + headers + source" from the package names of one kernel.
function packageKinds(packages: string[]): string {
  const kinds: string[] = [];
  if (packages.some((p) => p.startsWith("linux-image-"))) kinds.push("image");
  if (packages.some((p) => p.startsWith("linux-headers-"))) kinds.push("headers");
  if (packages.some((p) => p.startsWith("linux-source-"))) kinds.push("source");
  return kinds.join(" + ");
}

// journalctl has no dry run, so this estimates what a retention would free.
// Vacuuming only removes archived files, oldest first: by size it frees what
// exceeds the limit (never more than the archived total); by time it frees the
// archived files last written before the cutoff. Null when the journal files
// cannot be read, in which case no figure is claimed.
function journalFreeable(info: JournaldInfo, policy: JournalPolicy): number | null {
  if (info.files.length === 0) return null;
  const archived = info.files.filter((f) => f.archived);
  if (policy.mode === "size") {
    const archivedTotal = archived.reduce((sum, f) => sum + f.sizeBytes, 0);
    return Math.min(archivedTotal, Math.max(0, info.sizeBytes - policy.value * MB));
  }
  const cutoff = Math.floor(Date.now() / 1000) - policy.value * 86400;
  return archived.filter((f) => f.modifiedUnix < cutoff).reduce((sum, f) => sum + f.sizeBytes, 0);
}

// Total journal size before/after a cleanup, to record what was really freed.
export async function journalDiskUsage(): Promise<number> {
  try {
    return (await api.invoke<JournaldInfo>("scan_journald")).sizeBytes;
  } catch {
    return 0;
  }
}

const scanners: Record<CategoryId, () => Promise<Row[]>> = {
  async trash() {
    const info = await api.invoke<TrashInfo>("scan_trash");
    if (info.itemCount === 0) return [];
    return [
      {
        id: "trash",
        name: { key: "overview.trash.title" },
        detail: { key: "overview.trash.description", values: { count: info.itemCount } },
        extra: { text: "~/.local/share/Trash" },
        sizeBytes: info.sizeBytes,
        risk: "safe",
        count: info.itemCount,
      },
    ];
  },

  async cache() {
    const entries = await api.invoke<CacheEntry[]>("scan_user_cache");
    return entries.map((e) => {
      const slow = SLOW_REBUILD.test(e.name);
      // A folder with live state (a socket, a lock, something a running
      // program has open) is shown locked, with the reason, never cleaned.
      const noteKey =
        e.protectedReason === "open" ? "notes.cacheOpen" : e.protectedReason === "lock" ? "notes.cacheLock" : e.protectedReason ? "notes.cacheSocket" : slow ? "notes.slowRebuild" : undefined;
      return {
        id: e.path,
        name: { text: e.name },
        detail: { text: `~/.cache/${e.name}` },
        sizeBytes: e.sizeBytes,
        risk: e.protectedReason ? "protected" : slow ? "review" : "safe",
        noteKey,
      } as Row;
    });
  },

  async flatpak() {
    const entries = await api.invoke<FlatpakEntry[]>("scan_flatpak_unused");
    return entries.map((e) => ({
      id: e.reference,
      name: { text: e.name },
      detail: { text: e.reference },
      sizeBytes: e.sizeBytes,
      risk: "safe",
      payload: { installation: e.user ? "user" : "system" },
    }));
  },

  async snap() {
    const entries = await api.invoke<SnapRevision[]>("scan_snap_old_revisions");
    return entries.map((e) => ({
      id: `${e.name}@${e.revision}`,
      name: { text: e.name },
      detail: { key: "rows.disabledRevision" },
      extra: { text: e.revision },
      sizeBytes: e.sizeBytes,
      risk: "safe",
      payload: { name: e.name, revision: e.revision },
    }));
  },

  async apps() {
    // APT, Flatpak, Snap and AppImage apps in one list. A missing tool or
    // folder only empties its own part.
    const part = <T>(command: string) => api.invoke<T[]>(command).catch(() => [] as T[]);
    const [apt, flatpak, snap, appimages] = await Promise.all([
      api.invoke<AppInfo[]>("scan_installed_apps"),
      part<FlatpakApp>("scan_flatpak_apps"),
      part<SnapApp>("scan_snap_apps"),
      part<AppImageApp>("scan_appimages"),
    ]);
    const rows: Row[] = [];
    for (const e of apt) {
      rows.push({
        id: `apt:${e.name}`,
        name: { text: e.name },
        detail: { text: e.summary },
        extra: { text: "APT" },
        sizeBytes: e.sizeBytes,
        risk: "review",
        noteKey: "notes.manualApp",
        packages: [e.name],
        payload: { kind: "apt", name: e.name, base: e.base ? "1" : "0" },
      });
    }
    for (const e of flatpak) {
      rows.push({
        id: `flatpak:${e.reference}`,
        name: { text: e.name || e.reference },
        detail: { text: e.reference },
        extra: { text: "Flatpak" },
        sizeBytes: e.sizeBytes,
        risk: "review",
        noteKey: "notes.flatpakApp",
        payload: { kind: "flatpak", reference: e.reference, origin: e.origin, installation: e.user ? "user" : "system" },
      });
    }
    for (const e of snap) {
      rows.push({
        id: `snap:${e.name}`,
        name: { text: e.name },
        detail: { text: e.classic ? `${e.version} · classic` : e.version },
        extra: { text: "Snap" },
        sizeBytes: e.sizeBytes,
        risk: "review",
        noteKey: "notes.snapApp",
        payload: { kind: "snap", name: e.name, classic: e.classic ? "1" : "0" },
      });
    }
    for (const e of appimages) {
      rows.push({
        id: `appimage:${e.file}`,
        name: { text: e.name },
        detail: { text: e.version ? `${e.version} · ${e.file}` : e.file },
        extra: { text: "AppImage" },
        sizeBytes: e.sizeBytes,
        risk: "review",
        noteKey: "notes.appImageApp",
        payload: { kind: "appimage", file: e.file, name: e.name, version: e.version, url: e.updateUrl },
      });
    }
    return rows.sort((a, b) => b.sizeBytes - a.sizeBytes);
  },

  // Everything below is high risk: nothing is ever "safe" (so one-click
  // cleanups skip it), the kernel in use and the fallback are protected, and
  // the confirmation dialog shows a dry run of the removal.
  async kernels() {
    const scan = await api.invoke<KernelScan>("scan_kernels");
    const rows: Row[] = [];
    for (const k of scan.kernels) {
      if (k.packages.length === 0) continue;
      const protectedKernel = k.running || k.reserve;
      rows.push({
        id: k.version,
        name: { text: k.version },
        detail: { text: packageKinds(k.packages) },
        extra: { key: k.running ? "kernels.running" : k.reserve ? "kernels.reserve" : "kernels.old" },
        sizeBytes: k.sizeBytes,
        risk: protectedKernel ? "protected" : "review",
        noteKey: k.running ? "kernels.noteRunning" : k.reserve ? "kernels.noteReserve" : "kernels.noteOld",
        count: k.packages.length,
        packages: k.packages,
        payload: { kind: "kernel" },
      });
    }
    for (const dir of scan.orphanDirs) {
      rows.push({
        id: dir.path,
        name: { text: dir.path },
        detail: { key: "kernels.orphanDir" },
        extra: { key: "kernels.leftover" },
        sizeBytes: dir.sizeBytes,
        risk: "review",
        noteKey: "kernels.noteOrphanDir",
        payload: { kind: "dir" },
      });
    }
    for (const pkg of scan.orphanKbuild) {
      rows.push({
        id: pkg.name,
        name: { text: pkg.name },
        detail: { key: "kernels.orphanKbuild" },
        extra: { key: "kernels.leftover" },
        sizeBytes: pkg.sizeBytes,
        risk: "review",
        noteKey: "kernels.noteOrphanKbuild",
        packages: [pkg.name],
        payload: { kind: "kbuild" },
      });
    }
    return rows;
  },

  async drivers() {
    const scan = await api.invoke<DriverScan>("scan_drivers");
    const rows: Row[] = scan.packages.map((p) => ({
      id: p.name,
      name: { text: p.name },
      detail: { text: p.description },
      extra: { text: p.vendor },
      sizeBytes: p.sizeBytes,
      risk: "review",
      noteKey: "drivers.noteAbsent",
      packages: [p.name],
      payload: { kind: "driver" },
    }));
    // Without the detection tools nothing can be judged unused, so say so
    // instead of showing an empty page.
    if (!scan.detectionOk) {
      rows.push({
        id: "detection",
        name: { key: "detection.driversRow" },
        detail: { text: "pciutils, usbutils, kmod" },
        sizeBytes: 0,
        risk: "protected",
        noteKey: "detection.driversNote",
      });
    }
    for (const d of scan.dkms) {
      rows.push({
        id: `${d.module}/${d.version}`,
        name: { text: `${d.module} ${d.version}` },
        detail: { key: "drivers.dkmsFor", values: { kernels: d.kernels.join(", ") } },
        extra: { text: "DKMS" },
        sizeBytes: d.sizeBytes,
        risk: "review",
        noteKey: "drivers.noteDkms",
        payload: { kind: "dkms", module: d.module, version: d.version },
      });
    }
    return rows;
  },

  // One row per language, named in the app's language. A language whose
  // active variant is kept still lists the other variants as removable.
  async languages() {
    const entries = await api.invoke<LanguageEntry[]>("scan_languages");
    return entries.map((e) => {
      const isProtected = e.protectedBy !== null;
      return {
        id: e.base,
        name: { language: e.base },
        detail: { text: (isProtected ? e.keepCodes : e.removeCodes).join(", ") },
        extra: { keys: e.places.map((p) => `languages.place.${p}`) },
        sizeBytes: e.sizeBytes,
        risk: isProtected ? "protected" : "review",
        noteKey:
          e.protectedBy === "active"
            ? "languages.noteActive"
            : e.protectedBy === "english"
              ? "languages.noteEnglish"
              : e.keepCodes.length > 0
                ? "languages.noteKeeps"
                : "languages.noteUnused",
        payload: { kind: "language" },
      };
    }) as Row[];
  },

  // Large package documentation folders and a short catalogue of documentation
  // stores. Nothing here is offered as a whole: copyright files and the info
  // index stay.
  // Listing snapshots needs a password, so a scan only returns what the page
  // has already read; the page asks for the list itself (see SnapshotsGate).
  async snapshots() {
    return get(snapshotState).rows;
  },

  async docs() {
    const entries = await api.invoke<DocEntry[]>("scan_docs");
    return entries.map((e) => {
      const isPackage = e.id.startsWith("pkg:");
      return {
        id: e.id,
        name: isPackage ? { text: e.id.slice(4) } : { key: `docs.item.${e.id}` },
        detail: { text: e.path },
        extra: isPackage ? { key: "docs.keepsCopyright" } : e.keeps.length > 0 ? { key: "docs.keepsIndex" } : { text: "—" },
        sizeBytes: e.sizeBytes,
        risk: "review",
        noteKey: "docs.noteBack",
        payload: { kind: "docs" },
      };
    }) as Row[];
  },

  async firmware() {
    const entries = await api.invoke<FirmwareEntry[]>("scan_firmware");
    return entries.map((f) => ({
      id: f.family,
      name: { text: f.family },
      detail: f.owner ? { text: f.owner } : { key: "firmware.noOwner" },
      extra: { key: f.temporary ? "firmware.temporary" : "firmware.permanent" },
      sizeBytes: f.sizeBytes,
      risk: f.protectedBy ? "protected" : "review",
      noteKey:
        f.protectedBy === "detection"
          ? "detection.firmwareNote"
          : f.protectedBy === "hardware"
            ? "firmware.noteHardware"
            : f.protectedBy === "driver"
              ? "firmware.noteDriver"
              : "firmware.noteUnused",
      payload: { kind: "firmware" },
    })) as Row[];
  },

  async temp() {
    const entries = await api.invoke<TempEntry[]>("scan_temp_files");
    return entries.map((e) => ({
      id: e.path,
      name: { text: e.path },
      detail: { key: e.isDir ? "rows.folder" : "rows.file" },
      extra: { key: e.recent ? "rows.recent" : "rows.olderThanDay" },
      sizeBytes: e.sizeBytes,
      // Younger than a day may still be in use: left for the user to review.
      risk: e.recent ? "review" : "safe",
      noteKey: e.recent ? "notes.recentTemp" : undefined,
    }));
  },

  async aptCache() {
    // One row per cached package file, like the legacy list.
    const debs = await api.invoke<AptDeb[]>("scan_apt_debs");
    return debs.map((d) => ({
      id: d.name,
      name: { text: d.name },
      detail: { text: "/var/cache/apt/archives" },
      sizeBytes: d.sizeBytes,
      risk: "safe",
    }));
  },

  async residual() {
    const entries = await api.invoke<ResidualPackage[]>("scan_residual_configs");
    return entries.map((e) => ({
      id: e.name,
      name: { text: e.name },
      detail: { text: e.summary ? `${e.version} · ${e.summary}` : e.version },
      sizeBytes: e.sizeBytes,
      // Purging deletes the settings the package left; they are not
      // recovered if it is installed again. Login screens, boot loaders and
      // base packages are never offered: their purge script can delete files
      // other packages use.
      risk: e.systemCritical ? "protected" : "review",
      noteKey: e.systemCritical ? "residual.noteSystem" : "residual.note",
    }));
  },

  // What uninstalled programs left in the home folder. Nothing here is ever
  // "safe": the quick and full cleanups skip it, and the page preselects
  // nothing. A folder that is open or that changed lately is shown locked.
  async leftovers() {
    const entries = await api.invoke<LeftoverEntry[]>("scan_leftovers");
    return entries.map((e) => {
      const reason =
        e.reason === "creator" || e.reason === "refs"
          ? { key: `leftovers.reason.${e.reason}`, values: { program: e.package ?? "" } }
          : e.reason === "package"
            ? e.removedOn
              ? { key: "leftovers.reason.packageOn", values: { package: e.package ?? "", date: e.removedOn } }
              : { key: "leftovers.reason.package", values: { package: e.package ?? "" } }
            : { key: `leftovers.reason.${e.reason}` };
      const noteKey = e.protectedReason ? `leftovers.notes.${e.protectedReason}` : `leftovers.notes.${e.reason}`;
      return {
        id: e.path,
        // The path is the name: what is cleaned is a folder, and where it is says more
        // than its name alone.
        name: { text: e.display },
        detail: reason,
        sizeBytes: e.sizeBytes,
        risk: e.protectedReason ? "protected" : "review",
        // The creator was seen and is gone: that is proof, so the page selects it.
        preselected: e.reason === "creator" && !e.protectedReason,
        noteKey,
      } as Row;
    });
  },

  async orphans() {
    const entries = await api.invoke<OrphanPackage[]>("scan_orphan_packages");
    return entries.map((e) => ({
      id: e.name,
      name: { text: e.name },
      detail: { text: e.summary },
      sizeBytes: e.sizeBytes,
      // apt says nothing needs them, but the list is the user's to check.
      risk: "review",
      noteKey: "orphans.note",
      packages: [e.name],
    }));
  },

  async logs(): Promise<Row[]> {
    const info = await api.invoke<JournaldInfo>("scan_journald");
    if (info.sizeBytes === 0) return [];
    const policy = get(journalPolicy);
    const freeable = journalFreeable(info, policy);
    if (freeable === 0) return [];
    const known = freeable !== null;
    return [
      {
        id: "logs",
        name: { text: "journald · /var/log/journal" },
        detail:
          policy.mode === "time"
            ? { key: "rows.keepsDays", values: { days: policy.value } }
            : { key: "journal.keepsSize", values: { size: formatBytes(policy.value * MB) } },
        // The estimate when the journal files are readable; otherwise the
        // whole size, shown as "review" so one-click cleanups skip it.
        sizeBytes: known ? freeable : info.sizeBytes,
        risk: known ? "safe" : "review",
        noteKey: known ? "journal.estimateNote" : "journal.unknownNote",
        count: 0,
      },
    ];
  },
};

export interface ScanState {
  rows: Record<CategoryId, Row[]>;
  loading: boolean;
}

function emptyRows(): Record<CategoryId, Row[]> {
  return Object.fromEntries(CATEGORIES.map((c) => [c.id, [] as Row[]])) as Record<CategoryId, Row[]>;
}

export const scan = writable<ScanState>({ rows: emptyRows(), loading: true });
export const stats = writable<Stats | null>(null);

export async function loadStats(): Promise<void> {
  try {
    stats.set(await api.invoke<Stats>("get_stats"));
  } catch (error) {
    console.error("Failed to read stats", error);
  }
}

async function scanCategory(id: CategoryId): Promise<Row[]> {
  try {
    return await scanners[id]();
  } catch (error) {
    console.error(`Scan failed for ${id}`, error);
    return [];
  }
}

let inFlight: Promise<void> | null = null;

// Full scan of every category, in parallel. This is the one place that
// records a scan, so "last scan" means exactly what it says.
export function refresh(): Promise<void> {
  if (inFlight) return inFlight;
  scan.update((s) => ({ ...s, loading: true }));
  inFlight = (async () => {
    try {
      const results = await Promise.all(CATEGORIES.map(async (c) => [c.id, await scanCategory(c.id)] as const));
      const rows = emptyRows();
      for (const [id, list] of results) rows[id] = list;
      scan.set({ rows, loading: false });
      await api.invoke("record_scan");
      await loadStats();
    } finally {
      scan.update((s) => ({ ...s, loading: false }));
      inFlight = null;
    }
  })();
  return inFlight;
}

// Rescan a single category after cleaning it.
export async function refreshOne(id: CategoryId): Promise<void> {
  // After a cleanup the list is read again, which the password granted a
  // moment ago still covers.
  if (id === "snapshots") await loadSnapshots();
  await syncOne(id);
}

// Puts what a category holds now on its page, without reading anything that
// asks for the password again: the snapshots page uses it right after it read
// the list itself.
export async function syncOne(id: CategoryId): Promise<void> {
  const list = await scanCategory(id);
  scan.update((s) => ({ ...s, rows: { ...s.rows, [id]: list } }));
}

export function currentRows(id: CategoryId): Row[] {
  return get(scan).rows[id];
}
