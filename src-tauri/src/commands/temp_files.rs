// /tmp and /var/tmp — ported from 2.x's scanner/temp_files.py. Only
// top-level entries are scanned (not walked recursively) to avoid hanging
// on a large build tree someone left behind; directories are sized with
// `du` and reported as a single entry. Entries younger than a day are
// flagged as recent — a fresh temp file may belong to something still
// running. Active session sockets/dirs are excluded by name so this can
// never suggest deleting something X11, snapd or systemd still needs.

use serde::Serialize;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TempEntry {
    pub path: String,
    pub size_bytes: u64,
    pub is_dir: bool,
    // Modified within the last day.
    pub recent: bool,
}

const SKIP_DIRS: &[&str] = &[".X11-unix", ".ICE-unix", ".XIM-unix", ".font-unix", ".Test-unix"];
const SKIP_PREFIXES: &[&str] = &["systemd-private-", "snap-private-"];
const RECENT: Duration = Duration::from_secs(24 * 60 * 60);

fn dir_size_bytes(path: &Path) -> u64 {
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
pub async fn scan_temp_files() -> Vec<TempEntry> {
    tauri::async_runtime::spawn_blocking(scan_temp_files_blocking).await.unwrap_or_default()
}

fn scan_temp_files_blocking() -> Vec<TempEntry> {
    let now = SystemTime::now();
    let mut out = Vec::new();

    for base in ["/tmp", "/var/tmp"] {
        let Ok(entries) = std::fs::read_dir(base) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIP_DIRS.contains(&name.as_str()) || SKIP_PREFIXES.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            let file_type = meta.file_type();
            // Symlinks, sockets and named pipes are never meaningfully
            // "sized" and are almost always something still in active use.
            if file_type.is_symlink() || file_type.is_socket() || file_type.is_fifo() {
                continue;
            }
            // Everything is listed. Entries younger than a day are flagged:
            // they are the most likely to belong to something still running.
            let recent = meta
                .modified()
                .map(|modified| now.duration_since(modified).unwrap_or_default() < RECENT)
                .unwrap_or(true);
            let is_dir = file_type.is_dir();
            let size_bytes = if is_dir { dir_size_bytes(&entry.path()) } else { meta.len() };
            out.push(TempEntry { path: entry.path().to_string_lossy().into_owned(), size_bytes, is_dir, recent });
        }
    }

    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    out
}
