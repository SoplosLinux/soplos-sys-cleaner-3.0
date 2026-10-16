// Unused Flatpak runtimes — ported from 2.x's scanner/flatpak.py. Detection
// reuses flatpak's own dependency resolver rather than reimplementing it:
// `flatpak uninstall --unused` (no -y) prints exactly what it would remove,
// then "n\n" on stdin cancels before anything is actually deleted.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlatpakEntry {
    pub reference: String,
    pub name: String,
    pub size_bytes: u64,
    // Installed for this user only. Those cannot be removed as root, because
    // the administrator has its own, empty, user installation.
    pub user: bool,
}

fn flatpak_available() -> bool {
    Command::new("flatpak").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

// Extension segments loaded dynamically by gaming/media Flatpak apps (e.g.
// Lutris, Bottles) — flatpak's own resolver can't trace that use, so without
// this exclusion it would flag GPU/codec runtimes still in active use as
// "unused" (same list Welcome's own Flatpak cleanup uses, see
// soplos-welcome-3.0's clean_system()).
const EXTENSION_SEGMENTS: &[&str] = &[".GL.", ".GL32.", ".VulkanLayer.", ".ffmpeg", ".VAAPI.", ".openh264", ".Compat32."];

fn flatpak_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/flatpak"));
    }
    roots.push(PathBuf::from("/var/lib/flatpak"));
    roots
}

// `flatpak list --all --columns=ref` gives the exact id/arch/branch ref for
// every installed item; the unused-table only gives id (+ branch), so this
// maps back to the full ref needed both to display something unambiguous and
// to find the right directory for `du`.
fn all_refs() -> HashMap<String, (String, bool)> {
    let output = match Command::new("flatpak")
        .args(["list", "--all", "--columns=ref,installation"])
        .env("LC_ALL", "C")
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };
    let mut refs = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut columns = line.split_whitespace();
        let Some(line) = columns.next() else { continue };
        let user = columns.next() == Some("user");
        let parts: Vec<&str> = line.split('/').collect();
        if parts.len() < 2 {
            continue;
        }
        let app_id = parts[0];
        let branch = parts.get(2).copied().unwrap_or("");
        let key = if branch.is_empty() { app_id.to_string() } else { format!("{app_id}/{branch}") };
        refs.entry(key).or_insert_with(|| (line.to_string(), user));
        refs.entry(app_id.to_string()).or_insert_with(|| (line.to_string(), user));
    }
    refs
}

fn dir_size_bytes(path: &std::path::Path) -> u64 {
    Command::new("du")
        .args(["-sb", "--"])
        .arg(path)
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn ref_size_bytes(full_ref: &str) -> u64 {
    let parts: Vec<&str> = full_ref.split('/').collect();
    if parts.len() < 3 {
        return 0;
    }
    let (kind_hint, name, arch, branch) = ("runtime", parts[0], parts[1], parts[2]);
    for root in flatpak_roots() {
        let active = root.join(kind_hint).join(name).join(arch).join(branch).join("active");
        let target = std::fs::read_link(&active).map(|rel| active.parent().unwrap().join(rel)).unwrap_or(active);
        if target.is_dir() {
            return dir_size_bytes(&target);
        }
    }
    0
}

#[tauri::command]
pub async fn scan_flatpak_unused() -> Vec<FlatpakEntry> {
    tauri::async_runtime::spawn_blocking(scan_flatpak_unused_blocking).await.unwrap_or_default()
}

fn scan_flatpak_unused_blocking() -> Vec<FlatpakEntry> {
    if !flatpak_available() {
        return Vec::new();
    }

    let Ok(mut child) = Command::new("flatpak")
        .args(["uninstall", "--unused"])
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"n\n");
    }
    let Ok(output) = child.wait_with_output() else { return Vec::new() };
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    let refs = all_refs();
    let mut entries = Vec::new();
    for line in combined.lines() {
        // Rows in flatpak's table look like "1.  org.freedesktop.Platform  25.08  r".
        let tokens: Vec<&str> = line.trim().split_whitespace().collect();
        let Some(first) = tokens.first() else { continue };
        if !(first.ends_with('.') && first[..first.len() - 1].chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let Some(app_id) = tokens.get(1) else { continue };
        if EXTENSION_SEGMENTS.iter().any(|seg| app_id.contains(seg)) {
            continue;
        }
        let branch = tokens.get(2).filter(|b| **b != "r").copied().unwrap_or("");
        let key = if branch.is_empty() { app_id.to_string() } else { format!("{app_id}/{branch}") };
        let (full_ref, user) = refs.get(&key).or_else(|| refs.get(*app_id)).cloned().unwrap_or_else(|| (app_id.to_string(), false));
        entries.push((full_ref, app_id.to_string(), user));
    }

    // One `du` per entry, in parallel — same reasoning as user_cache's scan.
    let mut out = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = entries
            .into_iter()
            .map(|(reference, name, user)| {
                scope.spawn(move || {
                    let size_bytes = ref_size_bytes(&reference);
                    FlatpakEntry { reference, name, size_bytes, user }
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

/// Removes unused runtimes from the user's own installation. No privileges are
/// needed, so no password is asked. The refs are checked the same way the
/// privileged side checks them.
#[tauri::command]
pub async fn flatpak_uninstall_user(refs: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if refs.is_empty() {
            return Err("No refs given".to_string());
        }
        let valid = |r: &String| {
            r.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && r.chars().all(|c| c.is_ascii_alphanumeric() || "./-_".contains(c))
        };
        if let Some(bad) = refs.iter().find(|r| !valid(r)) {
            return Err(format!("Invalid Flatpak ref: {bad}"));
        }
        let status = Command::new("flatpak")
            .args(["uninstall", "--user", "-y", "--noninteractive"])
            .args(&refs)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("flatpak could not remove every runtime".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
