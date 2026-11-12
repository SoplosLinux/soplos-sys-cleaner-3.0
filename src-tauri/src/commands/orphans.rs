// Orphaned packages: what `apt autoremove` would remove, ported from 2.x's
// scanner/cache.py (get_autoremove_packages). The list comes from apt's own
// simulation, never from a guess, and the privileged side asks apt again
// before removing anything, so a request can only name packages apt itself
// considers unneeded.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// Where a repository's signing key lives, and where apt keeps its sources.
const KEY_DIRS: [&str; 2] = ["/usr/share/keyrings", "/etc/apt/keyrings"];
const SOURCES_FILE: &str = "/etc/apt/sources.list";
const SOURCES_DIR: &str = "/etc/apt/sources.list.d";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrphanPackage {
    pub name: String,
    pub summary: String,
    pub size_bytes: u64,
}

// The names in the "Remv name [version]" lines of `apt-get -s autoremove`.
fn parse_autoremove(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.starts_with("Remv "))
        .filter_map(|l| l.split_whitespace().nth(1).map(|n| n.to_string()))
        .collect()
}

fn autoremove_names() -> Result<Vec<String>, String> {
    let output = Command::new("apt-get")
        .env("LC_ALL", "C")
        .args(["-s", "autoremove"])
        .output()
        .map_err(|e| format!("apt-get: {e}"))?;
    if !output.status.success() {
        return Err("apt-get could not simulate the removal".to_string());
    }
    Ok(parse_autoremove(&String::from_utf8_lossy(&output.stdout)))
}

// The text of every configured apt source: sources.list and each file in
// sources.list.d, in either format (one-line or deb822).
fn apt_sources_text() -> String {
    let mut files = vec![PathBuf::from(SOURCES_FILE)];
    if let Ok(read) = std::fs::read_dir(SOURCES_DIR) {
        files.extend(read.flatten().map(|entry| entry.path()));
    }
    files.iter().filter_map(|path| std::fs::read_to_string(path).ok()).collect::<Vec<_>>().join("\n")
}

// The key files in a `dpkg -L` listing: files of the key folders that apt
// reads as keyrings.
fn key_files(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|path| {
            KEY_DIRS.iter().any(|dir| path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/')))
                && [".gpg", ".asc", ".pgp"].iter().any(|ext| path.ends_with(ext))
        })
        .map(str::to_string)
        .collect()
}

// Whether a source names this key, by its full path or by its file name (both
// forms are valid in a `Signed-By:` line or a `signed-by=` option).
fn source_names_key(sources: &str, key_path: &str) -> bool {
    let name = Path::new(key_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    sources.contains(key_path) || (!name.is_empty() && sources.contains(&name))
}

// The packages that carry the key of a configured repository. apt sees no
// package that needs them, so it lists them as removable, but taking them
// away leaves the source pointing at a key that is gone and every
// `apt update` fails to verify that repository.
fn keys_in_use(sources: &str, packages: &[String]) -> HashSet<String> {
    let mut found = HashSet::new();
    for package in packages {
        let Ok(output) = Command::new("dpkg-query").args(["-L", package]).output() else { continue };
        if !output.status.success() {
            continue;
        }
        let listing = String::from_utf8_lossy(&output.stdout);
        if key_files(&listing).iter().any(|key| source_names_key(sources, key)) {
            found.insert(package.clone());
        }
    }
    found
}

/// The privileged side's check: every requested package must be one apt
/// would itself remove now, and none may carry the key of a configured
/// repository.
pub fn validate_orphans(names: &[String]) -> Result<(), String> {
    let orphans: HashSet<String> = autoremove_names()?.into_iter().collect();
    if let Some(other) = names.iter().find(|n| !orphans.contains(*n)) {
        return Err(format!("Not an orphaned package: {other}"));
    }
    let keepers = keys_in_use(&apt_sources_text(), names);
    match names.iter().find(|n| keepers.contains(*n)) {
        Some(other) => Err(format!("Package holds the key of a configured repository: {other}")),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn scan_orphan_packages() -> Vec<OrphanPackage> {
    tauri::async_runtime::spawn_blocking(scan_orphan_packages_blocking).await.unwrap_or_default()
}

fn scan_orphan_packages_blocking() -> Vec<OrphanPackage> {
    let Ok(names) = autoremove_names() else { return Vec::new() };
    let keepers = keys_in_use(&apt_sources_text(), &names);
    let names: Vec<String> = names.into_iter().filter(|n| !keepers.contains(n)).collect();
    if names.is_empty() {
        return Vec::new();
    }
    let Ok(output) = Command::new("dpkg-query")
        .args(["-W", "-f=${Package}\t${Installed-Size}\t${binary:Summary}\n"])
        .args(&names)
        .output()
    else {
        return Vec::new();
    };
    let mut found: Vec<OrphanPackage> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                return None;
            }
            // dpkg-query reports Installed-Size in KiB.
            let size_bytes = parts[1].trim().parse::<u64>().unwrap_or(0) * 1024;
            Some(OrphanPackage { name: parts[0].trim().to_string(), summary: parts[2].trim().to_string(), size_bytes })
        })
        .collect();
    found.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.name.cmp(&b.name)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_removal_lines_are_read() {
        let output = "Reading package lists...\nThe following packages will be REMOVED:\n  kitty-doc libgegl-0.4-0t64\nRemv kitty-doc [0.48.2-1.1]\nRemv libgegl-0.4-0t64 [1:0.4.72-2]\nInst foo [1.0] (1.1 Debian:stable)\n";
        assert_eq!(parse_autoremove(output), vec!["kitty-doc".to_string(), "libgegl-0.4-0t64".to_string()]);
    }

    #[test]
    fn only_key_files_of_the_key_folders_are_read() {
        let listing = "/usr/share/keyrings\n/usr/share/keyrings/vendor-archive-keyring.gpg\n/etc/apt/keyrings/vendor.asc\n/usr/share/doc/vendor-keyring/copyright\n/usr/share/keyrings-extra/other.gpg\n/etc/apt/sources.list.d/vendor.sources\n";
        assert_eq!(key_files(listing), vec!["/usr/share/keyrings/vendor-archive-keyring.gpg".to_string(), "/etc/apt/keyrings/vendor.asc".to_string()]);
    }

    #[test]
    fn a_source_names_a_key_by_path_or_by_file_name() {
        let deb822 = "Types: deb\nURIs: https://repo.example.com\nSuites: stable\nSigned-By: /usr/share/keyrings/vendor-archive-keyring.gpg\n";
        let one_line = "deb [signed-by=vendor-two.gpg] https://repo.example.com stable main\n";
        let sources = format!("{deb822}\n{one_line}");
        assert!(source_names_key(&sources, "/usr/share/keyrings/vendor-archive-keyring.gpg"));
        assert!(source_names_key(&sources, "/etc/apt/keyrings/vendor-two.gpg"));
        assert!(!source_names_key(&sources, "/usr/share/keyrings/unrelated.gpg"));
        assert!(!source_names_key("", "/usr/share/keyrings/vendor-archive-keyring.gpg"));
    }
}
