// Installed kernels, ported from 2.x's scanner/kernels.py with its lessons:
// the running kernel is compared by exact equality (6.1.0-2 is a substring of
// 6.1.0-27), -dbg packages and metapackages such as linux-image-amd64 are not
// kernels, and directories left behind in /usr/src and /lib/modules are listed
// as orphans, except modules of kernels installed by hand (those have a
// /boot/vmlinuz and no package).
//
// Protected kernels: the one running, and the newest of the others as a
// fallback. The scan marks them, and the privileged side applies the same
// rule again, so no request can purge them.

use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::packages::{dir_size_bytes, installed_packages, looks_like_kernel_version, running_kernel};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KernelEntry {
    pub version: String,
    pub packages: Vec<String>,
    pub size_bytes: u64,
    pub running: bool,
    pub reserve: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PathEntry {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KernelScan {
    pub kernels: Vec<KernelEntry>,
    pub orphan_dirs: Vec<PathEntry>,
    pub orphan_kbuild: Vec<PackageEntry>,
}

// Version strings compared by runs of digits (numerically) and runs of other
// characters, so 6.1.0-9 sorts before 6.1.0-27.
fn version_chunks(version: &str) -> Vec<(bool, String)> {
    let mut chunks: Vec<(bool, String)> = Vec::new();
    for c in version.chars() {
        let is_digit = c.is_ascii_digit();
        match chunks.last_mut() {
            Some((last_is_digit, text)) if *last_is_digit == is_digit => text.push(c),
            _ => chunks.push((is_digit, c.to_string())),
        }
    }
    chunks
}

pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let (ca, cb) = (version_chunks(a), version_chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let ordering = if x.0 && y.0 {
            let (nx, ny) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            nx.len().cmp(&ny.len()).then_with(|| nx.cmp(ny))
        } else {
            x.1.cmp(&y.1)
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    ca.len().cmp(&cb.len())
}

// "6.1.0-27-amd64" -> "6.1": what linux-kbuild-X.Y is named after.
pub fn kernel_series(version: &str) -> Option<String> {
    let mut parts = version.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("{major}.{minor}"))
}

pub struct KernelState {
    // Newest first.
    pub versions: Vec<String>,
    pub running: String,
    pub reserve: Option<String>,
}

pub fn kernel_state(installed: &HashMap<String, u64>) -> KernelState {
    let mut versions: Vec<String> = installed
        .keys()
        .filter_map(|name| name.strip_prefix("linux-image-"))
        .filter(|v| looks_like_kernel_version(v) && !v.ends_with("-dbg"))
        .map(|v| v.to_string())
        .collect();
    versions.sort_by(|a, b| version_cmp(b, a));
    let running = running_kernel();
    let reserve = versions.iter().find(|v| **v != running).cloned();
    KernelState { versions, running, reserve }
}

fn orphan_module_dir(name: &str, installed: &HashMap<String, u64>, running: &str) -> bool {
    name != running && !installed.contains_key(&format!("linux-image-{name}")) && !Path::new(&format!("/boot/vmlinuz-{name}")).exists()
}

fn orphan_header_dir(name: &str, installed: &HashMap<String, u64>) -> bool {
    name.starts_with("linux-headers-") && !installed.contains_key(name)
}

fn kbuild_is_orphan(package: &str, state: &KernelState) -> bool {
    let Some(suffix) = package.strip_prefix("linux-kbuild-") else { return false };
    let Some(series) = kernel_series(suffix) else { return false };
    !state.versions.iter().any(|v| kernel_series(v).as_deref() == Some(series.as_str()))
}

fn scan_blocking() -> KernelScan {
    let installed = installed_packages();
    let state = kernel_state(&installed);

    let kernels = state
        .versions
        .iter()
        .map(|version| {
            let mut packages = Vec::new();
            let mut size_kib = 0;
            for name in [format!("linux-image-{version}"), format!("linux-headers-{version}"), format!("linux-source-{version}")] {
                if let Some(size) = installed.get(&name) {
                    size_kib += size;
                    packages.push(name);
                }
            }
            KernelEntry {
                version: version.clone(),
                packages,
                size_bytes: size_kib * 1024,
                running: *version == state.running,
                reserve: state.reserve.as_deref() == Some(version.as_str()),
            }
        })
        .collect();

    let mut orphan_dirs = Vec::new();
    for (base, is_orphan) in [("/lib/modules", 0), ("/usr/src", 1)] {
        let Ok(entries) = fs::read_dir(base) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            if !meta.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let orphan = if is_orphan == 0 { orphan_module_dir(&name, &installed, &state.running) } else { orphan_header_dir(&name, &installed) };
            if orphan {
                orphan_dirs.push(PathEntry { size_bytes: dir_size_bytes(&path), path: path.to_string_lossy().into_owned() });
            }
        }
    }
    orphan_dirs.sort_by(|a, b| a.path.cmp(&b.path));

    let mut orphan_kbuild: Vec<PackageEntry> = installed
        .iter()
        .filter(|(name, _)| kbuild_is_orphan(name, &state))
        .map(|(name, size_kib)| PackageEntry { name: name.clone(), size_bytes: size_kib * 1024 })
        .collect();
    orphan_kbuild.sort_by(|a, b| a.name.cmp(&b.name));

    KernelScan { kernels, orphan_dirs, orphan_kbuild }
}

#[tauri::command]
pub async fn scan_kernels() -> KernelScan {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or(KernelScan { kernels: Vec::new(), orphan_dirs: Vec::new(), orphan_kbuild: Vec::new() })
}

/// Privileged-side check of a kernel purge request. Only installed
/// linux-image/headers/source packages of kernels that are neither running
/// nor the reserve, and linux-kbuild packages no installed kernel uses.
pub fn validate_kernel_packages(packages: &[String]) -> Result<(), String> {
    if packages.is_empty() {
        return Err("No packages given".to_string());
    }
    let installed = installed_packages();
    let state = kernel_state(&installed);
    for package in packages {
        if !installed.contains_key(package.as_str()) {
            return Err(format!("Not an installed package: {package}"));
        }
        if let Some(version) = ["linux-image-", "linux-headers-", "linux-source-"].iter().find_map(|prefix| package.strip_prefix(prefix)) {
            if !looks_like_kernel_version(version) || version.ends_with("-dbg") {
                return Err(format!("Not a kernel version package: {package}"));
            }
            if version == state.running {
                return Err(format!("Refusing to remove the running kernel: {package}"));
            }
            if state.reserve.as_deref() == Some(version) {
                return Err(format!("Refusing to remove the reserve kernel: {package}"));
            }
        } else if package.starts_with("linux-kbuild-") {
            if !kbuild_is_orphan(package, &state) {
                return Err(format!("A kernel still uses {package}"));
            }
        } else {
            return Err(format!("Not a kernel package: {package}"));
        }
    }
    Ok(())
}

/// Privileged-side check of orphan directory deletions.
pub fn validate_orphan_dirs(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No paths given".to_string());
    }
    let installed = installed_packages();
    let running = running_kernel();
    for raw in paths {
        let path = Path::new(raw);
        let parent = path.parent().ok_or_else(|| format!("Invalid path: {raw}"))?;
        let name = path.file_name().ok_or_else(|| format!("Invalid path: {raw}"))?.to_string_lossy().into_owned();
        let meta = fs::symlink_metadata(path).map_err(|e| format!("Cannot read {raw}: {e}"))?;
        if !meta.is_dir() {
            return Err(format!("Not a directory: {raw}"));
        }
        let allowed = if parent == Path::new("/lib/modules") {
            orphan_module_dir(&name, &installed, &running)
        } else if parent == Path::new("/usr/src") {
            orphan_header_dir(&name, &installed)
        } else {
            false
        };
        if !allowed {
            return Err(format!("Not an orphan kernel directory: {raw}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prints the scan of this machine: cargo test dump_live_scan -- --ignored --nocapture
    #[test]
    #[ignore = "prints the live scan"]
    fn dump_live_scan() {
        let scan = scan_blocking();
        for k in &scan.kernels {
            println!("kernel {} running={} reserve={} size={} packages={:?}", k.version, k.running, k.reserve, k.size_bytes, k.packages);
        }
        for d in &scan.orphan_dirs {
            println!("orphan dir {} {}", d.path, d.size_bytes);
        }
        for p in &scan.orphan_kbuild {
            println!("orphan kbuild {} {}", p.name, p.size_bytes);
        }
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(version_cmp("6.1.0-27-amd64", "6.1.0-9-amd64"), Ordering::Greater);
        assert_eq!(version_cmp("6.1.0-2-amd64", "6.1.0-27-amd64"), Ordering::Less);
        assert_eq!(version_cmp("7.2.9-soplos-bore-v4", "7.2.9-soplos-bore-v4"), Ordering::Equal);
        assert_eq!(version_cmp("7.10.0", "7.9.9"), Ordering::Greater);
    }

    #[test]
    fn series_is_major_and_minor() {
        assert_eq!(kernel_series("6.1.0-27-amd64").as_deref(), Some("6.1"));
        assert_eq!(kernel_series("7.2").as_deref(), Some("7.2"));
        assert_eq!(kernel_series("amd64"), None);
    }

    #[test]
    fn reserve_is_the_newest_other_kernel() {
        let mut installed = HashMap::new();
        for v in ["6.1.0-9-amd64", "6.1.0-27-amd64", "6.1.0-18-amd64"] {
            installed.insert(format!("linux-image-{v}"), 1);
        }
        installed.insert("linux-image-amd64".to_string(), 1);
        installed.insert("linux-image-6.1.0-27-amd64-dbg".to_string(), 1);
        let state = kernel_state(&installed);
        assert_eq!(state.versions, vec!["6.1.0-27-amd64", "6.1.0-18-amd64", "6.1.0-9-amd64"]);
    }

    #[test]
    fn kbuild_is_orphan_only_without_a_kernel_of_its_series() {
        let state = KernelState { versions: vec!["6.1.0-27-amd64".to_string()], running: "6.1.0-27-amd64".to_string(), reserve: None };
        assert!(!kbuild_is_orphan("linux-kbuild-6.1", &state));
        assert!(kbuild_is_orphan("linux-kbuild-5.10", &state));
        assert!(!kbuild_is_orphan("linux-libc-dev", &state));
    }
}
