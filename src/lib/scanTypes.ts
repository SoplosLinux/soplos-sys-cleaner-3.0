// Mirrors the Rust structs in src-tauri/src/commands/trash.rs and user_cache.rs.
export interface TrashInfo {
  sizeBytes: number;
  itemCount: number;
}

export interface CacheEntry {
  name: string;
  path: string;
  sizeBytes: number;
  // Set when it holds live state and is not cleaned: known, socket, lock, open.
  protectedReason: string | null;
}

// Mirrors src-tauri/src/commands/flatpak.rs.
export interface FlatpakEntry {
  reference: string;
  name: string;
  sizeBytes: number;
  user: boolean;
}

// Mirrors src-tauri/src/commands/snap.rs.
export interface SnapRevision {
  name: string;
  revision: string;
  sizeBytes: number;
}

// Mirror src-tauri/src/commands/other_apps.rs.
export interface FlatpakApp {
  reference: string;
  name: string;
  origin: string;
  user: boolean;
  sizeBytes: number;
}

export interface SnapApp {
  name: string;
  version: string;
  classic: boolean;
  sizeBytes: number;
}

export interface AppImageApp {
  file: string;
  name: string;
  version: string;
  updateUrl: string;
  sizeBytes: number;
}

// Mirrors src-tauri/src/commands/residual.rs.
export interface ResidualPackage {
  name: string;
  version: string;
  summary: string;
  sizeBytes: number;
  systemCritical: boolean;
}

// Mirrors src-tauri/src/commands/leftovers.rs.
export interface LeftoverEntry {
  name: string;
  path: string;
  display: string;
  location: "config" | "share" | "state" | "home" | "flatpak" | "snap";
  sizeBytes: number;
  reason: "package" | "flatpak" | "snap";
  package: string | null;
  removedOn: string | null;
  protectedReason: "open" | null;
}

// Mirrors src-tauri/src/commands/orphans.rs.
export interface OrphanPackage {
  name: string;
  summary: string;
  sizeBytes: number;
}

// Mirrors src-tauri/src/commands/apps.rs.
export interface AppInfo {
  name: string;
  summary: string;
  sizeBytes: number;
  hasDesktop: boolean;
  base: boolean;
}

// Mirrors src-tauri/src/commands/apt_cache.rs.
export interface AptDeb {
  name: string;
  sizeBytes: number;
}

export interface AptCacheInfo {
  sizeBytes: number;
  debCount: number;
}

// Mirrors src-tauri/src/commands/temp_files.rs.
export interface TempEntry {
  path: string;
  sizeBytes: number;
  isDir: boolean;
  recent: boolean;
}

// Mirrors src-tauri/src/commands/logs.rs.
export interface JournalFile {
  sizeBytes: number;
  modifiedUnix: number;
  archived: boolean;
}

export interface JournaldInfo {
  sizeBytes: number;
  files: JournalFile[];
}

// Mirrors src-tauri/src/commands/kernels.rs.
export interface KernelEntry {
  version: string;
  packages: string[];
  sizeBytes: number;
  running: boolean;
  reserve: boolean;
}

export interface PathEntry {
  path: string;
  sizeBytes: number;
}

export interface PackageEntry {
  name: string;
  sizeBytes: number;
}

export interface KernelScan {
  kernels: KernelEntry[];
  orphanDirs: PathEntry[];
  orphanKbuild: PackageEntry[];
}

// Mirrors src-tauri/src/commands/drivers.rs.
export interface DriverEntry {
  name: string;
  description: string;
  vendor: string;
  sizeBytes: number;
}

export interface DkmsEntry {
  module: string;
  version: string;
  kernels: string[];
  sizeBytes: number;
}

export interface DriverScan {
  packages: DriverEntry[];
  dkms: DkmsEntry[];
  vmGuest: string;
  detectionOk: boolean;
}

// Mirrors src-tauri/src/commands/firmware.rs.
export interface FirmwareEntry {
  family: string;
  sizeBytes: number;
  owner: string | null;
  temporary: boolean;
  protectedBy: "hardware" | "driver" | "detection" | null;
}

// Mirrors src-tauri/src/commands/languages.rs.
export interface LanguageEntry {
  base: string;
  removeCodes: string[];
  keepCodes: string[];
  places: string[];
  sizeBytes: number;
  protectedBy: "active" | "english" | null;
}

// Mirrors src-tauri/src/commands/docs.rs.
export interface DocEntry {
  id: string;
  path: string;
  sizeBytes: number;
  keeps: string[];
}

// Mirrors src-tauri/src/commands/simulate.rs.
export interface Simulation {
  removes: string[];
  blocked: string | null;
  error: string | null;
}

// Mirrors src-tauri/src/commands/stats.rs.
export type CleanKind = "quick" | "full" | "manual";

export interface ActivityEntry {
  label: string;
  bytes: number;
  items: number;
  atUnix: number;
  // Entries written before the receipts existed have an empty kind, no
  // duration, no command and no details.
  kind: CleanKind | "";
  durationMs: number;
  ok: boolean;
  command: string;
  details: string[];
}

export interface Stats {
  totalBytesRecovered: number;
  totalItemsCleaned: number;
  lastScanUnix: number | null;
  activity: ActivityEntry[];
}

// Mirrors src-tauri/src/commands/system_info.rs.
export interface SystemInfo {
  distro: string;
  kernel: string;
  uptimeSecs: number;
  cpuModel: string;
  cpuThreads: number;
  cpuUsagePercent: number | null;
  cpuTempC: number | null;
  memUsedBytes: number;
  memTotalBytes: number;
  swapUsedBytes: number;
  swapTotalBytes: number;
  gpuName: string | null;
  gpuTempC: number | null;
}

export interface DiskInfo {
  mount: string;
  usedBytes: number;
  totalBytes: number;
}

// Mirrors src-tauri/src/commands/snapshots.rs.
export interface SnapshotEntry {
  tool: "timeshift" | "snapper";
  config: string;
  id: string;
  date: string;
  kind: string;
  tags: string;
  description: string;
  sizeBytes: number;
}
