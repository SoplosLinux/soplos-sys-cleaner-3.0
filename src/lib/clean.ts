import { get } from "svelte/store";
import { _, locale } from "svelte-i18n";
import { api } from "./api";
import { runJobDetailed } from "./job";
import { categoryById, commandFor, cellText, bytesOf, itemsOf, type CategoryId, type Row } from "./categories";
import { journalPolicy, type JournalPolicy } from "./journal";
import { journalDiskUsage } from "./scan";
import type { CleanKind } from "./scanTypes";

// Named operations understood by the privileged helper (commands/privileged.rs).
// The helper validates each one again before running it.
type Op =
  | { op: "apt-clean" }
  | { op: "apt-cache-debs"; names: string[] }
  | { op: "journal-vacuum"; mode: "time" | "size"; value: number }
  | { op: "delete-temp"; paths: string[] }
  | { op: "flatpak-unused"; refs: string[] }
  | { op: "snap-remove"; items: { name: string; revision: string }[] }
  | { op: "snap-apps"; names: string[] }
  | { op: "apt-remove"; names: string[] }
  | { op: "orphan-packages"; names: string[] }
  | { op: "purge-residual"; names: string[] }
  | { op: "kernel-purge"; packages: string[] }
  | { op: "orphan-dirs"; paths: string[] }
  | { op: "driver-purge"; packages: string[] }
  | { op: "dkms-remove"; items: { module: string; version: string }[] }
  | { op: "firmware-remove"; families: string[] }
  | { op: "language-remove"; codes: string[] }
  | { op: "docs-remove"; ids: string[] }
  | { op: "snapshot-list" }
  | { op: "snapshot-delete"; items: { tool: string; config: string; id: string }[] }
  | { op: "snapshot-create"; tool: string; config: string; comment: string };

export interface CleanResult {
  id: CategoryId;
  ok: boolean;
  freedBytes: number;
}

// The operations one category needs for its rows. Kernels and drivers can
// need two: packages plus directories, or packages plus DKMS builds.
function opsFor(id: CategoryId, rows: Row[], policy: JournalPolicy): Op[] {
  const ofKind = (...kinds: string[]) => rows.filter((r) => kinds.includes(r.payload?.kind ?? ""));
  const ops: Op[] = [];
  switch (id) {
    case "flatpak": {
      // Only the system installation needs the helper; the user's own is
      // cleaned without privileges (see cleanGroups).
      const system = rows.filter((r) => r.payload?.installation !== "user");
      return system.length > 0 ? [{ op: "flatpak-unused", refs: system.map((r) => r.id) }] : [];
    }
    case "snap":
      return [{ op: "snap-remove", items: rows.map((r) => ({ name: r.payload!.name, revision: r.payload!.revision })) }];
    case "apps": {
      // APT, system Flatpak and Snap apps go through the helper. User
      // Flatpaks and AppImages need no privileges (see cleanGroups).
      const apt = ofKind("apt").map((r) => r.payload!.name);
      const flatpak = ofKind("flatpak").filter((r) => r.payload!.installation !== "user").map((r) => r.payload!.reference);
      const snap = ofKind("snap").map((r) => r.payload!.name);
      if (apt.length > 0) ops.push({ op: "apt-remove", names: apt });
      if (flatpak.length > 0) ops.push({ op: "flatpak-unused", refs: flatpak });
      if (snap.length > 0) ops.push({ op: "snap-apps", names: snap });
      return ops;
    }
    case "orphans":
      return [{ op: "orphan-packages", names: rows.map((r) => r.id) }];
    case "residual":
      return [{ op: "purge-residual", names: rows.map((r) => r.id) }];
    case "temp":
      return [{ op: "delete-temp", paths: rows.map((r) => r.id) }];
    case "aptCache":
      return [{ op: "apt-cache-debs", names: rows.map((r) => r.id) }];
    case "logs":
      return [{ op: "journal-vacuum", mode: policy.mode, value: policy.value }];
    case "kernels": {
      const packages = ofKind("kernel", "kbuild").flatMap((r) => r.packages ?? []);
      const paths = ofKind("dir").map((r) => r.id);
      if (packages.length > 0) ops.push({ op: "kernel-purge", packages });
      if (paths.length > 0) ops.push({ op: "orphan-dirs", paths });
      return ops;
    }
    case "drivers": {
      const packages = ofKind("driver").flatMap((r) => r.packages ?? []);
      const items = ofKind("dkms").map((r) => ({ module: r.payload!.module, version: r.payload!.version }));
      if (packages.length > 0) ops.push({ op: "driver-purge", packages });
      if (items.length > 0) ops.push({ op: "dkms-remove", items });
      return ops;
    }
    case "firmware":
      return [{ op: "firmware-remove", families: rows.map((r) => r.id) }];
    case "languages":
      return [{ op: "language-remove", codes: rows.map((r) => r.id) }];
    case "docs":
      return [{ op: "docs-remove", ids: rows.map((r) => r.id) }];
    case "snapshots":
      return [{ op: "snapshot-delete", items: rows.map((r) => ({ tool: r.payload!.tool, config: r.payload!.config, id: r.payload!.id })) }];
    default:
      throw new Error(`Category ${id} does not run privileged operations`);
  }
}

// One pkexec call for all the operations, so the password is asked once per
// batch. The helper reports each operation as "@@op <index> ok|fail"; anything
// missing (a cancelled password prompt, a refused request) counts as failed.
async function runOps(ops: Op[]): Promise<boolean[]> {
  const oks = ops.map(() => false);
  // Nothing needs privileges: no password prompt.
  if (ops.length === 0) return oks;
  try {
    const status = await runJobDetailed("run_privileged_ops", { ops });
    for (const line of status.output.split("\n")) {
      const match = /^@@op (\d+) (ok|fail)$/.exec(line.trim());
      if (match && Number(match[1]) < oks.length) oks[Number(match[1])] = match[2] === "ok";
    }
  } catch (error) {
    console.error("Privileged operations failed", error);
  }
  return oks;
}

async function cleanUserCategory(id: CategoryId, rows: Row[]): Promise<boolean> {
  try {
    if (id === "trash") await api.invoke("empty_trash");
    else await api.invoke("delete_cache_entries", { paths: rows.map((r) => r.id) });
    return true;
  } catch (error) {
    console.error(`Cleanup failed for ${id}`, error);
    return false;
  }
}

// Cleans several categories at once. Categories that need no privileges run
// directly; all the others share one privileged batch. Every category gets a
// receipt in the history (kind of run, duration, command, items); a failed one
// is recorded as failed and frees nothing. Bytes are the bytes really freed:
// for the journal that is the measured difference, because vacuuming keeps
// part of it and the scan only estimates.
export async function cleanGroups(groups: { id: CategoryId; rows: Row[] }[], kind: CleanKind): Promise<CleanResult[]> {
  const started = Date.now();
  const results = new Map<CategoryId, { ok: boolean; freedBytes: number }>();

  for (const group of groups.filter((g) => !categoryById(g.id).admin)) {
    results.set(group.id, { ok: await cleanUserCategory(group.id, group.rows), freedBytes: bytesOf(group.rows) });
  }

  // Flatpaks installed for the user are removed without a password. Only the
  // system ones go to the privileged batch below.
  const flatpak = groups.find((g) => g.id === "flatpak");
  let flatpakUserOk = true;
  if (flatpak) {
    const refs = flatpak.rows.filter((r) => r.payload?.installation === "user").map((r) => r.id);
    if (refs.length > 0) {
      try {
        await api.invoke("flatpak_uninstall_user", { refs });
      } catch (error) {
        console.error("Flatpak cleanup failed", error);
        flatpakUserOk = false;
      }
    }
  }

  // Installed apps: the user's own Flatpaks and AppImages need no password.
  const apps = groups.find((g) => g.id === "apps");
  let appsUserOk = true;
  if (apps) {
    const refs = apps.rows.filter((r) => r.payload?.kind === "flatpak" && r.payload.installation === "user").map((r) => r.payload!.reference);
    const files = apps.rows.filter((r) => r.payload?.kind === "appimage").map((r) => r.payload!.file);
    try {
      if (refs.length > 0) await api.invoke("flatpak_uninstall_user", { refs });
    } catch (error) {
      console.error("Flatpak app removal failed", error);
      appsUserOk = false;
    }
    try {
      if (files.length > 0) await api.invoke("remove_appimages", { files });
    } catch (error) {
      console.error("AppImage removal failed", error);
      appsUserOk = false;
    }
  }

  const admin = groups.filter((g) => categoryById(g.id).admin);
  if (admin.length > 0) {
    const policy = get(journalPolicy);
    const hasLogs = admin.some((g) => g.id === "logs");
    const before = hasLogs ? await journalDiskUsage() : 0;
    const planned = admin.map((group) => ({ group, ops: opsFor(group.id, group.rows, policy) }));
    const oks = await runOps(planned.flatMap((p) => p.ops));
    const after = hasLogs ? await journalDiskUsage() : 0;
    // A category succeeded only if every one of its operations did.
    let offset = 0;
    for (const { group, ops } of planned) {
      let ok = oks.slice(offset, offset + ops.length).every(Boolean);
      // A category with no privileged operation (all user Flatpaks) is judged
      // by its unprivileged part only.
      if (ops.length === 0 && group.id !== "flatpak" && group.id !== "apps") ok = false;
      if (group.id === "flatpak") ok = ok && flatpakUserOk;
      if (group.id === "apps") ok = ok && appsUserOk;
      offset += ops.length;
      const freedBytes = group.id === "logs" ? Math.max(0, before - after) : bytesOf(group.rows);
      results.set(group.id, { ok, freedBytes });
    }
  }

  const durationMs = Date.now() - started;
  const translate = get(_);
  const currentLocale = get(locale);
  const policy = get(journalPolicy);
  const ordered: CleanResult[] = [];
  for (const group of groups) {
    const { ok, freedBytes } = results.get(group.id)!;
    // A journal cleanup that freed nothing (it was already within the
    // retention) is not worth a receipt.
    if (!(ok && group.id === "logs" && freedBytes === 0)) {
      await api.invoke("record_cleanup", {
        record: {
          label: categoryById(group.id).titleKey,
          bytes: freedBytes,
          items: itemsOf(group.rows),
          kind,
          durationMs,
          ok,
          command: commandFor(categoryById(group.id), policy),
          details: group.rows.map((r) => cellText(r.name, translate, currentLocale)),
        },
      });
    }
    ordered.push({ id: group.id, ok, freedBytes });
  }
  return ordered;
}
