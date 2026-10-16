// Firmware families under /lib/firmware that no hardware layer claims,
// ported from 2.x with its lessons:
// - a loaded module is not proof of hardware (see hardware.rs);
// - families owned by a generic bundle (firmware-linux-nonfree, ...) come
//   back when that package is next upgraded, so they are marked temporary;
// - a family is blocked while the driver packages of its vendor are still
//   installed: deleting firmware under a driver that is still there leaves
//   the driver broken, so the driver goes first (Drivers page).

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::hardware::{
    detect, firmware_protection_set, is_firmware_family_protected, lookup, FIRMWARE_DIR, FIRMWARE_PACKAGES, PCI_VENDOR_FIRMWARE, PCI_VENDOR_PACKAGES,
};
use super::packages::installed_packages;

const GENERIC_PACKAGES: &[&str] = &["firmware-linux-nonfree", "firmware-misc-nonfree", "firmware-linux", "firmware-linux-free"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareEntry {
    pub family: String,
    pub size_bytes: u64,
    pub owner: Option<String>,
    // The deletion only lasts until the owning package is upgraded.
    pub temporary: bool,
    // "hardware": a detection layer needs it. "driver": its driver is
    // installed. "detection": the detection tools are missing, so nothing can
    // be judged unused.
    pub protected_by: Option<String>,
}

fn families() -> Vec<String> {
    let Ok(entries) = fs::read_dir(FIRMWARE_DIR) else { return Vec::new() };
    let mut names: Vec<String> = entries.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

// One du call for every directory; "size<TAB>path" per line.
fn sizes(names: &[String]) -> HashMap<String, u64> {
    let mut sizes = HashMap::new();
    if names.is_empty() {
        return sizes;
    }
    let paths: Vec<PathBuf> = names.iter().map(|n| Path::new(FIRMWARE_DIR).join(n)).collect();
    let Ok(output) = Command::new("du").args(["-sb", "--"]).args(&paths).output() else { return sizes };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((size, path)) = line.split_once('\t') {
            if let (Ok(size), Some(name)) = (size.trim().parse::<u64>(), Path::new(path).file_name()) {
                sizes.insert(name.to_string_lossy().into_owned(), size);
            }
        }
    }
    sizes
}

// dpkg only records the resolved path of a merged-usr system (/usr/lib/...),
// so /lib/firmware paths are resolved first. Output lines are
// "owner[, owner2]: /path".
fn owners_of(paths: &[PathBuf]) -> HashMap<PathBuf, String> {
    let mut owners = HashMap::new();
    if paths.is_empty() {
        return owners;
    }
    let Ok(output) = Command::new("dpkg").arg("-S").args(paths).output() else { return owners };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((owner, path)) = line.split_once(": ") {
            let first_owner = owner.split(',').next().unwrap_or(owner).trim().split(':').next().unwrap_or("").to_string();
            owners.insert(PathBuf::from(path.trim()), first_owner);
        }
    }
    owners
}

fn firmware_owners(names: &[String]) -> HashMap<String, String> {
    let real: Vec<(String, PathBuf)> = names.iter().filter_map(|n| fs::canonicalize(Path::new(FIRMWARE_DIR).join(n)).ok().map(|p| (n.clone(), p))).collect();
    let by_dir = owners_of(&real.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>());
    let mut result: HashMap<String, String> = HashMap::new();
    let mut unresolved: Vec<(String, PathBuf)> = Vec::new();
    for (name, path) in &real {
        match by_dir.get(path) {
            Some(owner) => {
                result.insert(name.clone(), owner.clone());
            }
            None => {
                // The directory itself may not be tracked: ask about a file inside.
                if let Some(first) = fs::read_dir(path).ok().and_then(|mut it| it.next()).and_then(|e| e.ok()) {
                    unresolved.push((name.clone(), first.path()));
                }
            }
        }
    }
    let by_file = owners_of(&unresolved.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>());
    for (name, path) in unresolved {
        if let Some(owner) = by_file.get(&path) {
            result.insert(name, owner.clone());
        }
    }
    result
}

// Driver packages of the vendor whose hardware uses this firmware family.
fn driver_packages_for_family(family: &str) -> Vec<&'static str> {
    PCI_VENDOR_FIRMWARE
        .iter()
        .filter(|(_, fams)| fams.contains(&family))
        .flat_map(|(vendor, _)| lookup(PCI_VENDOR_PACKAGES, vendor).iter().copied())
        .collect()
}

fn installed_driver_package(family: &str, installed: &HashMap<String, u64>) -> Option<&'static str> {
    driver_packages_for_family(family).into_iter().find(|p| installed.contains_key(*p))
}

fn dedicated_package(family: &str) -> Option<&'static str> {
    FIRMWARE_PACKAGES.iter().find(|(f, _)| *f == family).map(|(_, p)| *p).filter(|p| !GENERIC_PACKAGES.contains(p))
}

fn scan_blocking() -> Vec<FirmwareEntry> {
    let names = families();
    let hw = detect();
    let protection = firmware_protection_set(&hw);
    let installed = installed_packages();
    let sizes = sizes(&names);
    let owners = firmware_owners(&names);

    let mut entries: Vec<FirmwareEntry> = names
        .into_iter()
        .map(|family| {
            let owner = owners.get(&family).cloned();
            let protected_by = if !hw.detection_reliable {
                Some("detection".to_string())
            } else if is_firmware_family_protected(&family, &protection) {
                Some("hardware".to_string())
            } else if installed_driver_package(&family, &installed).is_some() {
                Some("driver".to_string())
            } else {
                None
            };
            // Permanent only when nothing owns the directory, or its owner is
            // the dedicated firmware package that is purged along with it.
            // Any other owner (a generic bundle, firmware-intel-misc, ...)
            // brings the files back the next time it is upgraded.
            let temporary = match owner.as_deref() {
                None => false,
                Some(o) => dedicated_package(&family) != Some(o),
            };
            FirmwareEntry {
                size_bytes: sizes.get(&family).copied().unwrap_or(0),
                temporary,
                owner,
                protected_by,
                family,
            }
        })
        .collect();
    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    entries
}

#[tauri::command]
pub async fn scan_firmware() -> Vec<FirmwareEntry> {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or_default()
}

/// Privileged-side check. The hardware layers run again here, as root, so
/// the kernel log is readable and the protection is at its strongest.
pub fn validate_firmware_families(families: &[String]) -> Result<(), String> {
    if families.is_empty() {
        return Err("No firmware families given".to_string());
    }
    // Names first: a bad request should not cost a full hardware detection.
    for family in families {
        if family.contains('/') || family == ".." || family == "." || !Path::new(FIRMWARE_DIR).join(family).is_dir() {
            return Err(format!("Not a firmware family: {family}"));
        }
    }
    let hw = detect();
    if !hw.detection_reliable {
        return Err("Hardware detection is not available (pciutils, usbutils and kmod are required)".to_string());
    }
    let protection = firmware_protection_set(&hw);
    let installed = installed_packages();
    for family in families {
        if is_firmware_family_protected(family, &protection) {
            return Err(format!("Firmware in use by this hardware: {family}"));
        }
        if let Some(driver) = installed_driver_package(family, &installed) {
            return Err(format!("The driver {driver} is still installed; remove it first: {family}"));
        }
    }
    Ok(())
}

/// What removing these families does: the dedicated firmware packages to
/// purge (only when every family of the package that exists on disk is being
/// removed) and the directories that are then deleted by hand.
pub fn firmware_plan(families: &[String]) -> (Vec<String>, Vec<PathBuf>) {
    let installed = installed_packages();
    let selected: HashSet<&str> = families.iter().map(|s| s.as_str()).collect();
    let mut purge: Vec<String> = Vec::new();
    for family in families {
        let Some(package) = dedicated_package(family) else { continue };
        if !installed.contains_key(package) || purge.iter().any(|p| p == package) {
            continue;
        }
        let owned_on_disk: Vec<&str> = FIRMWARE_PACKAGES
            .iter()
            .filter(|(f, p)| *p == package && Path::new(FIRMWARE_DIR).join(f).is_dir())
            .map(|(f, _)| *f)
            .collect();
        if !owned_on_disk.is_empty() && owned_on_disk.iter().all(|f| selected.contains(f)) {
            purge.push(package.to_string());
        }
    }
    let dirs = families.iter().map(|f| Path::new(FIRMWARE_DIR).join(f)).collect();
    (purge, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prints the scan of this machine: cargo test dump_live_scan -- --ignored --nocapture
    #[test]
    #[ignore = "prints the live scan"]
    fn dump_live_scan() {
        for f in scan_blocking() {
            println!("{:<22} {:>12} owner={:?} temporary={} protected_by={:?}", f.family, f.size_bytes, f.owner, f.temporary, f.protected_by);
        }
    }

    #[test]
    fn intel_families_map_to_intel_driver_packages() {
        assert!(driver_packages_for_family("i915").contains(&"intel-media-va-driver"));
        assert!(driver_packages_for_family("nvidia").contains(&"nvidia-driver"));
        assert!(driver_packages_for_family("unknown-family").is_empty());
    }

    #[test]
    fn generic_bundles_are_never_dedicated_packages() {
        assert_eq!(dedicated_package("amdgpu"), Some("firmware-amd-graphics"));
        assert_eq!(dedicated_package("not-a-family"), None);
    }

    #[test]
    fn firmware_names_with_path_syntax_are_refused() {
        assert!(validate_firmware_families(&["../etc".to_string()]).is_err());
        assert!(validate_firmware_families(&["a/b".to_string()]).is_err());
        assert!(validate_firmware_families(&[]).is_err());
    }
}
