// ~/.cache breakdown — ported from 2.x's scanner/user_cache.py. Entirely
// user-scope, never needs root.
//
// Some programs keep live state in ~/.cache: the IBus daemon's socket, lock
// and pid files, a browser's singleton files. Deleting that folder while the
// program runs leaves it alive but unreachable (IBus: GTK apps lose the
// keyboard until the session restarts). So an entry is protected, never
// listed as cleanable and refused by the delete command itself, while it
// holds something that is alive: a socket somebody listens on, a pid file of
// a process that exists, a lock file a process has open, a folder of a daemon
// that runs, or anything a running program has open. What was left behind by
// a program that is no longer running is not protected: it is cleaned.

use serde::Serialize;
use std::collections::HashSet;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".cache"))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    // Why it is not cleaned: "known", "socket", "lock" or "open".
    pub protected_reason: Option<String>,
}

/// Cache folders that cost time to rebuild (shaders, fonts). They are listed
/// as "review" in the interface and never counted as safe to clean. Mirrors
/// the rule of the frontend scan.
pub fn is_slow_rebuild(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["shader", "nvidia", "dxvk", "vkd3d", "mesa", "fontconfig"].iter().any(|keyword| lower.contains(keyword))
}

// ---- What is alive right now -------------------------------------------------

// Folders that hold live state while their daemon runs, with the process
// names of that daemon (as /proc/<pid>/comm shows them).
const KNOWN_LIVE: &[(&str, &[&str])] = &[("ibus", &["ibus-daemon"]), ("fcitx", &["fcitx"]), ("fcitx5", &["fcitx5"])];

// How deep and how much of an entry is looked at for live state.
const MAX_DEPTH: usize = 4;
const MAX_ENTRIES: usize = 20_000;

#[derive(Default)]
pub(crate) struct Live {
    // Everything a running program has open or as its working directory.
    pub(crate) open: HashSet<PathBuf>,
    // Paths of Unix sockets that something is bound to (/proc/net/unix).
    sockets: HashSet<PathBuf>,
    // Names of the running processes.
    pub(crate) comms: HashSet<String>,
}

impl Live {
    // Read once per scan or delete. What cannot be read (other users'
    // processes) is skipped.
    pub(crate) fn capture() -> Self {
        let mut live = Live::default();
        if let Ok(procs) = std::fs::read_dir("/proc") {
            for proc_entry in procs.flatten() {
                let name = proc_entry.file_name();
                if !name.to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let base = proc_entry.path();
                if let Ok(comm) = std::fs::read_to_string(base.join("comm")) {
                    live.comms.insert(comm.trim().to_string());
                }
                if let Ok(cwd) = std::fs::read_link(base.join("cwd")) {
                    live.open.insert(cwd);
                }
                if let Ok(fds) = std::fs::read_dir(base.join("fd")) {
                    for fd in fds.flatten() {
                        if let Ok(target) = std::fs::read_link(fd.path()) {
                            if target.is_absolute() {
                                live.open.insert(target);
                            }
                        }
                    }
                }
            }
        }
        // "Num RefCount Protocol Flags Type St Inode Path": the path is the
        // eighth field, and only bound sockets have one.
        if let Ok(text) = std::fs::read_to_string("/proc/net/unix") {
            for line in text.lines().skip(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 8 && fields[7].starts_with('/') {
                    live.sockets.insert(PathBuf::from(fields[7..].join(" ")));
                }
            }
        }
        live
    }

    pub(crate) fn is_open(&self, path: &Path) -> bool {
        // Path::starts_with compares whole components: "ibus" is not "ibus2".
        self.open.iter().any(|o| o.starts_with(path))
    }
}

fn process_exists(pid: u32) -> bool {
    pid > 0 && Path::new(&format!("/proc/{pid}")).exists()
}

fn is_lock_or_pid(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".lock") || lower.ends_with(".pid") || matches!(name, "LOCK" | "SingletonLock")
}

// Whether a lock or pid file belongs to something that is running now.
fn lock_is_live(path: &Path, name: &str, live: &Live) -> bool {
    if name.to_lowercase().ends_with(".pid") {
        // The file holds the process number.
        return std::fs::read_to_string(path).ok().and_then(|t| t.split_whitespace().next().and_then(|p| p.parse::<u32>().ok())).is_some_and(process_exists);
    }
    if name == "SingletonLock" {
        // Chromium style: a link to "<host>-<pid>".
        return std::fs::read_link(path)
            .ok()
            .and_then(|t| t.to_string_lossy().rsplit('-').next().and_then(|p| p.parse::<u32>().ok()))
            .is_some_and(process_exists);
    }
    // A plain lock file is alive when a process has it open.
    live.open.contains(path)
}

// A live socket or a live lock or pid file at the entry or inside it.
// Sockets win over locks in the answer. Nothing is followed through a link.
fn live_state(path: &Path, live: &Live) -> Option<&'static str> {
    let mut found_lock = false;
    let mut stack = vec![(path.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((current, depth)) = stack.pop() {
        seen += 1;
        if seen > MAX_ENTRIES {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&current) else { continue };
        if meta.file_type().is_socket() && live.sockets.contains(&current) {
            return Some("socket");
        }
        if let Some(name) = current.file_name().map(|n| n.to_string_lossy().into_owned()) {
            if is_lock_or_pid(&name) && lock_is_live(&current, &name, live) {
                found_lock = true;
            }
        }
        if meta.is_dir() && depth < MAX_DEPTH {
            if let Ok(children) = std::fs::read_dir(&current) {
                for child in children.flatten() {
                    stack.push((child.path(), depth + 1));
                }
            }
        }
    }
    found_lock.then_some("lock")
}

/// Why an entry of ~/.cache must not be cleaned, if it must not.
fn protection(path: &Path, live: &Live) -> Option<&'static str> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if KNOWN_LIVE.iter().any(|(folder, daemons)| *folder == name && daemons.iter().any(|d| live.comms.contains(*d))) {
        return Some("known");
    }
    if let Some(state) = live_state(path, live) {
        return Some(state);
    }
    if live.is_open(path) {
        return Some("open");
    }
    None
}

// ---- Scan ------------------------------------------------------------------

pub(crate) fn dir_size_bytes(path: &Path) -> u64 {
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
pub async fn scan_user_cache() -> Vec<CacheEntry> {
    tauri::async_runtime::spawn_blocking(scan_user_cache_blocking).await.unwrap_or_default()
}

fn scan_user_cache_blocking() -> Vec<CacheEntry> {
    let Some(cache) = cache_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&cache) else { return Vec::new() };
    let paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    let live = Live::capture();

    // One `du` process per top-level entry, run in parallel — same
    // reasoning as the GPG key list in soplos-repo-selector-3.0: a real
    // ~/.cache can have dozens of per-app subdirectories, and `du` isn't
    // free on a large one (browser caches routinely run into the GBs).
    // Running them one at a time would make the scan take as long as the
    // sum of every call instead of roughly the slowest one.
    let mut out = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .map(|path| {
                let live = &live;
                scope.spawn(move || {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let size_bytes = dir_size_bytes(path);
                    let protected_reason = protection(path, live).map(str::to_string);
                    CacheEntry { name, path: path.to_string_lossy().to_string(), size_bytes, protected_reason }
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

// ---- Delete ----------------------------------------------------------------

#[tauri::command]
pub async fn delete_cache_entries(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cache = cache_dir().ok_or("HOME is not set")?;
        delete_in(&cache, &paths, &Live::capture())
    })
    .await
    .map_err(|e| e.to_string())?
}

// The whole request is checked before anything is deleted: a path outside
// the cache, or an entry that is protected (checked again here, whatever the
// interface sent), refuses the request.
fn delete_in(cache: &Path, paths: &[String], live: &Live) -> Result<(), String> {
    let mut targets = Vec::new();
    for p in paths {
        let path = PathBuf::from(p);
        // Refuse anything that isn't a direct child of ~/.cache — this
        // command takes raw paths from the frontend, and a scan result
        // should never contain anything else, but a path outside ~/.cache
        // must never be deletable through this command no matter what
        // calls it with what.
        if path.parent() != Some(cache) {
            return Err(format!("Refusing to delete path outside ~/.cache: {p}"));
        }
        if let Some(reason) = protection(&path, live) {
            return Err(format!("Refusing to delete a protected entry ({reason}): {p}"));
        }
        targets.push(path);
    }
    for path in targets {
        let is_dir = std::fs::symlink_metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
        let result = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = result {
            // A cache entry can legitimately be recreated/locked by a
            // running app between scan and delete (e.g. a browser's own
            // cache while it's open) — skip it rather than aborting the
            // whole batch over one in-use file.
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("soplos-sys-cleaner: could not remove {}: {e}", path.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    // A short path of its own: Unix socket paths are limited to about 100
    // characters.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_with_a_live_socket_is_protected_and_a_dead_one_is_not() {
        let cache = scratch("sock");
        let live_dir = cache.join("daemon");
        std::fs::create_dir_all(&live_dir).unwrap();
        let listener = UnixListener::bind(live_dir.join("s")).unwrap();
        assert_eq!(protection(&live_dir, &Live::capture()), Some("socket"));
        // The listener gone, the socket file is only a leftover.
        drop(listener);
        assert_eq!(protection(&live_dir, &Live::capture()), None);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_folder_with_plain_files_can_be_cleaned() {
        let cache = scratch("plain");
        let plain = cache.join("thumbs");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("a.png"), b"x").unwrap();
        assert_eq!(protection(&plain, &Live::default()), None);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn pid_files_count_only_while_their_process_exists() {
        let cache = scratch("pid");
        let alive = cache.join("alive");
        std::fs::create_dir_all(&alive).unwrap();
        std::fs::write(alive.join("app.pid"), std::process::id().to_string()).unwrap();
        assert_eq!(protection(&alive, &Live::default()), Some("lock"));
        let dead = cache.join("dead");
        std::fs::create_dir_all(&dead).unwrap();
        // No process has this number.
        std::fs::write(dead.join("app.pid"), "4194999").unwrap();
        assert_eq!(protection(&dead, &Live::default()), None);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn lock_files_count_only_while_a_process_has_them_open() {
        let cache = scratch("lock");
        let folder = cache.join("copilot");
        std::fs::create_dir_all(&folder).unwrap();
        let lock = folder.join("x.lock");
        std::fs::write(&lock, b"").unwrap();
        // Nobody has it open: a leftover.
        assert_eq!(protection(&folder, &Live::default()), None);
        // This process has it open.
        let _held = std::fs::File::open(&lock).unwrap();
        assert!(protection(&folder, &Live::capture()).is_some());
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_known_folder_is_protected_only_while_its_daemon_runs() {
        let cache = scratch("known");
        let ibus = cache.join("ibus");
        std::fs::create_dir_all(&ibus).unwrap();
        assert_eq!(protection(&ibus, &Live::default()), None);
        let mut running = Live::default();
        running.comms.insert("ibus-daemon".to_string());
        assert_eq!(protection(&ibus, &running), Some("known"));
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_folder_a_program_has_open_is_protected() {
        let cache = scratch("open");
        let busy = cache.join("busy");
        std::fs::create_dir_all(&busy).unwrap();
        let mut live = Live::default();
        live.open.insert(busy.join("data.db"));
        assert_eq!(protection(&busy, &live), Some("open"));
        // A sibling whose name only starts the same is not the same folder.
        let busy2 = cache.join("busy2");
        std::fs::create_dir_all(&busy2).unwrap();
        assert_eq!(protection(&busy2, &live), None);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn the_delete_refuses_a_protected_entry_even_when_asked() {
        let cache = scratch("del");
        let live_dir = cache.join("live");
        std::fs::create_dir_all(&live_dir).unwrap();
        let _listener = UnixListener::bind(live_dir.join("s")).unwrap();
        let plain = cache.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("f"), b"x").unwrap();

        let live_s = live_dir.to_string_lossy().into_owned();
        let plain_s = plain.to_string_lossy().into_owned();
        let live = Live::capture();
        // One protected path refuses the whole request: the plain one stays too.
        assert!(delete_in(&cache, &[plain_s.clone(), live_s], &live).is_err());
        assert!(live_dir.exists() && plain.exists());
        // A path outside the cache is refused.
        assert!(delete_in(&cache, &["/etc".to_string()], &live).is_err());
        // A plain entry is deleted.
        assert!(delete_in(&cache, &[plain_s], &live).is_ok());
        assert!(!plain.exists());
        let _ = std::fs::remove_dir_all(&cache);
    }
}
