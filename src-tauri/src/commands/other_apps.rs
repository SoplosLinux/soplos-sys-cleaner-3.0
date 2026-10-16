// Flatpak, Snap and AppImage apps for the Installed apps page, next to the
// APT ones. Only applications: Flatpak runtimes and old Snap revisions have
// their own pages, and Snap base or content snaps are never listed.
//
// AppImages are the ones Soplos AppImage Manager keeps: the file in
// ~/AppImages, its launcher in ~/.local/share/applications and its icon in
// ~/AppImages/.icons.

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ---- Flatpak ---------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlatpakApp {
    pub reference: String,
    pub name: String,
    pub origin: String,
    // Installed for this user only: removed without privileges.
    pub user: bool,
    pub size_bytes: u64,
}

// "1.4 MB", "88.8 MB", "1.2 GB": flatpak's decimal sizes.
fn parse_size(text: &str) -> u64 {
    let mut parts = text.split_whitespace();
    let Some(number) = parts.next().and_then(|n| n.replace(',', ".").parse::<f64>().ok()) else { return 0 };
    let factor = match parts.next().unwrap_or("bytes") {
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => 1.0,
    };
    (number * factor) as u64
}

fn valid_flatpak_ref(reference: &str) -> bool {
    reference.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && reference.chars().all(|c| c.is_ascii_alphanumeric() || "./-_".contains(c))
}

fn parse_flatpak_apps(output: &str) -> Vec<FlatpakApp> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 5 || !valid_flatpak_ref(fields[0]) {
                return None;
            }
            Some(FlatpakApp {
                reference: fields[0].to_string(),
                name: fields[1].trim().to_string(),
                origin: fields[2].trim().to_string(),
                user: fields[3].trim() == "user",
                size_bytes: parse_size(fields[4]),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn scan_flatpak_apps() -> Vec<FlatpakApp> {
    tauri::async_runtime::spawn_blocking(|| {
        let Ok(output) = Command::new("flatpak")
            .args(["list", "--app", "--columns=ref,name,origin,installation,size"])
            .env("LC_ALL", "C")
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        parse_flatpak_apps(&String::from_utf8_lossy(&output.stdout))
    })
    .await
    .unwrap_or_default()
}

// ---- Snap ------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SnapApp {
    pub name: String,
    pub version: String,
    // Needs --classic to be installed again.
    pub classic: bool,
    pub size_bytes: u64,
}

// An application snap declares at least one `apps:` entry that is not a
// background service. Bases, the snap daemon and content snaps (themes,
// runtimes) declare none, and Mesa's runtime only declares a monitor daemon.
// Anything that cannot be read is not an application.
fn is_app_snap(name: &str) -> bool {
    let Ok(yaml) = fs::read_to_string(format!("/snap/{name}/current/meta/snap.yaml")) else { return false };
    snap_yaml_is_app(&yaml)
}

fn snap_yaml_is_app(yaml: &str) -> bool {
    let kind = yaml.lines().find_map(|l| l.strip_prefix("type:")).map(|v| v.trim().to_string());
    if kind.is_some_and(|k| k != "app") {
        return false;
    }
    // Walk the apps: block. Entries are keys indented two spaces; their
    // settings (daemon:, command:...) are indented four.
    let mut in_apps = false;
    let mut entry_is_daemon = false;
    let mut entries = 0;
    let mut daemons = 0;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_apps = line.starts_with("apps:");
            continue;
        }
        if !in_apps {
            continue;
        }
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            entries += 1;
            entry_is_daemon = false;
        } else if line.starts_with("    daemon:") && !entry_is_daemon {
            entry_is_daemon = true;
            daemons += 1;
        }
    }
    entries > daemons
}

fn valid_snap_name(name: &str) -> bool {
    !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// "Name  Version  Rev  Tracking  Publisher  Notes" rows of `snap list`.
fn parse_snap_list(output: &str) -> Vec<(String, String, String, bool)> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 || !valid_snap_name(parts[0]) {
                return None;
            }
            let classic = parts.iter().skip(5).any(|p| p.split(',').any(|n| n == "classic"));
            Some((parts[0].to_string(), parts[1].to_string(), parts[2].to_string(), classic))
        })
        .collect()
}

fn installed_snaps() -> Vec<(String, String, String, bool)> {
    let Ok(output) = Command::new("snap").arg("list").env("LC_ALL", "C").output() else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    parse_snap_list(&String::from_utf8_lossy(&output.stdout))
}

/// The privileged side's check: every requested snap must be an installed
/// application snap.
pub fn validate_snap_apps(names: &[String]) -> Result<(), String> {
    let apps: HashSet<String> = installed_snaps().into_iter().map(|(name, ..)| name).filter(|n| is_app_snap(n)).collect();
    match names.iter().find(|n| !apps.contains(*n)) {
        Some(other) => Err(format!("Not an installed application snap: {other}")),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn scan_snap_apps() -> Vec<SnapApp> {
    tauri::async_runtime::spawn_blocking(|| {
        installed_snaps()
            .into_iter()
            .filter(|(name, ..)| is_app_snap(name))
            .map(|(name, version, revision, classic)| {
                let size_bytes = fs::metadata(format!("/var/lib/snapd/snaps/{name}_{revision}.snap")).map(|m| m.len()).unwrap_or(0);
                SnapApp { name, version, classic, size_bytes }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

// ---- AppImage --------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppImageApp {
    // File name inside ~/AppImages.
    pub file: String,
    pub name: String,
    pub version: String,
    pub update_url: String,
    pub size_bytes: u64,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn appimages_dir() -> Option<PathBuf> {
    home().map(|h| h.join("AppImages"))
}

fn desktop_dir() -> Option<PathBuf> {
    home().map(|h| h.join(".local/share/applications"))
}

fn is_appimage_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".appimage")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._+~-".contains(c))
}

struct Launcher {
    path: PathBuf,
    // The AppImage the launcher starts, as a file name in ~/AppImages.
    file: String,
    name: String,
    version: String,
    update_url: String,
    icon: String,
}

// The launchers of ~/.local/share/applications whose Exec or TryExec points
// at a file directly inside ~/AppImages, the same rule AppImage Manager uses.
fn launchers() -> Vec<Launcher> {
    let (Some(apps), Some(dir)) = (appimages_dir(), desktop_dir()) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("desktop"))
        .filter_map(|e| {
            let text = fs::read_to_string(e.path()).ok()?;
            let get = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().to_string()).unwrap_or_default();
            let exec = {
                let try_exec = get("TryExec=");
                if try_exec.is_empty() { get("Exec=") } else { try_exec }
            };
            // Exec may carry arguments or quotes; the program is the first word.
            let program = PathBuf::from(exec.split_whitespace().next()?.trim_matches('"'));
            if program.parent()? != apps {
                return None;
            }
            let file = program.file_name()?.to_string_lossy().into_owned();
            Some(Launcher {
                path: e.path(),
                file,
                name: get("Name="),
                version: get("X-AppImage-Version="),
                update_url: get("X-AppImage-UpdateUrl="),
                icon: get("Icon="),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn scan_appimages() -> Vec<AppImageApp> {
    tauri::async_runtime::spawn_blocking(|| {
        let Some(dir) = appimages_dir() else { return Vec::new() };
        let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
        let launchers = launchers();
        entries
            .flatten()
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                if !is_appimage_name(&file) {
                    return None;
                }
                let meta = fs::symlink_metadata(e.path()).ok()?;
                if !meta.is_file() {
                    return None;
                }
                let launcher = launchers.iter().find(|l| l.file == file);
                let stem = file.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| file.clone());
                Some(AppImageApp {
                    name: launcher.map(|l| l.name.clone()).filter(|n| !n.is_empty()).unwrap_or(stem),
                    version: launcher.map(|l| l.version.clone()).unwrap_or_default(),
                    update_url: launcher.map(|l| l.update_url.clone()).unwrap_or_default(),
                    size_bytes: meta.len(),
                    file,
                })
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Removes AppImages by file name: the file, the launcher that starts it and
/// the icon AppImage Manager copied, nothing else. Names only, never paths.
#[tauri::command]
pub async fn remove_appimages(files: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = appimages_dir().ok_or_else(|| "Cannot find the AppImages folder".to_string())?;
        let icons = dir.join(".icons");
        let launchers = launchers();
        let mut failed = Vec::new();
        for file in &files {
            if !is_appimage_name(file) {
                return Err(format!("Invalid AppImage name: {file}"));
            }
            let path = dir.join(file);
            // Only a regular file: a link in the folder is left alone.
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => {
                    failed.push(file.clone());
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    failed.push(file.clone());
                    continue;
                }
            }
            if let Err(e) = fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    failed.push(file.clone());
                    continue;
                }
            }
            for launcher in launchers.iter().filter(|l| &l.file == file) {
                let _ = fs::remove_file(&launcher.path);
                let icon = PathBuf::from(&launcher.icon);
                if icon.parent() == Some(icons.as_path()) {
                    let _ = fs::remove_file(icon);
                }
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("Could not remove: {}", failed.join(", ")))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_list_is_read_with_its_sizes() {
        let output = "com.protonvpn.www/x86_64/stable\tProton VPN\tflathub\tuser\t88.8 MB\norg.gimp.GIMP/x86_64/stable\tGIMP\tflathub\tsystem\t1.2 GB\n--bad/x\tX\tflathub\tuser\t1 MB\n";
        let apps = parse_flatpak_apps(output);
        assert_eq!(apps.len(), 2);
        assert!(apps[0].user && apps[0].size_bytes == 88_800_000);
        assert!(!apps[1].user && apps[1].size_bytes == 1_200_000_000);
    }

    #[test]
    fn snap_list_marks_classic_snaps() {
        let output = "Name   Version  Rev   Tracking       Publisher  Notes\ncode   1.95     123   latest/stable  vscode✓    classic\ncore22 20240111 1380  latest/stable  canonical✓ base\nfirefox 131.0   5187  latest/stable  mozilla✓   -\n";
        let snaps = parse_snap_list(output);
        assert_eq!(snaps.len(), 3);
        assert!(snaps.iter().find(|s| s.0 == "code").unwrap().3);
        assert!(!snaps.iter().find(|s| s.0 == "firefox").unwrap().3);
    }

    #[test]
    fn only_snaps_with_a_real_app_count_as_applications() {
        // GIMP: one normal app.
        assert!(snap_yaml_is_app("name: gimp\napps:\n  gimp:\n    command: usr/bin/gimp\n    plugs:\n    - desktop\nconfinement: strict\n"));
        // Mesa runtime: only a monitor daemon.
        assert!(!snap_yaml_is_app("name: mesa-2404\napps:\n  component-monitor:\n    command: bin/component-monitor\n    restart-delay: 3s\n    daemon: simple\nconfinement: strict\n"));
        // A service next to a normal command is still an application.
        assert!(snap_yaml_is_app("apps:\n  svc:\n    daemon: simple\n  cli:\n    command: bin/cli\n"));
        // Bases, snapd and content snaps.
        assert!(!snap_yaml_is_app("name: core24\ntype: base\n"));
        assert!(!snap_yaml_is_app("name: snapd\ntype: snapd\napps:\n  snap:\n    command: x\n"));
        assert!(!snap_yaml_is_app("name: gtk-common-themes\nslots:\n  gtk-3-themes:\n    interface: content\n"));
    }

    #[test]
    fn appimage_names_are_plain_file_names() {
        assert!(is_appimage_name("Affinity-3.0.2-x86_64.AppImage"));
        assert!(!is_appimage_name("../x.AppImage"));
        assert!(!is_appimage_name(".hidden.AppImage"));
        assert!(!is_appimage_name("a b.AppImage"));
        assert!(!is_appimage_name("notes.txt"));
    }
}
