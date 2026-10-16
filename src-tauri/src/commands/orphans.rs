// Orphaned packages: what `apt autoremove` would remove, ported from 2.x's
// scanner/cache.py (get_autoremove_packages). The list comes from apt's own
// simulation, never from a guess, and the privileged side asks apt again
// before removing anything, so a request can only name packages apt itself
// considers unneeded.

use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;

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

/// The privileged side's check: every requested package must be one apt
/// would itself remove now.
pub fn validate_orphans(names: &[String]) -> Result<(), String> {
    let orphans: HashSet<String> = autoremove_names()?.into_iter().collect();
    match names.iter().find(|n| !orphans.contains(*n)) {
        Some(other) => Err(format!("Not an orphaned package: {other}")),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn scan_orphan_packages() -> Vec<OrphanPackage> {
    tauri::async_runtime::spawn_blocking(scan_orphan_packages_blocking).await.unwrap_or_default()
}

fn scan_orphan_packages_blocking() -> Vec<OrphanPackage> {
    let Ok(names) = autoremove_names() else { return Vec::new() };
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
}
