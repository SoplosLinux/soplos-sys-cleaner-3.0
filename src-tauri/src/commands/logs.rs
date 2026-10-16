// systemd-journald disk usage, ported from the journald half of 2.x's
// scanner/logs.py. Besides the total, the scan lists the journal files with
// their size and last-modified time, so the frontend can estimate how much a
// retention setting would free: journalctl has no dry run, but vacuuming only
// ever removes archived files, oldest first.

use serde::Serialize;
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::UNIX_EPOCH;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JournalFile {
    pub size_bytes: u64,
    pub modified_unix: u64,
    pub archived: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JournaldInfo {
    pub size_bytes: u64,
    pub files: Vec<JournalFile>,
}

/// What vacuuming the journal to the last `days` days would free, estimated
/// from the archived files last written before the cutoff. None when the
/// journal files cannot be read, in which case no figure is claimed. This is
/// the same estimate the interface makes for the default retention.
pub fn journal_freeable_days(info: &JournaldInfo, days: u64, now_unix: u64) -> Option<u64> {
    if info.files.is_empty() {
        return None;
    }
    let cutoff = now_unix.saturating_sub(days * 86400);
    Some(info.files.iter().filter(|f| f.archived && f.modified_unix < cutoff).map(|f| f.size_bytes).sum())
}

// journalctl --disk-usage prints a sentence like "Archived and active
// journals take up 1.2G in the filesystem." — same approach as 2.x: scan
// every token for one that parses as a size instead of depending on exact
// wording, which changes across systemd versions/locales.
fn parse_journal_size(token: &str) -> Option<u64> {
    let token = token.trim_matches('.');
    let (num_part, mult): (&str, f64) = if let Some(n) = token.strip_suffix('G') {
        (n, 1024f64 * 1024.0 * 1024.0)
    } else if let Some(n) = token.strip_suffix('M') {
        (n, 1024f64 * 1024.0)
    } else if let Some(n) = token.strip_suffix('K') {
        (n, 1024f64)
    } else if let Some(n) = token.strip_suffix('B') {
        if n.is_empty() {
            return None;
        }
        (n, 1.0)
    } else {
        return None;
    };
    num_part.parse::<f64>().ok().map(|n| (n * mult) as u64)
}

// Listing a directory and reading file metadata only needs the directory's
// execute bit, which journald leaves open, so no privileges are involved.
// Sizes are allocated bytes, the same figure journalctl --disk-usage sums.
fn journal_files() -> Vec<JournalFile> {
    let mut out = Vec::new();
    for base in ["/var/log/journal", "/run/log/journal"] {
        let Ok(machines) = std::fs::read_dir(base) else { continue };
        for machine in machines.flatten() {
            let Ok(files) = std::fs::read_dir(machine.path()) else { continue };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if !(name.ends_with(".journal") || name.ends_with(".journal~")) {
                    continue;
                }
                let Ok(meta) = file.metadata() else { continue };
                let modified_unix = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                out.push(JournalFile {
                    size_bytes: meta.blocks() * 512,
                    modified_unix,
                    // Active files are exactly system.journal and user-<uid>.journal.
                    archived: name.contains('@') || name.ends_with('~'),
                });
            }
        }
    }
    out
}

#[tauri::command]
pub async fn scan_journald() -> JournaldInfo {
    tauri::async_runtime::spawn_blocking(scan_journald_blocking).await.unwrap_or(JournaldInfo { size_bytes: 0, files: Vec::new() })
}

fn scan_journald_blocking() -> JournaldInfo {
    let files = journal_files();
    if !files.is_empty() {
        let size_bytes = files.iter().map(|f| f.size_bytes).sum();
        return JournaldInfo { size_bytes, files };
    }

    let Ok(output) = Command::new("journalctl").arg("--disk-usage").output() else {
        return JournaldInfo { size_bytes: 0, files };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let size_bytes = text.split_whitespace().find_map(parse_journal_size).unwrap_or(0);
    JournaldInfo { size_bytes, files }
}
