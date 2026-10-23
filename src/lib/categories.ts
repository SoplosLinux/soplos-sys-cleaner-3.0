import type { ComponentType } from "svelte";
import { Trash2, Archive, Box, RotateCcw, AppWindow, FolderClock, PackageX, PackageMinus, FileX, ScrollText, Cpu, Plug, Microchip, Languages, BookOpen, Camera } from "lucide-svelte";
import { journalCommand, type JournalPolicy } from "./journal";

export type CategoryId =
  | "trash"
  | "cache"
  | "flatpak"
  | "snap"
  | "apps"
  | "temp"
  | "aptCache"
  | "orphans"
  | "residual"
  | "logs"
  | "kernels"
  | "snapshots"
  | "drivers"
  | "firmware"
  | "languages"
  | "docs";

// safe: preselected and included in quick/full clean. review: listed but never
// preselected and never part of quick/full clean. protected: shown, locked.
export type Risk = "safe" | "review" | "protected";

// A table cell is either a literal string (paths, names) or an i18n key, so a
// language switch re-renders every cell without rescanning.
export interface Cell {
  text?: string;
  key?: string;
  values?: Record<string, string | number>;
  // A language code shown as its name in the app's own language ("es" ->
  // "Spanish" or "Español"), so a language switch renames it too.
  language?: string;
  // Several i18n keys shown together ("Translations + Manual pages").
  keys?: string[];
}

export interface Row {
  id: string;
  name: Cell;
  detail?: Cell;
  extra?: Cell;
  sizeBytes: number;
  risk: Risk;
  // i18n key of the explanation shown under the name (review/protected rows).
  noteKey?: string;
  // Items this row stands for in the history (a trash row stands for many).
  count?: number;
  // Arguments the cleaner needs for this row (snap name + revision, ...).
  payload?: Record<string, string>;
  // Packages this row removes. Rows that have them get a dry run of the
  // removal in the confirmation dialog.
  packages?: string[];
}

export interface CategoryDef {
  id: CategoryId;
  icon: ComponentType;
  // Sidebar label.
  labelKey: string;
  // Page title in dialogs; also the label stored in stats.json history.
  titleKey: string;
  descriptionKey: string;
  admin: boolean;
  // Shown verbatim in "Equivalent to" and in the confirmation dialog.
  command: string;
  // i18n keys of the table columns: name first, then detail, then extra.
  headers: string[];
  conceptKey: string;
  keepKey: string;
}

export const CATEGORIES: CategoryDef[] = [
  {
    id: "trash",
    icon: Trash2,
    labelKey: "overview.trash.title",
    titleKey: "overview.trash.title",
    descriptionKey: "app.tabs.trash.description",
    admin: false,
    command: "gio trash --empty",
    headers: ["cols.item", "cols.contents", "cols.location"],
    conceptKey: "categories.trash.concept",
    keepKey: "categories.trash.keep",
  },
  {
    id: "cache",
    icon: Archive,
    labelKey: "overview.cache.title",
    titleKey: "overview.cache.title",
    descriptionKey: "app.tabs.cache.description",
    admin: false,
    command: "rm -rf ~/.cache/<directory>",
    headers: ["cols.item", "cols.path"],
    conceptKey: "categories.cache.concept",
    keepKey: "categories.cache.keep",
  },
  {
    id: "flatpak",
    icon: Box,
    labelKey: "app.tabs.flatpak.label",
    titleKey: "overview.flatpak.title",
    descriptionKey: "app.tabs.flatpak.description",
    admin: true,
    command: "flatpak uninstall --unused -y",
    headers: ["cols.item", "cols.reference"],
    conceptKey: "categories.flatpak.concept",
    keepKey: "categories.flatpak.keep",
  },
  {
    id: "snap",
    icon: RotateCcw,
    labelKey: "app.tabs.snap.label",
    titleKey: "overview.snap.title",
    descriptionKey: "app.tabs.snap.description",
    admin: true,
    command: "snap remove <snap> --revision=<rev>",
    headers: ["cols.item", "cols.state", "cols.revision"],
    conceptKey: "categories.snap.concept",
    keepKey: "categories.snap.keep",
  },
  {
    id: "apps",
    icon: AppWindow,
    labelKey: "overview.apps.title",
    titleKey: "overview.apps.title",
    descriptionKey: "app.tabs.apps.description",
    admin: true,
    command: "apt-get remove -y <package> | flatpak uninstall <app> | snap remove <name> | rm <file>.AppImage",
    headers: ["cols.item", "cols.description", "cols.source"],
    conceptKey: "categories.apps.concept",
    keepKey: "categories.apps.keep",
  },
  {
    id: "temp",
    icon: FolderClock,
    labelKey: "system.temp.title",
    titleKey: "system.temp.title",
    descriptionKey: "app.tabs.temp.description",
    admin: true,
    command: "rm -rf -- <path>",
    headers: ["cols.path", "cols.type", "cols.age"],
    conceptKey: "categories.temp.concept",
    keepKey: "categories.temp.keep",
  },
  {
    id: "aptCache",
    icon: PackageX,
    labelKey: "system.aptCache.title",
    titleKey: "system.aptCache.title",
    descriptionKey: "app.tabs.aptCache.description",
    admin: true,
    command: "rm -- /var/cache/apt/archives/<package>.deb",
    headers: ["cols.item", "cols.path"],
    conceptKey: "categories.aptCache.concept",
    keepKey: "categories.aptCache.keep",
  },
  {
    id: "orphans",
    icon: PackageMinus,
    labelKey: "orphans.title",
    titleKey: "orphans.title",
    descriptionKey: "orphans.description",
    admin: true,
    command: "apt-get remove -y <package>",
    headers: ["cols.item", "cols.description"],
    conceptKey: "orphans.concept",
    keepKey: "orphans.keep",
  },
  {
    id: "residual",
    icon: FileX,
    labelKey: "residual.title",
    titleKey: "residual.title",
    descriptionKey: "residual.description",
    admin: true,
    command: "apt-get purge -y <package>",
    headers: ["cols.item", "cols.description"],
    conceptKey: "residual.concept",
    keepKey: "residual.keep",
  },
  {
    id: "logs",
    icon: ScrollText,
    labelKey: "system.logs.title",
    titleKey: "system.logs.title",
    descriptionKey: "app.tabs.logs.description",
    admin: true,
    command: "journalctl --vacuum-time=7d",
    headers: ["cols.item", "cols.retention"],
    conceptKey: "categories.logs.concept",
    keepKey: "categories.logs.keep",
  },
  {
    id: "kernels",
    icon: Cpu,
    labelKey: "kernels.title",
    titleKey: "kernels.title",
    descriptionKey: "kernels.description",
    admin: true,
    command: "apt-get purge -y -- linux-image-<version> linux-headers-<version>",
    headers: ["kernels.colVersion", "kernels.colPackages", "cols.state"],
    conceptKey: "kernels.concept",
    keepKey: "kernels.keep",
  },
  {
    id: "snapshots",
    icon: Camera,
    labelKey: "snapshots.title",
    titleKey: "snapshots.title",
    descriptionKey: "snapshots.description",
    admin: true,
    command: "timeshift --delete --snapshot <name> | snapper -c <config> delete <number>",
    headers: ["cols.item", "cols.description", "snapshots.colSource"],
    conceptKey: "snapshots.concept",
    keepKey: "snapshots.keep",
  },
  {
    id: "drivers",
    icon: Plug,
    labelKey: "drivers.title",
    titleKey: "drivers.title",
    descriptionKey: "drivers.description",
    admin: true,
    command: "apt-get purge -y -- <package>",
    headers: ["cols.item", "cols.description", "drivers.colVendor"],
    conceptKey: "drivers.concept",
    keepKey: "drivers.keep",
  },
  {
    id: "firmware",
    icon: Microchip,
    labelKey: "firmware.title",
    titleKey: "firmware.title",
    descriptionKey: "firmware.description",
    admin: true,
    command: "rm -rf /lib/firmware/<family>",
    headers: ["cols.item", "firmware.colPackage", "firmware.colRemoval"],
    conceptKey: "firmware.concept",
    keepKey: "firmware.keep",
  },
  {
    id: "languages",
    icon: Languages,
    labelKey: "languages.title",
    titleKey: "languages.title",
    descriptionKey: "languages.description",
    admin: true,
    command: "rm -rf /usr/share/{locale,help,man}/<language>",
    headers: ["languages.colLanguage", "languages.colCodes", "languages.colContent"],
    conceptKey: "languages.concept",
    keepKey: "languages.keep",
  },
  {
    id: "docs",
    icon: BookOpen,
    labelKey: "docs.title",
    titleKey: "docs.title",
    descriptionKey: "docs.description",
    admin: true,
    command: "rm -rf /usr/share/doc/<package>/*",
    headers: ["cols.item", "cols.path", "docs.colKeeps"],
    conceptKey: "docs.concept",
    keepKey: "docs.keep",
  },
];

export function categoryById(id: CategoryId): CategoryDef {
  return CATEGORIES.find((c) => c.id === id)!;
}

// The command shown to the user; the journal one follows its retention.
export function commandFor(def: CategoryDef, policy: JournalPolicy): string {
  return def.id === "logs" ? journalCommand(policy) : def.command;
}

type Translate = (id: string, options?: { values?: Record<string, string | number> }) => string;

export function cellText(cell: Cell | undefined, t: Translate, locale?: string | null): string {
  if (!cell) return "";
  if (cell.key) return t(cell.key, cell.values ? { values: cell.values } : undefined);
  if (cell.keys) return cell.keys.map((k) => t(k)).join(" + ");
  if (cell.language) {
    try {
      const name = new Intl.DisplayNames([locale ?? "en"], { type: "language" }).of(cell.language);
      if (name) return name.charAt(0).toLocaleUpperCase(locale ?? "en") + name.slice(1);
    } catch {
      // An unknown code falls back to the code itself.
    }
    return cell.language;
  }
  return cell.text ?? "";
}

export function bytesOf(rows: Row[], risk?: Risk): number {
  return rows.filter((r) => !risk || r.risk === risk).reduce((sum, r) => sum + r.sizeBytes, 0);
}

export function itemsOf(rows: Row[]): number {
  return rows.reduce((sum, r) => sum + (r.count ?? 1), 0);
}
