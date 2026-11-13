// Privileged operations. The app never hands a shell script to root: the
// frontend sends a list of named operations, this module validates them on
// the user side, writes them as JSON to a private file and re-invokes this
// same binary under pkexec as `--run-ops <file>`. The root side validates
// every operation again before running it (the file is only a request) and
// executes each one with a fixed argv, never through a shell. Several
// operations travel in one pkexec call, so a batch asks for the password once.
//
// The root side prints one `@@op <index> ok|fail` line per operation so the
// frontend can tell which categories really succeeded.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::hardware::PKG_MODULES;
use super::jobs::{failed_job, start_job};
use super::packages::{command_exists, installed_packages};
use super::{disk, docs, drivers, firmware, kernels, languages, orphans, other_apps, residual, snapshots};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JournalMode {
    Time,
    Size,
}

#[derive(Serialize, Deserialize)]
pub struct SnapItem {
    pub name: String,
    pub revision: String,
}

#[derive(Serialize, Deserialize)]
pub struct DkmsItem {
    pub module: String,
    pub version: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Op {
    AptClean,
    // File names (not paths) of .deb files in /var/cache/apt/archives.
    AptCacheDebs { names: Vec<String> },
    // Time: days to keep. Size: megabytes to keep.
    JournalVacuum { mode: JournalMode, value: u32 },
    DeleteTemp { paths: Vec<String> },
    FlatpakUnused { refs: Vec<String> },
    SnapRemove { items: Vec<SnapItem> },
    // Whole application snaps; the privileged side checks each one is an
    // installed application and not a base or content snap.
    SnapApps { names: Vec<String> },
    AptRemove { names: Vec<String> },
    // Packages apt autoremove would remove; the privileged side asks apt
    // again and refuses any name that is not one of them.
    OrphanPackages { names: Vec<String> },
    // Packages removed but with their configuration left (dpkg state rc);
    // the privileged side asks dpkg again and refuses anything else.
    PurgeResidual { names: Vec<String> },
    // The operations below touch kernels, drivers or firmware. Their requests
    // are checked again against the live system by the privileged side.
    KernelPurge { packages: Vec<String> },
    OrphanDirs { paths: Vec<String> },
    DriverPurge { packages: Vec<String> },
    DkmsRemove { items: Vec<DkmsItem> },
    FirmwareRemove { families: Vec<String> },
    // Language codes (es, ca...) and documentation ids (info, pkg:nodejs...).
    // The privileged side finds the paths itself; the request never names one.
    LanguageRemove { codes: Vec<String> },
    DocsRemove { ids: Vec<String> },
    // Lists the Timeshift and Snapper snapshots: one `@@snapshot <json>` line
    // for each, printed for the window to read.
    SnapshotList,
    // Snapshots the tool itself lists now; never the newest of a set.
    SnapshotDelete { items: Vec<snapshots::SnapshotRef> },
    // A snapshot made now, with a short plain comment.
    SnapshotCreate { tool: String, config: String, comment: String },
    // Reads the space each entry of a folder takes, as root, so the folders
    // a user cannot open are counted. It only reads: one `@@disk <json>` line.
    DiskList { path: String },
}

// Names of packages, modules and firmware families: a leading letter or
// digit, then letters, digits and . _ + - only.
fn safe_name(value: &str) -> bool {
    starts_alphanumeric(value) && value.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

fn needs_initramfs(op: &Op) -> bool {
    matches!(op, Op::DriverPurge { .. } | Op::DkmsRemove { .. } | Op::FirmwareRemove { .. })
}

const MAX_OPS_FILE_BYTES: u64 = 1_000_000;

fn starts_alphanumeric(value: &str) -> bool {
    value.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
}

// A path here is deleted as root, so only direct children of /tmp and
// /var/tmp are accepted: no traversal (`..`), no deeper paths, no symlinks,
// and never the directories themselves.
fn validate_temp_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(format!("Not an absolute path: {raw}"));
    }
    let parent = path.parent().ok_or_else(|| format!("No parent directory: {raw}"))?;
    if parent != Path::new("/tmp") && parent != Path::new("/var/tmp") {
        return Err(format!("Refusing to delete outside /tmp or /var/tmp: {raw}"));
    }
    let name = path.file_name().ok_or_else(|| format!("Invalid path: {raw}"))?.to_string_lossy();
    // Same names the scanner never lists: live session sockets and private
    // directories of running services.
    let skipped = name.starts_with("systemd-private-")
        || name.starts_with("snap-private-")
        || [".X11-unix", ".ICE-unix", ".XIM-unix", ".font-unix", ".Test-unix"].contains(&name.as_ref());
    if skipped {
        return Err(format!("Refusing to delete a protected entry: {raw}"));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("Cannot read {raw}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("Refusing to delete a symbolic link: {raw}"));
    }
    Ok(path.to_path_buf())
}

// A cached package is named by its file name only: letters, digits and
// . _ + ~ % : - (apt writes the epoch colon as %3a), ending in .deb, never a
// path and never starting with a dash.
fn valid_deb_name(name: &str) -> bool {
    name.ends_with(".deb")
        && starts_alphanumeric(name)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._+~%:-".contains(c))
}

pub fn validate(op: &Op) -> Result<(), String> {
    match op {
        Op::AptClean => Ok(()),
        Op::AptCacheDebs { names } => {
            if names.is_empty() {
                return Err("No files given".to_string());
            }
            match names.iter().find(|n| !valid_deb_name(n)) {
                Some(bad) => Err(format!("Invalid package file: {bad}")),
                None => Ok(()),
            }
        }
        Op::JournalVacuum { mode, value } => {
            let allowed = match mode {
                JournalMode::Time => (1..=365).contains(value),
                JournalMode::Size => (16..=10240).contains(value),
            };
            if allowed {
                Ok(())
            } else {
                Err(format!("Journal retention out of range: {value}"))
            }
        }
        Op::DeleteTemp { paths } => {
            if paths.is_empty() {
                return Err("No paths given".to_string());
            }
            paths.iter().try_for_each(|p| validate_temp_path(p).map(|_| ()))
        }
        Op::FlatpakUnused { refs } => {
            if refs.is_empty() {
                return Err("No refs given".to_string());
            }
            // Whitelist: Flatpak refs only use these characters, and a leading
            // letter or digit keeps a ref from being read as an option.
            match refs.iter().find(|r| !starts_alphanumeric(r) || !r.chars().all(|c| c.is_ascii_alphanumeric() || "./-_".contains(c))) {
                Some(bad) => Err(format!("Invalid Flatpak ref: {bad}")),
                None => Ok(()),
            }
        }
        Op::SnapApps { names } => {
            if names.is_empty() {
                return Err("No snaps given".to_string());
            }
            match names.iter().find(|n| !starts_alphanumeric(n) || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')) {
                Some(bad) => Err(format!("Invalid snap name: {bad}")),
                None => Ok(()),
            }
        }
        Op::SnapRemove { items } => {
            if items.is_empty() {
                return Err("No revisions given".to_string());
            }
            for item in items {
                let name_ok = starts_alphanumeric(&item.name) && item.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                let revision_ok = !item.revision.is_empty() && item.revision.chars().all(|c| c.is_ascii_digit());
                if !name_ok || !revision_ok {
                    return Err(format!("Invalid snap revision: {} {}", item.name, item.revision));
                }
            }
            Ok(())
        }
        Op::AptRemove { names } | Op::OrphanPackages { names } | Op::PurgeResidual { names } => {
            if names.is_empty() {
                return Err("No packages given".to_string());
            }
            match names.iter().find(|n| !starts_alphanumeric(n) || !n.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))) {
                Some(bad) => Err(format!("Invalid package name: {bad}")),
                None => Ok(()),
            }
        }
        Op::KernelPurge { packages } | Op::DriverPurge { packages } => {
            if packages.is_empty() {
                return Err("No packages given".to_string());
            }
            match packages.iter().find(|p| !safe_name(p)) {
                Some(bad) => Err(format!("Invalid package name: {bad}")),
                None => Ok(()),
            }
        }
        Op::OrphanDirs { paths } => {
            if paths.is_empty() {
                return Err("No paths given".to_string());
            }
            match paths.iter().find(|p| !(p.starts_with("/lib/modules/") || p.starts_with("/usr/src/")) || p.contains("..")) {
                Some(bad) => Err(format!("Not an orphan directory path: {bad}")),
                None => Ok(()),
            }
        }
        Op::DkmsRemove { items } => {
            if items.is_empty() {
                return Err("No DKMS modules given".to_string());
            }
            match items.iter().find(|i| !safe_name(&i.module) || !safe_name(&i.version)) {
                Some(bad) => Err(format!("Invalid DKMS module: {}/{}", bad.module, bad.version)),
                None => Ok(()),
            }
        }
        Op::FirmwareRemove { families } => {
            if families.is_empty() {
                return Err("No firmware families given".to_string());
            }
            match families.iter().find(|f| !safe_name(f)) {
                Some(bad) => Err(format!("Invalid firmware family: {bad}")),
                None => Ok(()),
            }
        }
        Op::LanguageRemove { codes } => {
            if codes.is_empty() {
                return Err("No languages given".to_string());
            }
            match codes.iter().find(|c| !(2..=3).contains(&c.len()) || !c.chars().all(|ch| ch.is_ascii_lowercase())) {
                Some(bad) => Err(format!("Not a language code: {bad}")),
                None => Ok(()),
            }
        }
        Op::DocsRemove { ids } => {
            if ids.is_empty() {
                return Err("No documentation given".to_string());
            }
            match ids.iter().find(|id| id.len() > 200 || !id.chars().all(|c| c.is_ascii_alphanumeric() || ":+._-".contains(c))) {
                Some(bad) => Err(format!("Invalid documentation id: {bad}")),
                None => Ok(()),
            }
        }
        Op::SnapshotList => Ok(()),
        Op::DiskList { path } => disk::validate_path(path),
        Op::SnapshotDelete { items } => {
            if items.is_empty() {
                return Err("No snapshots given".to_string());
            }
            items.iter().try_for_each(snapshots::validate_ref)
        }
        Op::SnapshotCreate { tool, config, comment } => snapshots::validate_create(tool, config, comment),
    }
}

// Checks against the live system, only done where the request is about to
// run (as root): the user side only needs the syntax check above.
fn validate_system(op: &Op) -> Result<(), String> {
    match op {
        Op::KernelPurge { packages } => kernels::validate_kernel_packages(packages),
        Op::OrphanDirs { paths } => kernels::validate_orphan_dirs(paths),
        Op::OrphanPackages { names } => orphans::validate_orphans(names),
        Op::PurgeResidual { names } => residual::validate_residual(names),
        Op::SnapApps { names } => other_apps::validate_snap_apps(names),
        Op::DriverPurge { packages } => drivers::validate_driver_packages(packages),
        Op::DkmsRemove { items } => drivers::validate_dkms(&items.iter().map(|i| (i.module.clone(), i.version.clone())).collect::<Vec<_>>()),
        Op::FirmwareRemove { families } => firmware::validate_firmware_families(families),
        Op::LanguageRemove { codes } => languages::validate_languages(codes),
        Op::DocsRemove { ids } => docs::validate_docs(ids),
        Op::SnapshotDelete { items } => snapshots::validate_delete(items),
        _ => Ok(()),
    }
}

fn run_command(program: &str, args: &[String]) -> bool {
    match Command::new(program).args(args).status() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("soplos-sys-cleaner: could not run {program}: {e}");
            false
        }
    }
}

// apt must never stop to ask a question: there is no terminal.
fn run_apt(args: &[&str]) -> bool {
    match Command::new("apt-get").env("DEBIAN_FRONTEND", "noninteractive").args(args).status() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("soplos-sys-cleaner: could not run apt-get: {e}");
            false
        }
    }
}

fn purge_packages(packages: &[String]) -> bool {
    let mut args = vec!["purge", "-y", "--"];
    args.extend(packages.iter().map(|p| p.as_str()));
    run_apt(&args)
}

fn remove_dir(path: &Path) -> bool {
    match std::fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            eprintln!("soplos-sys-cleaner: could not delete {}: {e}", path.display());
            false
        }
    }
}

// Rebuilt once at the end of a batch, never while another operation of the
// same batch is running (2.x corrupted its protocol by overlapping a rebuild
// with an autoremove). Debian-family systems use dracut or initramfs-tools,
// and both are run through `update-initramfs -u -k all` when it is there: on
// Soplos it is dracut's own wrapper, and unlike `dracut -f` it does every
// kernel and runs the hooks of /etc/initramfs/post-update.d, which copy the
// new initramfs to the EFI partition when the system boots with
// systemd-boot. `dracut` alone is only the fallback.
fn rebuild_initramfs() -> bool {
    if command_exists("update-initramfs") {
        run_command("update-initramfs", &["-u".to_string(), "-k".to_string(), "all".to_string()])
    } else if command_exists("dracut") {
        run_command("dracut", &["--regenerate-all".to_string(), "-f".to_string()])
    } else {
        true
    }
}

// After purging a driver, drop the module lines it left in /etc/modules and
// /etc/modules-load.d, which would otherwise raise errors at every boot.
// Commented entries ("# vboxguest") are matched too.
fn clean_module_references(purged: &[String]) {
    let installed = installed_packages();
    let mut modules: Vec<&str> = Vec::new();
    for (package, mods) in PKG_MODULES {
        if purged.iter().any(|p| p == package) && !installed.contains_key(*package) {
            modules.extend(mods.iter().copied());
        }
    }
    if modules.is_empty() {
        return;
    }
    let mut files = vec![PathBuf::from("/etc/modules")];
    if let Ok(entries) = std::fs::read_dir("/etc/modules-load.d") {
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()) == Some("conf")));
    }
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let mut changed = false;
        let mut kept: Vec<&str> = Vec::new();
        for line in text.lines() {
            let name = line.trim().trim_start_matches('#').trim();
            if modules.contains(&name) {
                changed = true;
            } else {
                kept.push(line);
            }
        }
        if changed {
            let _ = std::fs::write(&file, kept.join("\n") + "\n");
        }
    }
}

fn remove_dkms(items: &[DkmsItem]) -> bool {
    let mut all_ok = true;
    for item in items {
        let spec = format!("{}/{}", item.module, item.version);
        if command_exists("dkms") {
            run_command("dkms", &["remove".to_string(), spec.clone(), "--all".to_string()]);
        }
        // dkms may be absent or leave the tree behind.
        all_ok &= remove_dir(&Path::new("/var/lib/dkms").join(&item.module).join(&item.version));
    }
    run_command("depmod", &["-a".to_string()]);
    all_ok
}

fn remove_firmware(families: &[String]) -> bool {
    let (packages, dirs) = firmware::firmware_plan(families);
    let mut all_ok = true;
    if !packages.is_empty() {
        all_ok &= purge_packages(&packages);
    }
    for dir in dirs {
        all_ok &= remove_dir(&dir);
    }
    all_ok
}

fn delete_cached_debs(names: &[String]) -> bool {
    let mut all_ok = true;
    for name in names {
        let path = Path::new(super::apt_cache::ARCHIVES_DIR).join(name);
        // Only a regular file: a link planted in the directory is refused.
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => {
                eprintln!("soplos-sys-cleaner: not a regular file: {name}");
                all_ok = false;
                continue;
            }
            // Already gone is fine.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("soplos-sys-cleaner: cannot read {name}: {e}");
                all_ok = false;
                continue;
            }
        }
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("soplos-sys-cleaner: could not delete {name}: {e}");
                all_ok = false;
            }
        }
    }
    all_ok
}

fn delete_temp(paths: &[String]) -> bool {
    let mut all_ok = true;
    for raw in paths {
        let path = match validate_temp_path(raw) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("soplos-sys-cleaner: {e}");
                all_ok = false;
                continue;
            }
        };
        let is_dir = std::fs::symlink_metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
        let result = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = result {
            // Already gone is fine; anything else is a real failure.
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("soplos-sys-cleaner: could not delete {raw}: {e}");
                all_ok = false;
            }
        }
    }
    all_ok
}

fn execute(op: &Op) -> bool {
    match op {
        Op::AptClean => run_command("apt-get", &["clean".to_string()]),
        Op::AptCacheDebs { names } => delete_cached_debs(names),
        Op::JournalVacuum { mode, value } => {
            let flag = match mode {
                JournalMode::Time => format!("--vacuum-time={value}d"),
                JournalMode::Size => format!("--vacuum-size={value}M"),
            };
            run_command("journalctl", &[flag])
        }
        Op::DeleteTemp { paths } => delete_temp(paths),
        Op::FlatpakUnused { refs } => {
            let mut args = vec!["uninstall".to_string(), "--system".to_string(), "-y".to_string(), "--noninteractive".to_string()];
            args.extend(refs.iter().cloned());
            run_command("flatpak", &args)
        }
        Op::SnapApps { names } => names.iter().fold(true, |ok, name| run_command("snap", &["remove".to_string(), name.clone()]) && ok),
        Op::SnapRemove { items } => {
            let mut all_ok = true;
            for item in items {
                let args = vec!["remove".to_string(), item.name.clone(), format!("--revision={}", item.revision)];
                all_ok &= run_command("snap", &args);
            }
            all_ok
        }
        Op::AptRemove { names } | Op::OrphanPackages { names } => {
            let mut args = vec!["remove", "-y", "--"];
            args.extend(names.iter().map(|n| n.as_str()));
            run_apt(&args)
        }
        Op::PurgeResidual { names } => purge_packages(names),
        Op::KernelPurge { packages } => purge_packages(packages),
        Op::OrphanDirs { paths } => paths.iter().fold(true, |ok, p| remove_dir(Path::new(p)) && ok),
        Op::DriverPurge { packages } => {
            let ok = purge_packages(packages);
            if ok {
                clean_module_references(packages);
            }
            ok
        }
        Op::DkmsRemove { items } => remove_dkms(items),
        Op::FirmwareRemove { families } => remove_firmware(families),
        Op::LanguageRemove { codes } => languages::remove_languages(codes),
        Op::DocsRemove { ids } => docs::remove_docs(ids),
        Op::SnapshotList => snapshots::print_list(),
        Op::DiskList { path } => disk::print_listing(path),
        Op::SnapshotDelete { items } => snapshots::delete(items),
        Op::SnapshotCreate { tool, config, comment } => snapshots::create(tool, config, comment),
    }
}

// What a purge script must not be allowed to take with it. The link that
// tells systemd which login screen to start and the file that names the
// default one: the purge script of a display manager that another one
// replaced can delete them, and the next boot has no login screen. And the
// GRUB defaults: the purge script of grub-efi-amd64 deletes
// /etc/default/grub even when another GRUB package still uses it. Their state
// is noted before a batch and put back afterwards if the batch removed them.
const DISPLAY_MANAGER_LINK: &str = "/etc/systemd/system/display-manager.service";
const GUARDED_FILES: &[&str] = &["/etc/X11/default-display-manager", "/etc/default/grub"];

struct GuardedState {
    link_target: Option<PathBuf>,
    files: Vec<(PathBuf, Vec<u8>)>,
}

fn capture_guards(link: &Path, files: &[&Path]) -> GuardedState {
    GuardedState {
        link_target: std::fs::read_link(link).ok(),
        files: files.iter().filter_map(|f| std::fs::read(f).ok().map(|content| (f.to_path_buf(), content))).collect(),
    }
}

// True when something had to be put back.
fn restore_guards(state: &GuardedState, link: &Path) -> bool {
    let mut restored = false;
    if let Some(target) = &state.link_target {
        // symlink_metadata: a link to a missing file still counts as there.
        if std::fs::symlink_metadata(link).is_err() && std::os::unix::fs::symlink(target, link).is_ok() {
            eprintln!("soplos-sys-cleaner: restored {} -> {}", link.display(), target.display());
            restored = true;
        }
    }
    for (file, content) in &state.files {
        if !file.exists() && std::fs::write(file, content).is_ok() {
            eprintln!("soplos-sys-cleaner: restored {}", file.display());
            restored = true;
        }
    }
    restored
}

/// Root side, called from main() before Tauri starts when pkexec re-invokes
/// this binary as `--run-ops <file>`. Returns the process exit code: 0 only
/// when every operation succeeded.
pub fn run_ops_file(path: &str) -> i32 {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => {
            eprintln!("soplos-sys-cleaner: cannot read the operations file: {e}");
            return 1;
        }
    };
    // Only a regular file written by the user who authenticated is honoured.
    if !meta.file_type().is_file() || meta.len() > MAX_OPS_FILE_BYTES {
        eprintln!("soplos-sys-cleaner: refusing the operations file");
        return 1;
    }
    if let Ok(uid) = std::env::var("PKEXEC_UID") {
        if meta.uid().to_string() != uid {
            eprintln!("soplos-sys-cleaner: the operations file does not belong to the requesting user");
            return 1;
        }
    }

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("soplos-sys-cleaner: cannot read the operations file: {e}");
            return 1;
        }
    };
    let _ = std::fs::remove_file(path);

    let ops: Vec<Op> = match serde_json::from_str(&text) {
        Ok(ops) => ops,
        Err(e) => {
            eprintln!("soplos-sys-cleaner: invalid operations: {e}");
            return 1;
        }
    };

    let guarded_paths: Vec<&Path> = GUARDED_FILES.iter().map(|f| Path::new(*f)).collect();
    let guards = capture_guards(Path::new(DISPLAY_MANAGER_LINK), &guarded_paths);

    // Every request is validated, in the live system too, before it runs.
    let mut results: Vec<bool> = Vec::with_capacity(ops.len());
    for (index, op) in ops.iter().enumerate() {
        let ok = match validate(op).and_then(|()| validate_system(op)) {
            Ok(()) => execute(op),
            Err(e) => {
                eprintln!("soplos-sys-cleaner: refused operation {index}: {e}");
                false
            }
        };
        results.push(ok);
    }

    // One initramfs rebuild for the whole batch, after everything else. If it
    // fails, the operations that needed it are reported as failed so the
    // problem is not hidden behind a green result.
    let needed: Vec<usize> = ops.iter().enumerate().filter(|(i, op)| results[*i] && needs_initramfs(op)).map(|(i, _)| i).collect();
    if !needed.is_empty() && !rebuild_initramfs() {
        eprintln!("soplos-sys-cleaner: rebuilding the initramfs failed");
        for index in needed {
            results[index] = false;
        }
    }

    // Whatever the batch removed, the login screen and the GRUB defaults stay.
    if restore_guards(&guards, Path::new(DISPLAY_MANAGER_LINK)) {
        run_command("systemctl", &["daemon-reload".to_string()]);
    }

    for (index, ok) in results.iter().enumerate() {
        println!("@@op {index} {}", if *ok { "ok" } else { "fail" });
    }
    let _ = std::io::stdout().flush();
    if results.iter().all(|ok| *ok) {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_op(path: &str) -> Op {
        Op::DeleteTemp { paths: vec![path.to_string()] }
    }

    #[test]
    fn temp_paths_outside_direct_children_are_refused() {
        assert!(validate(&temp_op("/tmp/../etc/passwd")).is_err());
        assert!(validate(&temp_op("/etc/passwd")).is_err());
        assert!(validate(&temp_op("/tmp")).is_err());
        assert!(validate(&temp_op("/var/tmp")).is_err());
        assert!(validate(&temp_op("/tmp/a/b")).is_err());
        assert!(validate(&temp_op("relative/path")).is_err());
        assert!(validate(&temp_op("/tmp/.X11-unix")).is_err());
        assert!(validate(&temp_op("/tmp/systemd-private-abc")).is_err());
        assert!(validate(&Op::DeleteTemp { paths: vec![] }).is_err());
    }

    #[test]
    fn temp_file_with_shell_syntax_in_its_name_is_deleted_literally() {
        let path = format!("/tmp/soplos-test-$(id)-{}", std::process::id());
        std::fs::write(&path, b"x").unwrap();
        assert!(validate(&temp_op(&path)).is_ok());
        assert!(delete_temp(&[path.clone()]));
        assert!(!Path::new(&path).exists());
    }

    #[test]
    fn symbolic_links_are_refused() {
        let link = format!("/tmp/soplos-test-link-{}", std::process::id());
        std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();
        assert!(validate(&temp_op(&link)).is_err());
        std::fs::remove_file(&link).unwrap();
    }

    #[test]
    fn package_and_ref_names_are_whitelisted() {
        assert!(validate(&Op::AptRemove { names: vec!["libreoffice-writer".into(), "g++".into()] }).is_ok());
        assert!(validate(&Op::AptRemove { names: vec!["-y".into()] }).is_err());
        assert!(validate(&Op::AptRemove { names: vec!["a;b".into()] }).is_err());
        assert!(validate(&Op::AptRemove { names: vec![] }).is_err());
        assert!(validate(&Op::AptCacheDebs { names: vec!["libc6_2.41-1_amd64.deb".into(), "vim_2%3a9.1.0-1_amd64.deb".into()] }).is_ok());
        assert!(validate(&Op::AptCacheDebs { names: vec!["../etc/passwd.deb".into()] }).is_err());
        assert!(validate(&Op::AptCacheDebs { names: vec!["-rf.deb".into()] }).is_err());
        assert!(validate(&Op::AptCacheDebs { names: vec!["a.deb;b".into()] }).is_err());
        assert!(validate(&Op::AptCacheDebs { names: vec!["notes.txt".into()] }).is_err());
        assert!(validate(&Op::AptCacheDebs { names: vec![] }).is_err());
        assert!(validate(&Op::OrphanPackages { names: vec!["kitty-doc".into(), "libgegl-0.4-0t64".into()] }).is_ok());
        assert!(validate(&Op::OrphanPackages { names: vec!["-y".into()] }).is_err());
        assert!(validate(&Op::OrphanPackages { names: vec![] }).is_err());
        assert!(validate(&Op::PurgeResidual { names: vec!["brave-browser".into(), "kitty-doc".into()] }).is_ok());
        assert!(validate(&Op::PurgeResidual { names: vec!["-y".into()] }).is_err());
        assert!(validate(&Op::PurgeResidual { names: vec!["a;b".into()] }).is_err());
        assert!(validate(&Op::PurgeResidual { names: vec![] }).is_err());
        assert!(validate(&Op::SnapApps { names: vec!["firefox".into(), "code".into()] }).is_ok());
        assert!(validate(&Op::SnapApps { names: vec!["$(id)".into()] }).is_err());
        assert!(validate(&Op::SnapApps { names: vec!["-y".into()] }).is_err());
        assert!(validate(&Op::SnapApps { names: vec![] }).is_err());
        assert!(validate(&Op::FlatpakUnused { refs: vec!["org.kde.Platform/x86_64/5.15-23.08".into()] }).is_ok());
        assert!(validate(&Op::FlatpakUnused { refs: vec!["--all".into()] }).is_err());
        assert!(validate(&Op::FlatpakUnused { refs: vec!["a b".into()] }).is_err());
        assert!(validate(&Op::SnapRemove { items: vec![SnapItem { name: "core22".into(), revision: "1033".into() }] }).is_ok());
        assert!(validate(&Op::SnapRemove { items: vec![SnapItem { name: "core22".into(), revision: "10;3".into() }] }).is_err());
        assert!(validate(&Op::SnapRemove { items: vec![SnapItem { name: "$(id)".into(), revision: "1".into() }] }).is_err());
    }

    #[test]
    fn the_login_screen_link_is_put_back_when_a_batch_removes_it() {
        let dir = std::env::temp_dir().join(format!("sc-dm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("display-manager.service");
        let default_file = dir.join("default-display-manager");
        let grub = dir.join("grub");
        std::os::unix::fs::symlink("/usr/lib/systemd/system/plasmalogin.service", &link).unwrap();
        std::fs::write(&default_file, "/usr/bin/plasmalogin\n").unwrap();
        std::fs::write(&grub, "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet splash\"\n").unwrap();
        let state = capture_guards(&link, &[default_file.as_path(), grub.as_path()]);

        // Untouched: nothing to put back.
        assert!(!restore_guards(&state, &link));

        // A purge script removes all three.
        std::fs::remove_file(&link).unwrap();
        std::fs::remove_file(&default_file).unwrap();
        std::fs::remove_file(&grub).unwrap();
        assert!(restore_guards(&state, &link));
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("/usr/lib/systemd/system/plasmalogin.service"));
        assert_eq!(std::fs::read_to_string(&default_file).unwrap(), "/usr/bin/plasmalogin\n");
        assert_eq!(std::fs::read_to_string(&grub).unwrap(), "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet splash\"\n");

        // A file the batch only changed is not overwritten with the old copy.
        std::fs::write(&grub, "changed\n").unwrap();
        assert!(!restore_guards(&state, &link));
        assert_eq!(std::fs::read_to_string(&grub).unwrap(), "changed\n");

        // A system that had none of them is not given any.
        let none = capture_guards(&dir.join("missing-link"), &[dir.join("missing-file").as_path()]);
        assert!(!restore_guards(&none, &dir.join("missing-link")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kernel_driver_and_firmware_requests_are_checked_for_syntax() {
        assert!(validate(&Op::KernelPurge { packages: vec!["linux-image-6.1.0-9-amd64".into()] }).is_ok());
        assert!(validate(&Op::KernelPurge { packages: vec!["-y".into()] }).is_err());
        assert!(validate(&Op::KernelPurge { packages: vec![] }).is_err());
        assert!(validate(&Op::DriverPurge { packages: vec!["nvidia-driver".into(), "a;b".into()] }).is_err());
        assert!(validate(&Op::OrphanDirs { paths: vec!["/lib/modules/6.1.0-9-amd64".into()] }).is_ok());
        assert!(validate(&Op::OrphanDirs { paths: vec!["/lib/modules/../../etc".into()] }).is_err());
        assert!(validate(&Op::OrphanDirs { paths: vec!["/etc".into()] }).is_err());
        assert!(validate(&Op::DkmsRemove { items: vec![DkmsItem { module: "nvidia".into(), version: "580.126.20".into() }] }).is_ok());
        assert!(validate(&Op::DkmsRemove { items: vec![DkmsItem { module: "../x".into(), version: "1".into() }] }).is_err());
        assert!(validate(&Op::FirmwareRemove { families: vec!["rtl_bt".into()] }).is_ok());
        assert!(validate(&Op::FirmwareRemove { families: vec!["../etc".into()] }).is_err());
    }

    #[test]
    fn language_and_documentation_requests_are_checked_for_syntax() {
        assert!(validate(&Op::LanguageRemove { codes: vec!["de".into(), "pt".into()] }).is_ok());
        assert!(validate(&Op::LanguageRemove { codes: vec!["../etc".into()] }).is_err());
        assert!(validate(&Op::LanguageRemove { codes: vec!["ES".into()] }).is_err());
        assert!(validate(&Op::LanguageRemove { codes: vec![] }).is_err());
        assert!(validate(&Op::DocsRemove { ids: vec!["info".into(), "pkg:nodejs".into()] }).is_ok());
        assert!(validate(&Op::DocsRemove { ids: vec!["pkg:a/b".into()] }).is_err());
        assert!(validate(&Op::DocsRemove { ids: vec!["a b".into()] }).is_err());
    }

    #[test]
    fn protected_kernels_are_refused_against_the_live_system() {
        // The running kernel can never be purged, whatever the request says.
        let running = super::super::packages::running_kernel();
        let request = vec![format!("linux-image-{running}")];
        assert!(kernels::validate_kernel_packages(&request).is_err());
        // Metapackages are not kernel version packages.
        assert!(kernels::validate_kernel_packages(&["linux-image-amd64".to_string()]).is_err());
        // Anything outside the kernel packages is refused.
        assert!(kernels::validate_kernel_packages(&["libc6".to_string()]).is_err());
    }

    #[test]
    fn journal_retention_is_range_checked() {
        assert!(validate(&Op::JournalVacuum { mode: JournalMode::Time, value: 7 }).is_ok());
        assert!(validate(&Op::JournalVacuum { mode: JournalMode::Time, value: 0 }).is_err());
        assert!(validate(&Op::JournalVacuum { mode: JournalMode::Time, value: 4000 }).is_err());
        assert!(validate(&Op::JournalVacuum { mode: JournalMode::Size, value: 250 }).is_ok());
        assert!(validate(&Op::JournalVacuum { mode: JournalMode::Size, value: 1 }).is_err());
    }

    #[test]
    fn operations_round_trip_through_json() {
        let json = r#"[{"op":"apt-clean"},{"op":"journal-vacuum","mode":"time","value":7},{"op":"snap-remove","items":[{"name":"core22","revision":"1033"}]}]"#;
        let ops: Vec<Op> = serde_json::from_str(json).unwrap();
        assert_eq!(ops.len(), 3);
        assert_eq!(serde_json::to_string(&ops).unwrap(), json);
        assert!(serde_json::from_str::<Vec<Op>>(r#"[{"op":"run-script","script":"id"}]"#).is_err());
    }
}

static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(1);

// The request file lives in the user's runtime directory when there is one
// (private to the user), and is created with create_new and mode 0600 so
// nobody can pre-create or read it.
fn write_ops_file(json: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let id = NEXT_FILE_ID.fetch_add(1, Ordering::SeqCst);
    let path = dir.join(format!("soplos-sys-cleaner-ops-{}-{id}-{nanos}.json", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(path)
}

/// Starts one pkexec job that runs every given operation, in order.
#[tauri::command]
pub fn run_privileged_ops(ops: Vec<Op>) -> u64 {
    if ops.is_empty() {
        return failed_job("No operations given".to_string());
    }
    if let Some(error) = ops.iter().find_map(|op| validate(op).err()) {
        return failed_job(error);
    }
    // The polkit action is tied to the advanced binary, which also hosts the
    // helper. The basic program elevates through it and never runs as root.
    let Some(helper) = super::launcher::elevation_binary_path() else {
        return failed_job("The advanced version is required to run privileged operations".to_string());
    };
    let json = match serde_json::to_string(&ops) {
        Ok(json) => json,
        Err(e) => return failed_job(format!("Could not encode the operations: {e}")),
    };
    match write_ops_file(&json) {
        Ok(path) => start_job("pkexec", vec![helper.to_string_lossy().into_owned(), "--run-ops".to_string(), path.to_string_lossy().into_owned()]),
        Err(e) => failed_job(format!("Could not prepare the operations file: {e}")),
    }
}
