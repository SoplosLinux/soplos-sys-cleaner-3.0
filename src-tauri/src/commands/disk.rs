// Disk usage by folder. It only reads: nothing here deletes, moves or changes
// a file, because what to free is the user's decision, made in the folders
// themselves. A listing gives the entries of one folder with the space each
// one takes, the largest first; the window opens a folder to see inside it.
//
// Sizes are the space the files take on the disk (allocated blocks), which is
// what a cleanup frees, not their apparent length. A file with several hard
// links counts once, and a scan never crosses into another filesystem, so a
// mounted partition is listed (marked) but not added to its parent.
//
// A user scan only reads what the user can read; the folders it could not
// open are marked, and the window can ask for the same listing as
// administrator (the `disk-list` operation of privileged.rs).

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

// Folders of the root that are not storage: the kernel and the runtime.
const VIRTUAL: &[&str] = &["/proc", "/sys", "/dev", "/run"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    // "dir", "file" or "link".
    pub kind: String,
    // A folder that is another filesystem (a mounted partition or a virtual one).
    pub other_fs: bool,
    // Something inside could not be read, so the size is a minimum.
    pub unreadable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskListing {
    pub path: String,
    pub entries: Vec<DiskEntry>,
    // The folder itself: everything the entries add up to.
    pub total_bytes: u64,
    // The filesystem the folder is on.
    pub fs_total_bytes: u64,
    pub fs_free_bytes: u64,
}

fn allocated(meta: &fs::Metadata) -> u64 {
    meta.blocks() * 512
}

// The space a folder takes with everything under it, on its own filesystem.
fn dir_size(path: &Path, dev: u64, seen: &Mutex<HashSet<(u64, u64)>>, unreadable: &AtomicBool) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        unreadable.store(true, Ordering::Relaxed);
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        // The entry itself, not what a link points to.
        let Ok(meta) = entry.metadata() else {
            unreadable.store(true, Ordering::Relaxed);
            continue;
        };
        if meta.is_dir() {
            if meta.dev() != dev {
                continue;
            }
            total += allocated(&meta) + dir_size(&entry.path(), dev, seen, unreadable);
        } else {
            if meta.is_file() && meta.nlink() > 1 && !seen.lock().unwrap_or_else(|e| e.into_inner()).insert((meta.dev(), meta.ino())) {
                continue;
            }
            total += allocated(&meta);
        }
    }
    total
}

fn measure(path: &Path, name: String, parent_dev: u64, seen: &Mutex<HashSet<(u64, u64)>>) -> Option<DiskEntry> {
    let meta = fs::symlink_metadata(path).ok()?;
    let text = path.to_string_lossy().into_owned();
    let (kind, size_bytes, other_fs, unreadable) = if meta.is_dir() {
        let other_fs = meta.dev() != parent_dev || VIRTUAL.contains(&text.as_str());
        if VIRTUAL.contains(&text.as_str()) {
            ("dir", 0, true, false)
        } else {
            let unreadable = AtomicBool::new(false);
            let size = allocated(&meta) + dir_size(path, meta.dev(), seen, &unreadable);
            ("dir", size, other_fs, unreadable.load(Ordering::Relaxed))
        }
    } else if meta.file_type().is_symlink() {
        ("link", allocated(&meta), false, false)
    } else {
        let counted = meta.nlink() <= 1 || seen.lock().unwrap_or_else(|e| e.into_inner()).insert((meta.dev(), meta.ino()));
        ("file", if counted { allocated(&meta) } else { 0 }, false, false)
    };
    Some(DiskEntry { name, path: text, size_bytes, kind: kind.to_string(), other_fs, unreadable })
}

fn filesystem_space(path: &Path) -> (u64, u64) {
    let Ok(output) = Command::new("df").args(["-B1", "--output=size,avail"]).arg(path).output() else { return (0, 0) };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .nth(1)
        .and_then(|line| {
            let mut parts = line.split_whitespace().map(|p| p.parse::<u64>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .unwrap_or((0, 0))
}

/// The entries of one folder with their sizes, the largest first.
pub fn list(path: &Path) -> Result<DiskListing, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err("Not a folder".to_string());
    }
    let parent_dev = meta.dev();
    let children: Vec<(String, PathBuf)> = fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();

    let seen = Mutex::new(HashSet::new());
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<DiskEntry>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(2, 8);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((name, child)) = children.get(index) else { break };
                if let Some(entry) = measure(child, name.clone(), parent_dev, &seen) {
                    found.lock().unwrap_or_else(|e| e.into_inner()).push(entry);
                }
            });
        }
    });
    let mut entries = found.into_inner().unwrap_or_default();
    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.name.cmp(&b.name)));
    let total_bytes = allocated(&meta) + entries.iter().filter(|e| !e.other_fs).map(|e| e.size_bytes).sum::<u64>();
    let (fs_total_bytes, fs_free_bytes) = filesystem_space(path);
    Ok(DiskListing { path: path.to_string_lossy().into_owned(), entries, total_bytes, fs_total_bytes, fs_free_bytes })
}

/// The folder a request names, with links resolved. A user scan never leaves
/// the home folder; a system scan can read any folder.
pub fn resolve(raw: &str, scope: &str) -> Result<PathBuf, String> {
    let path = canonical_request(raw)?;
    if scope == "user" {
        let home = std::env::var("HOME").map_err(|_| "No home folder".to_string())?;
        let home = fs::canonicalize(home).map_err(|e| e.to_string())?;
        if !path.starts_with(&home) {
            return Err("Outside the home folder".to_string());
        }
    }
    Ok(path)
}

// Absolute, with no `..` and no NUL, resolved through any link.
fn canonical_request(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') {
        return Err("Invalid path".to_string());
    }
    let path = Path::new(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Invalid path".to_string());
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}

/// Syntax check of a request, without touching the disk.
pub fn validate_path(raw: &str) -> Result<(), String> {
    if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') {
        return Err("Invalid path".to_string());
    }
    let path = Path::new(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Invalid path".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn scan_disk(path: String, scope: String) -> Result<DiskListing, String> {
    tauri::async_runtime::spawn_blocking(move || list(&resolve(&path, &scope)?))
        .await
        .map_err(|e| e.to_string())?
}

/// The home folder, where a user scan starts.
#[tauri::command]
pub fn disk_home() -> String {
    std::env::var("HOME").ok().and_then(|home| fs::canonicalize(home).ok()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "/".to_string())
}

/// Root side of the listing: one `@@disk <json>` line for the window to read.
pub fn print_listing(raw: &str) -> bool {
    match canonical_request(raw).and_then(|path| list(&path)) {
        Ok(listing) => match serde_json::to_string(&listing) {
            Ok(json) => {
                println!("@@disk {json}");
                true
            }
            Err(_) => false,
        },
        Err(error) => {
            eprintln!("soplos-sys-cleaner: could not list {raw}: {error}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("soplos-disk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_the_largest_first_and_counts_a_hard_link_once() {
        let dir = scratch("sizes");
        fs::create_dir_all(dir.join("big")).unwrap();
        fs::write(dir.join("big/a"), vec![1u8; 200_000]).unwrap();
        fs::hard_link(dir.join("big/a"), dir.join("big/b")).unwrap();
        fs::write(dir.join("small"), b"x").unwrap();
        let listing = list(&dir).unwrap();
        assert_eq!(listing.entries[0].name, "big");
        let big = listing.entries[0].size_bytes;
        // The two names of the file are one file on the disk.
        assert!(big >= 200_000 && big < 300_000, "{big}");
        assert_eq!(listing.entries[0].kind, "dir");
        assert!(listing.total_bytes >= big);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_link_is_not_followed() {
        let dir = scratch("links");
        std::os::unix::fs::symlink("/usr", dir.join("to-usr")).unwrap();
        let listing = list(&dir).unwrap();
        let link = listing.entries.iter().find(|e| e.name == "to-usr").unwrap();
        assert_eq!(link.kind, "link");
        assert!(link.size_bytes < 8192);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_that_climb_or_are_relative_are_refused() {
        assert!(validate_path("/home/user/../../etc").is_err());
        assert!(validate_path("relative/path").is_err());
        assert!(validate_path("").is_err());
        assert!(validate_path("/ok\0bad").is_err());
        assert!(validate_path("/var/lib").is_ok());
    }
}
