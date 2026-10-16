// Trash scanning/emptying — ported from 2.x's scanner/trash.py. Entirely
// user-scope (the user's own ~/.local/share/Trash), never needs root.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

fn trash_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local/share/Trash"))
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrashInfo {
    pub size_bytes: u64,
    pub item_count: u64,
}

// `du` (not a manual walk) matches what 2.x used and correctly accounts for
// actual disk usage rather than apparent file size, same reasoning as the
// cache scanner below.
fn dir_size_bytes(path: &PathBuf) -> u64 {
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
pub async fn scan_trash() -> TrashInfo {
    tauri::async_runtime::spawn_blocking(scan_trash_blocking).await.unwrap_or_default()
}

fn scan_trash_blocking() -> TrashInfo {
    let Some(trash) = trash_dir() else { return TrashInfo::default() };
    let files_dir = trash.join("files");
    if !files_dir.is_dir() {
        return TrashInfo::default();
    }

    let item_count = std::fs::read_dir(&files_dir).map(|entries| entries.count() as u64).unwrap_or(0);
    if item_count == 0 {
        return TrashInfo::default();
    }

    let size_bytes = dir_size_bytes(&files_dir) + dir_size_bytes(&trash.join("info"));
    TrashInfo { size_bytes, item_count }
}

#[tauri::command]
pub async fn empty_trash() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(empty_trash_blocking).await.map_err(|e| e.to_string())?
}

fn empty_trash_blocking() -> Result<(), String> {
    let trash = trash_dir().ok_or("HOME is not set")?;
    // Clears the contents of files/ and info/, not the Trash directory
    // structure itself — matches 2.x, and keeps the directories (and
    // whatever permissions/mount the DE expects there) intact.
    for sub in ["files", "info"] {
        let dir = trash.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let result = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
            if let Err(e) = result {
                return Err(format!("Could not remove {}: {e}", path.display()));
            }
        }
    }
    Ok(())
}
