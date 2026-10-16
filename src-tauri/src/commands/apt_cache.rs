// APT's .deb download cache — ported from 2.x's scanner/cache.py (the
// simple, safe part of it: just /var/cache/apt/archives). A single
// direct-action card like Trash, not a reviewable list: individual .deb
// files aren't something worth cherry-picking, `apt-get clean` is exactly
// what every "free up disk space" guide already tells people to run.

use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AptCacheInfo {
    pub size_bytes: u64,
    pub deb_count: u32,
}

pub const ARCHIVES_DIR: &str = "/var/cache/apt/archives";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AptDeb {
    pub name: String,
    pub size_bytes: u64,
}

/// Every .deb in the cache, one entry each, biggest first.
#[tauri::command]
pub async fn scan_apt_debs() -> Vec<AptDeb> {
    tauri::async_runtime::spawn_blocking(scan_apt_debs_blocking).await.unwrap_or_default()
}

fn scan_apt_debs_blocking() -> Vec<AptDeb> {
    let Ok(entries) = std::fs::read_dir(ARCHIVES_DIR) else { return Vec::new() };
    let mut debs: Vec<AptDeb> = entries
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("deb"))
        .filter_map(|entry| {
            // symlink_metadata: a link is not a file to list or delete.
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            if !meta.is_file() {
                return None;
            }
            Some(AptDeb { name: entry.file_name().to_string_lossy().into_owned(), size_bytes: meta.len() })
        })
        .collect();
    debs.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.name.cmp(&b.name)));
    debs
}

#[tauri::command]
pub async fn scan_apt_cache() -> AptCacheInfo {
    tauri::async_runtime::spawn_blocking(scan_apt_cache_blocking).await.unwrap_or(AptCacheInfo { size_bytes: 0, deb_count: 0 })
}

fn scan_apt_cache_blocking() -> AptCacheInfo {
    let Ok(entries) = std::fs::read_dir("/var/cache/apt/archives") else {
        return AptCacheInfo { size_bytes: 0, deb_count: 0 };
    };
    let mut size_bytes = 0u64;
    let mut deb_count = 0u32;
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|s| s.to_str()) == Some("deb") {
            if let Ok(meta) = entry.metadata() {
                size_bytes += meta.len();
                deb_count += 1;
            }
        }
    }
    AptCacheInfo { size_bytes, deb_count }
}
