// Cumulative cleanup stats, persisted locally — powers the Dashboard's
// "Space Recovered" / "Files Cleaned" / "Last Scan" / "Recent Activity" and
// the History page. Every number here comes from an action this app actually
// performed (recorded by the frontend right after the cleanup ran, with the
// bytes it measured or targeted) — nothing synthesized.
//
// Each entry is a receipt: category, kind of run (quick/full/manual),
// duration, whether it succeeded, the command and the items it covered.
// Failed cleanups are recorded too, but never add to the totals.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 2;
const MAX_ACTIVITY: usize = 1000;
const MAX_DETAILS: usize = 100;
const MAX_DETAIL_CHARS: usize = 200;

fn default_true() -> bool {
    true
}

// Fields added in schema 2 have defaults, so entries written by older
// versions still load (they show no kind, duration, command or details).
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub label: String,
    pub bytes: u64,
    pub items: u32,
    pub at_unix: u64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default = "default_true")]
    pub ok: bool,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub details: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    #[serde(default)]
    pub schema_version: u32,
    pub total_bytes_recovered: u64,
    pub total_items_cleaned: u64,
    pub last_scan_unix: Option<u64>,
    pub activity: Vec<ActivityEntry>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn stats_path() -> Option<PathBuf> {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(v) => PathBuf::from(v),
        Err(_) => PathBuf::from(std::env::var("HOME").ok()?).join(".local/share"),
    };
    Some(base.join("soplos-sys-cleaner").join("stats.json"))
}

fn load() -> Stats {
    stats_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// Written to a sibling file and renamed over the real one, so a crash or a
// power cut in the middle of saving can never leave a half-written file.
fn save(stats: &mut Stats) {
    let Some(path) = stats_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    stats.schema_version = SCHEMA_VERSION;
    let Ok(json) = serde_json::to_string_pretty(stats) else { return };
    let temp = path.with_extension("json.tmp");
    if fs::write(&temp, json).is_ok() {
        let _ = fs::rename(&temp, &path);
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// When the last successful cleanup happened, if there is one.
pub(crate) fn last_cleanup_unix() -> Option<u64> {
    load().activity.iter().find(|e| e.ok).map(|e| e.at_unix)
}

#[tauri::command]
pub fn get_stats() -> Stats {
    load()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupRecord {
    pub label: String,
    pub bytes: u64,
    pub items: u32,
    pub kind: String,
    pub duration_ms: u64,
    pub ok: bool,
    pub command: String,
    pub details: Vec<String>,
}

// Called by the frontend after a cleanup ran. Only successful cleanups add
// to the totals; a failed one is kept as a receipt with zero bytes.
#[tauri::command]
pub fn record_cleanup(record: CleanupRecord) {
    let _guard = LOCK.lock().unwrap();
    let mut stats = load();
    let (bytes, items) = if record.ok { (record.bytes, record.items) } else { (0, 0) };
    stats.total_bytes_recovered += bytes;
    stats.total_items_cleaned += items as u64;
    let details = record.details.into_iter().take(MAX_DETAILS).map(|d| d.chars().take(MAX_DETAIL_CHARS).collect()).collect();
    stats.activity.insert(
        0,
        ActivityEntry {
            label: record.label,
            bytes,
            items,
            at_unix: now_unix(),
            kind: record.kind,
            duration_ms: record.duration_ms,
            ok: record.ok,
            command: record.command,
            details,
        },
    );
    stats.activity.truncate(MAX_ACTIVITY);
    save(&mut stats);
}

#[tauri::command]
pub fn record_scan() {
    let _guard = LOCK.lock().unwrap();
    let mut stats = load();
    stats.last_scan_unix = Some(now_unix());
    save(&mut stats);
}

// Saves text the frontend built (the history as CSV) to a path the user
// picked in the system save dialog (the history as CSV, the list of installed
// apps as a reinstall script). Only .csv, .txt and .sh files are written.
#[tauri::command]
pub fn export_text_file(path: String, contents: String) -> Result<(), String> {
    let path = Path::new(&path);
    if !path.is_absolute() || !matches!(path.extension().and_then(|e| e.to_str()), Some("csv" | "txt" | "sh")) {
        return Err("Only absolute .csv, .txt or .sh paths are accepted".to_string());
    }
    fs::write(path, contents).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub mount: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

// Real filesystem usage via `df`, filtered to actual block-device mounts
// (source path starting with /dev/) — excludes tmpfs, overlay, squashfs,
// proc/sysfs and other virtual filesystems `df` also lists, which aren't
// "storage" a person would recognize on a Storage Overview.
#[tauri::command]
pub async fn scan_storage() -> Vec<DiskInfo> {
    tauri::async_runtime::spawn_blocking(scan_storage_blocking).await.unwrap_or_default()
}

fn scan_storage_blocking() -> Vec<DiskInfo> {
    let Ok(output) = std::process::Command::new("df").args(["-B1", "--output=source,target,size,used"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut disks = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let source = parts[0];
        // Loop devices are images mounted read-only (every installed snap is
        // one, always 100% full): not storage a person can free.
        if !source.starts_with("/dev/") || source.starts_with("/dev/loop") {
            continue;
        }
        let mount = parts[1].to_string();
        let Ok(total_bytes) = parts[2].parse::<u64>() else { continue };
        let Ok(used_bytes) = parts[3].parse::<u64>() else { continue };
        disks.push(DiskInfo { mount, used_bytes, total_bytes });
    }
    disks.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));
    disks
}
