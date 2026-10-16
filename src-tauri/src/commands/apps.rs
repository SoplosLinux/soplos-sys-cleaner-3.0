// Manually installed apps — ported from 2.x's scanner/installed_apps.py.
// "Manually installed" (apt-mark showmanual) excludes anything only pulled
// in as a dependency, and kernel/firmware packages are filtered out since
// those get their own dedicated cards (not built yet) rather than showing
// up here as generic "apps".

use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub summary: String,
    pub size_bytes: u64,
    pub has_desktop: bool,
    // A base package of the system: left out of the reinstall script.
    pub base: bool,
}

fn manually_installed() -> HashSet<String> {
    Command::new("apt-mark")
        .arg("showmanual")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

// Packages that own a .desktop file in /usr/share/applications — used only
// to badge entries that have a visible launcher, same as 2.x.
fn desktop_packages() -> HashSet<String> {
    let dir = std::path::Path::new("/usr/share/applications");
    let Ok(entries) = std::fs::read_dir(dir) else { return HashSet::new() };
    let files: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            (path.extension().and_then(|s| s.to_str()) == Some("desktop")).then(|| path.to_string_lossy().into_owned())
        })
        .collect();
    if files.is_empty() {
        return HashSet::new();
    }
    let Ok(output) = Command::new("dpkg").arg("-S").args(&files).output() else { return HashSet::new() };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(':').map(|(pkg, _)| pkg.trim().to_string()))
        .collect()
}

// Packages better handled by their own dedicated card (kernels, firmware)
// than listed here as a generic "app".
fn is_system_package(name: &str, section: &str) -> bool {
    let section = section.to_lowercase();
    if section.contains("kernel") || section.contains("firmware") {
        return true;
    }
    if name.starts_with("firmware-") || name == "linux-firmware" || name == "linux-firmware-nonfree" {
        return true;
    }
    ["linux-image-", "linux-headers-", "linux-modules-", "linux-kbuild-", "linux-libc-dev", "linux-compiler-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[tauri::command]
pub async fn scan_installed_apps() -> Vec<AppInfo> {
    tauri::async_runtime::spawn_blocking(scan_installed_apps_blocking).await.unwrap_or_default()
}

fn scan_installed_apps_blocking() -> Vec<AppInfo> {
    let manual = manually_installed();
    if manual.is_empty() {
        return Vec::new();
    }
    let desktop = desktop_packages();

    let Ok(output) = Command::new("dpkg-query")
        .args(["-W", "-f=${Package}\t${Installed-Size}\t${binary:Summary}\t${Section}\t${Priority}\t${Essential}\n"])
        .output()
    else {
        return Vec::new();
    };

    let mut apps: Vec<AppInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(6, '\t').collect();
            if parts.len() < 6 {
                return None;
            }
            let name = parts[0].trim();
            if !manual.contains(name) || is_system_package(name, parts[3].trim()) {
                return None;
            }
            // dpkg-query reports Installed-Size in KiB.
            let size_bytes = parts[1].trim().parse::<u64>().unwrap_or(0) * 1024;
            Some(AppInfo {
                name: name.to_string(),
                summary: parts[2].trim().to_string(),
                size_bytes,
                has_desktop: desktop.contains(name),
                // Part of every installation: essential, or priority
                // required or important.
                base: parts[5].trim() == "yes" || matches!(parts[4].trim(), "required" | "important"),
            })
        })
        .collect();

    apps.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    apps
}
