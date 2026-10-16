// Disabled snap revisions — ported from 2.x's scanner/snap.py. Snap keeps
// old (disabled) revisions of every installed snap around for rollback by
// default; this is the same cleanup `snap list --all` + "disabled" filter
// Welcome's own clean_system() already runs system-wide, surfaced here as
// its own reviewable card instead of folded into a blanket "clean system".

use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SnapRevision {
    pub name: String,
    pub revision: String,
    pub size_bytes: u64,
}

fn snap_available() -> bool {
    Command::new("snap").arg("version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn dir_size_bytes(path: &str) -> u64 {
    Command::new("du")
        .args(["-sb", "--"])
        .arg(path)
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[tauri::command]
pub async fn scan_snap_old_revisions() -> Vec<SnapRevision> {
    tauri::async_runtime::spawn_blocking(scan_snap_old_revisions_blocking).await.unwrap_or_default()
}

fn scan_snap_old_revisions_blocking() -> Vec<SnapRevision> {
    if !snap_available() {
        return Vec::new();
    }
    let Ok(output) = Command::new("snap").args(["list", "--all"]).env("LC_ALL", "C").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    // Header row ("Name  Version  Rev  Tracking  Publisher  Notes") plus one
    // row per revision; "disabled" shows up in the Notes column for every
    // revision superseded by the currently active one.
    let candidates: Vec<(String, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter(|line| line.to_lowercase().contains("disabled"))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                return None;
            }
            Some((parts[0].to_string(), parts[2].to_string()))
        })
        .collect();

    let mut out = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .into_iter()
            .map(|(name, revision)| {
                scope.spawn(move || {
                    let size_bytes = dir_size_bytes(&format!("/snap/{name}/{revision}"));
                    SnapRevision { name, revision, size_bytes }
                })
            })
            .collect();
        for handle in handles {
            if let Ok(entry) = handle.join() {
                out.push(entry);
            }
        }
    });

    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    out
}
