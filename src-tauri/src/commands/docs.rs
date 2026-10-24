// Documentation that can go without breaking anything: a fixed catalogue of
// documentation stores, and every per-package folder under /usr/share/doc
// that holds more than its copyright file, whatever its size: the user
// decides what goes, so nothing is left out of the list. Compared with 2.x, what is not documentation is gone from
// the list: the Xfce helpers (preferred applications), doc-base (dpkg's
// registry), KDE doctools, the CUPS web interface and the Soplos Welcome
// content. The manual pages and /usr/share/doc itself are never offered as a
// whole.
//
// Removing a source deletes what is inside it and keeps the folder itself
// (dpkg owns it) plus the files that must stay: the copyright file of a
// package (Debian policy) and the info index. The privileged side recomputes
// the sources and takes every path from there, never from the request.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DOC_DIR: &str = "/usr/share/doc";

struct Source {
    id: &'static str,
    path: &'static str,
    keep: &'static [&'static str],
}

const SOURCES: &[Source] = &[
    // `dir` is the index the info reader needs; install-info maintains it.
    Source { id: "info", path: "/usr/share/info", keep: &["dir"] },
    Source { id: "qt5", path: "/usr/share/qt5/doc", keep: &[] },
    Source { id: "qt6", path: "/usr/share/qt6/doc", keep: &[] },
    Source { id: "gtkdoc", path: "/usr/share/gtk-doc", keep: &[] },
    Source { id: "devhelp", path: "/usr/share/devhelp", keep: &[] },
];

const PACKAGE_KEEP: &[&str] = &["copyright"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocEntry {
    // "info", "qt5"... or "pkg:<package>".
    pub id: String,
    pub path: String,
    pub size_bytes: u64,
    pub keeps: Vec<String>,
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn sizes(paths: &[PathBuf]) -> HashMap<PathBuf, u64> {
    let mut sizes = HashMap::new();
    if paths.is_empty() {
        return sizes;
    }
    let Ok(output) = Command::new("du").args(["-sb", "--"]).args(paths).output() else { return sizes };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((size, path)) = line.split_once('\t') {
            if let Ok(size) = size.trim().parse::<u64>() {
                sizes.insert(PathBuf::from(path), size);
            }
        }
    }
    sizes
}

fn package_dirs() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(DOC_DIR) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // HTML holds the KDE help, which belongs to Languages. Symlinked
            // folders point at another package's documentation.
            name != "HTML" && !name.starts_with('.') && is_real_dir(&e.path())
        })
        .map(|e| e.path())
        .collect()
}

// Bytes a removal frees: the folder minus the files that are kept.
fn removable_size(path: &Path, keep: &[&str], sizes: &HashMap<PathBuf, u64>) -> u64 {
    let total = sizes.get(path).copied().unwrap_or(0);
    let kept: u64 = keep.iter().map(|name| fs::symlink_metadata(path.join(name)).map(|m| m.len()).unwrap_or(0)).sum();
    total.saturating_sub(kept)
}

fn scan_blocking() -> Vec<DocEntry> {
    let source_paths: Vec<PathBuf> = SOURCES.iter().map(|s| PathBuf::from(s.path)).filter(|p| is_real_dir(p)).collect();
    let packages = package_dirs();
    let all: Vec<PathBuf> = source_paths.iter().chain(packages.iter()).cloned().collect();
    let sizes = sizes(&all);

    let mut entries = Vec::new();
    for source in SOURCES {
        let path = Path::new(source.path);
        if !is_real_dir(path) {
            continue;
        }
        let size_bytes = removable_size(path, source.keep, &sizes);
        if size_bytes > 0 {
            entries.push(DocEntry { id: source.id.to_string(), path: source.path.to_string(), size_bytes, keeps: source.keep.iter().map(|s| s.to_string()).collect() });
        }
    }
    for path in packages {
        let size_bytes = removable_size(&path, PACKAGE_KEEP, &sizes);
        // A folder that holds nothing but the copyright file has nothing to remove.
        if size_bytes > 0 {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            entries.push(DocEntry { id: format!("pkg:{name}"), path: path.to_string_lossy().into_owned(), size_bytes, keeps: PACKAGE_KEEP.iter().map(|s| s.to_string()).collect() });
        }
    }
    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    entries
}

#[tauri::command]
pub async fn scan_docs() -> Vec<DocEntry> {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or_default()
}

// id -> (folder, files to keep), or None when the id is not removable.
fn resolve(id: &str) -> Option<(PathBuf, Vec<&'static str>)> {
    if let Some(source) = SOURCES.iter().find(|s| s.id == id) {
        let path = PathBuf::from(source.path);
        return if is_real_dir(&path) { Some((path, source.keep.to_vec())) } else { None };
    }
    let name = id.strip_prefix("pkg:")?;
    let valid = !name.is_empty() && name != "HTML" && name.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) && name.chars().all(|c| c.is_ascii_alphanumeric() || "+._-".contains(c));
    if !valid {
        return None;
    }
    let path = Path::new(DOC_DIR).join(name);
    if is_real_dir(&path) {
        Some((path, PACKAGE_KEEP.to_vec()))
    } else {
        None
    }
}

/// Privileged-side check: every id must name a documentation source.
pub fn validate_docs(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Err("No documentation given".to_string());
    }
    match ids.iter().find(|id| resolve(id).is_none()) {
        Some(bad) => Err(format!("Not a documentation source: {bad}")),
        None => Ok(()),
    }
}

// Everything inside `path` goes except the named entries; the folder itself
// stays. Symbolic links are removed as links, never followed.
fn remove_children_except(path: &Path, keep: &[&str]) -> bool {
    let Ok(entries) = fs::read_dir(path) else { return true };
    let mut all_ok = true;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.contains(&name.as_str()) {
            continue;
        }
        let child = entry.path();
        let is_dir = fs::symlink_metadata(&child).map(|m| m.is_dir()).unwrap_or(false);
        let result = if is_dir { fs::remove_dir_all(&child) } else { fs::remove_file(&child) };
        if let Err(e) = result {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("soplos-sys-cleaner: could not delete {}: {e}", child.display());
                all_ok = false;
            }
        }
    }
    all_ok
}

/// Deletes what is inside each source, except the files that must stay.
pub fn remove_docs(ids: &[String]) -> bool {
    let mut all_ok = true;
    for id in ids {
        if let Some((path, keep)) = resolve(id) {
            all_ok &= remove_children_except(&path, &keep);
        }
    }
    all_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prints the scan of this machine: cargo test dump_live_scan -- --ignored --nocapture
    #[test]
    #[ignore = "prints the live scan"]
    fn dump_live_scan() {
        for e in scan_blocking() {
            println!("{:<28} {:>12} keeps={:?} {}", e.id, e.size_bytes, e.keeps, e.path);
        }
    }

    #[test]
    fn ids_outside_the_catalogue_are_refused() {
        assert!(validate_docs(&[]).is_err());
        assert!(validate_docs(&["../etc".to_string()]).is_err());
        assert!(validate_docs(&["pkg:../etc".to_string()]).is_err());
        assert!(validate_docs(&["pkg:HTML".to_string()]).is_err());
        assert!(validate_docs(&["pkg:".to_string()]).is_err());
        assert!(validate_docs(&["cups".to_string()]).is_err());
        assert!(validate_docs(&["man".to_string()]).is_err());
    }

    #[test]
    fn removal_keeps_the_copyright_file() {
        let dir = std::env::temp_dir().join(format!("soplos-docs-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("examples")).unwrap();
        fs::write(dir.join("copyright"), b"c").unwrap();
        fs::write(dir.join("changelog.gz"), b"x").unwrap();
        fs::write(dir.join("examples/a"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc", dir.join("link-to-etc")).unwrap();
        assert!(remove_children_except(&dir, PACKAGE_KEEP));
        assert!(dir.join("copyright").exists());
        assert!(!dir.join("link-to-etc").exists());
        // The link was removed, not followed.
        assert!(Path::new("/etc/passwd").exists());
        assert!(!dir.join("changelog.gz").exists());
        assert!(!dir.join("examples").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
