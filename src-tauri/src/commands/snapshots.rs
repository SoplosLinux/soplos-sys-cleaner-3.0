// System snapshots: the ones Timeshift keeps (in its rsync or its BTRFS mode)
// and the ones Snapper keeps. Both tools need root to list and to delete, so
// everything here runs on the privileged side (see privileged.rs): the list is
// printed as `@@snapshot <json>` lines for the window to read, and a delete
// is only accepted for a snapshot that the tool itself lists right now, and
// never for the newest one of its set, so the machine is not left without a
// restore point.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    /// "timeshift" or "snapper".
    pub tool: String,
    /// The Snapper configuration; empty for Timeshift.
    pub config: String,
    /// The name of a Timeshift snapshot, the number of a Snapper one.
    pub id: String,
    pub date: String,
    /// "rsync" or "btrfs" for Timeshift; "single", "pre" or "post" for Snapper.
    pub kind: String,
    pub tags: String,
    pub description: String,
    /// 0 when it cannot be told.
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapshotRef {
    pub tool: String,
    pub config: String,
    pub id: String,
}

fn has_command(name: &str) -> bool {
    super::packages::command_exists(name)
}

/// Stdout of a command run with the C locale; None when it fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).env("LC_ALL", "C").env("LANG", "C").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---------------------------------------------------------------- Timeshift

/// A Timeshift snapshot is named after the moment it was made: 2024-05-01_10-00-00.
pub fn is_timeshift_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != 19 {
        return false;
    }
    bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 | 13 | 16 => *b == b'-',
        10 => *b == b'_',
        _ => b.is_ascii_digit(),
    })
}

fn timeshift_date(name: &str) -> String {
    let (day, time) = name.split_once('_').unwrap_or((name, ""));
    format!("{day} {}", time.replace('-', ":"))
}

/// What `timeshift --list` prints: the mode ("rsync" or "btrfs") and one entry
/// for each line of its table, which looks like
/// `0    >  2024-05-01_10-00-00  O  Before the upgrade`.
pub fn parse_timeshift(text: &str) -> (String, Vec<SnapshotEntry>) {
    let mut mode = String::new();
    let mut in_table = false;
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Mode") {
            if let Some(value) = rest.split(':').nth(1) {
                mode = value.trim().to_lowercase();
            }
            continue;
        }
        if trimmed.starts_with("---") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let Some(position) = parts.iter().position(|part| is_timeshift_name(part)) else {
            continue;
        };
        let name = parts[position];
        let rest = &parts[position + 1..];
        // The tags are capital letters (O on demand, B boot, H hourly, D daily,
        // W weekly, M monthly); what follows is the description.
        let (tags, words) = match rest.first() {
            Some(first) if first.len() <= 6 && first.chars().all(|c| c.is_ascii_uppercase()) => (first.to_string(), &rest[1..]),
            _ => (String::new(), rest),
        };
        entries.push(SnapshotEntry {
            tool: "timeshift".to_string(),
            config: String::new(),
            id: name.to_string(),
            date: timeshift_date(name),
            kind: mode.clone(),
            tags,
            description: words.join(" "),
            size_bytes: 0,
        });
    }
    // The mode line comes before the table; set it on every entry afterwards.
    for entry in &mut entries {
        entry.kind = mode.clone();
    }
    (mode, entries)
}

/// The size of rsync snapshots kept on the root device. Hard links make the
/// figure of a snapshot what only it holds: du counts each file once, in the
/// first directory it is given, so they are given newest first.
fn timeshift_sizes(entries: &mut [SnapshotEntry]) {
    let base = Path::new("/timeshift/snapshots");
    if !base.is_dir() {
        return;
    }
    let mut newest_first: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
    newest_first.sort();
    newest_first.reverse();
    let paths: Vec<String> = newest_first.iter().map(|name| format!("{}/{name}", base.display())).collect();
    let mut args: Vec<&str> = vec!["120", "du", "-sb", "--one-file-system", "--"];
    args.extend(paths.iter().map(|p| p.as_str()));
    let Some(text) = run("timeout", &args) else {
        return;
    };
    for line in text.lines() {
        let Some((bytes, path)) = line.split_once('\t') else {
            continue;
        };
        let Ok(bytes) = bytes.trim().parse::<u64>() else {
            continue;
        };
        if let Some(name) = path.trim().rsplit('/').next() {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == name) {
                entry.size_bytes = bytes;
            }
        }
    }
}

/// The subvolumes of a BTRFS file system, from `btrfs subvolume list`, whose
/// lines look like `ID 257 gen 10 top level 5 path timeshift-btrfs/snapshots/2024-05-01_10-00-00/@`:
/// the id and the path from the top of the file system.
pub fn parse_subvolume_list(text: &str) -> Vec<(u64, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let id: u64 = line.strip_prefix("ID ")?.split_whitespace().next()?.parse().ok()?;
            let path = line.split_once(" path ")?.1.trim();
            Some((id, path.trim_start_matches("<FS_TREE>/").to_string()))
        })
        .collect()
}

/// What each subvolume holds on its own, in bytes, from
/// `btrfs qgroup show --raw`: rows of `0/<id> <referenced> <exclusive>`.
pub fn parse_qgroup_exclusive(text: &str) -> Vec<(u64, u64)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (level, id) = parts.next()?.split_once('/')?;
            if level != "0" {
                return None;
            }
            let id: u64 = id.parse().ok()?;
            let _referenced: u64 = parts.next()?.parse().ok()?;
            let exclusive: u64 = parts.next()?.parse().ok()?;
            Some((id, exclusive))
        })
        .collect()
}

/// The size of BTRFS snapshots: what each one holds that nothing else does,
/// which is what deleting it gives back. BTRFS only tells that when quotas
/// (qgroups) are on, which is Timeshift's own condition for showing sizes; with
/// them off the sizes stay 0, which the window shows as unknown.
fn timeshift_btrfs_sizes(entries: &mut [SnapshotEntry]) {
    let Some(list) = run("btrfs", &["subvolume", "list", "/"]) else {
        return;
    };
    let Some(groups) = run("btrfs", &["qgroup", "show", "--raw", "/"]) else {
        return;
    };
    let subvolumes = parse_subvolume_list(&list);
    let exclusive = parse_qgroup_exclusive(&groups);
    for entry in entries.iter_mut() {
        // A snapshot is one subvolume for each part of the system it covers
        // (@, @home), all under the folder named after it.
        let prefix = format!("timeshift-btrfs/snapshots/{}/", entry.id);
        entry.size_bytes = subvolumes
            .iter()
            .filter(|(_, path)| path.starts_with(&prefix))
            .filter_map(|(id, _)| exclusive.iter().find(|(group, _)| group == id).map(|(_, bytes)| *bytes))
            .sum();
    }
}

fn list_timeshift() -> Vec<SnapshotEntry> {
    if !has_command("timeshift") {
        return Vec::new();
    }
    let Some(text) = run("timeshift", &["--list", "--scripted"]) else {
        return Vec::new();
    };
    let (mode, mut entries) = parse_timeshift(&text);
    if mode == "rsync" {
        timeshift_sizes(&mut entries);
    } else if mode == "btrfs" {
        timeshift_btrfs_sizes(&mut entries);
    }
    entries
}

// ------------------------------------------------------------------ Snapper

/// The names of the Snapper configurations, from `snapper --jsonout list-configs`.
pub fn parse_snapper_configs(json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .get("configs")
        .and_then(|v| v.as_array())
        .map(|list| list.iter().filter_map(|c| c.get("config").and_then(|v| v.as_str()).map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The snapshots of one configuration, from `snapper --jsonout -c <name> list`.
/// Number 0 is the running system, not a snapshot.
pub fn parse_snapper(config: &str, json: &str) -> Vec<SnapshotEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(list) = value.as_object().and_then(|object| object.values().find_map(|v| v.as_array())) else {
        return Vec::new();
    };
    let text = |item: &serde_json::Value, key: &str| item.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    list.iter()
        .filter_map(|item| {
            let number = item.get("number")?.as_u64()?;
            if number == 0 {
                return None;
            }
            Some(SnapshotEntry {
                tool: "snapper".to_string(),
                config: config.to_string(),
                id: number.to_string(),
                date: text(item, "date"),
                kind: text(item, "type"),
                tags: text(item, "cleanup"),
                description: text(item, "description"),
                // Only there when the quota of the file system is enabled.
                size_bytes: item.get("used-space").and_then(|v| v.as_u64()).unwrap_or(0),
            })
        })
        .collect()
}

fn snapper_configs() -> Vec<String> {
    if !has_command("snapper") {
        return Vec::new();
    }
    run("snapper", &["--jsonout", "list-configs"]).map(|json| parse_snapper_configs(&json)).unwrap_or_default()
}

fn list_snapper() -> Vec<SnapshotEntry> {
    let mut entries = Vec::new();
    for config in snapper_configs() {
        if let Some(json) = run("snapper", &["--jsonout", "-c", config.as_str(), "list"]) {
            entries.extend(parse_snapper(&config, &json));
        }
    }
    entries
}

// --------------------------------------------------------------- the helper

/// Every snapshot of the tools that are installed.
pub fn list_snapshots() -> Vec<SnapshotEntry> {
    let mut all = list_timeshift();
    all.extend(list_snapper());
    all
}

/// Root side of the list. First the places a snapshot can be made in, one
/// `@@tool timeshift` or `@@tool snapper <config>` line each, so the window
/// can offer them even when there are no snapshots yet; then one
/// `@@snapshot <json>` line for each snapshot.
pub fn print_list() -> bool {
    if has_command("timeshift") {
        println!("@@tool timeshift");
    }
    for config in snapper_configs() {
        println!("@@tool snapper {config}");
    }
    for entry in list_snapshots() {
        if let Ok(json) = serde_json::to_string(&entry) {
            println!("@@snapshot {json}");
        }
    }
    true
}

fn valid_config(config: &str) -> bool {
    !config.is_empty() && config.len() <= 64 && config.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && config.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Syntax of a request, checked on the user side too.
pub fn validate_ref(item: &SnapshotRef) -> Result<(), String> {
    match item.tool.as_str() {
        "timeshift" => {
            if !item.config.is_empty() || !is_timeshift_name(&item.id) {
                return Err(format!("Invalid Timeshift snapshot: {}", item.id));
            }
            Ok(())
        }
        "snapper" => {
            let number_ok = !item.id.is_empty() && item.id.len() <= 9 && item.id.chars().all(|c| c.is_ascii_digit()) && item.id != "0" && !item.id.starts_with('0');
            if !valid_config(&item.config) || !number_ok {
                return Err(format!("Invalid Snapper snapshot: {} {}", item.config, item.id));
            }
            Ok(())
        }
        other => Err(format!("Unknown snapshot tool: {other}")),
    }
}

/// The id of the newest snapshot of a tool (and configuration): the last name
/// of Timeshift, the highest number of Snapper.
fn newest<'a>(live: &'a [SnapshotEntry], tool: &str, config: &str) -> Option<&'a str> {
    live.iter()
        .filter(|e| e.tool == tool && e.config == config)
        .max_by(|a, b| match tool {
            "snapper" => a.id.parse::<u64>().unwrap_or(0).cmp(&b.id.parse::<u64>().unwrap_or(0)),
            _ => a.id.cmp(&b.id),
        })
        .map(|e| e.id.as_str())
}

/// Checks a delete against the system as it is now: the snapshot must be
/// listed by the tool, and must not be the newest of its set.
pub fn validate_delete(items: &[SnapshotRef]) -> Result<(), String> {
    let live = list_snapshots();
    for item in items {
        if !live.iter().any(|e| e.tool == item.tool && e.config == item.config && e.id == item.id) {
            return Err(format!("Not a snapshot of the system: {} {}", item.tool, item.id));
        }
        if newest(&live, &item.tool, &item.config) == Some(item.id.as_str()) {
            return Err(format!("The newest snapshot is always kept: {}", item.id));
        }
    }
    Ok(())
}

/// The boot menu that lists snapshots has to be made again when one goes,
/// unless grub-btrfsd does it by itself.
fn refresh_grub_menu() {
    if !Path::new("/etc/grub.d/41_snapshots-btrfs").exists() || !has_command("update-grub") {
        return;
    }
    let daemon = Command::new("systemctl").args(["is-active", "--quiet", "grub-btrfsd"]).status().map(|s| s.success()).unwrap_or(false);
    if !daemon {
        let _ = Command::new("update-grub").status();
    }
}

/// Deletes the given snapshots; true only when every one went.
pub fn delete(items: &[SnapshotRef]) -> bool {
    let mut all_ok = true;
    for item in items {
        let status = match item.tool.as_str() {
            "timeshift" => Command::new("timeshift").args(["--delete", "--snapshot", item.id.as_str(), "--scripted", "--yes"]).status(),
            "snapper" => Command::new("snapper").args(["-c", item.config.as_str(), "delete", item.id.as_str()]).status(),
            _ => {
                all_ok = false;
                continue;
            }
        };
        match status {
            Ok(status) if status.success() => {}
            Ok(_) => all_ok = false,
            Err(e) => {
                eprintln!("soplos-sys-cleaner: could not delete a snapshot: {e}");
                all_ok = false;
            }
        }
    }
    refresh_grub_menu();
    all_ok
}

/// The text of a new snapshot's comment: short, with plain characters.
pub fn valid_comment(comment: &str) -> bool {
    !comment.is_empty() && comment.chars().count() <= 60 && comment.chars().all(|c| c.is_alphanumeric() || " .,_-".contains(c)) && !comment.starts_with('-')
}

pub fn validate_create(tool: &str, config: &str, comment: &str) -> Result<(), String> {
    if !valid_comment(comment) {
        return Err("Invalid snapshot comment".to_string());
    }
    match tool {
        "timeshift" if config.is_empty() => Ok(()),
        "snapper" if valid_config(config) => Ok(()),
        _ => Err(format!("Cannot create a snapshot with {tool} {config}")),
    }
}

/// Makes a snapshot now.
pub fn create(tool: &str, config: &str, comment: &str) -> bool {
    let status = match tool {
        "timeshift" => Command::new("timeshift").args(["--create", "--comments", comment, "--tags", "O", "--scripted", "--yes"]).status(),
        "snapper" => Command::new("snapper").args(["-c", config, "create", "--description", comment]).status(),
        _ => return false,
    };
    let ok = status.map(|s| s.success()).unwrap_or(false);
    if ok {
        refresh_grub_menu();
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMESHIFT: &str = "Mode   : RSYNC\nDevice : /dev/sda2\nUUID   : 1234\nPath   : /run/timeshift/1/backup\nDevice is OK.\n3 snapshots, 20.1 GB free\n\nNum     Name                 Tags  Description\n------------------------------------------------------------------------------\n0    >  2024-04-01_10-00-00  O     Before the upgrade\n1    >  2024-05-01_03-00-01  D     \n2    >  2024-05-02_03-00-01  BD    \n3    >  2024-05-03_09-30-00        My notes here\n";

    #[test]
    fn a_timeshift_list_gives_its_mode_and_its_snapshots() {
        let (mode, entries) = parse_timeshift(TIMESHIFT);
        assert_eq!(mode, "rsync");
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].id, "2024-04-01_10-00-00");
        assert_eq!(entries[0].date, "2024-04-01 10:00:00");
        assert_eq!(entries[0].tags, "O");
        assert_eq!(entries[0].description, "Before the upgrade");
        assert_eq!(entries[2].tags, "BD");
        assert_eq!(entries[3].tags, "");
        assert_eq!(entries[3].description, "My notes here");
        assert!(entries.iter().all(|e| e.kind == "rsync" && e.tool == "timeshift"));
    }

    #[test]
    fn the_subvolumes_of_a_btrfs_file_system_come_with_their_paths() {
        let text = "ID 256 gen 900 top level 5 path @\nID 257 gen 901 top level 5 path @home\nID 300 gen 800 top level 5 path timeshift-btrfs/snapshots/2024-05-01_10-00-00/@\nID 301 gen 800 top level 5 path timeshift-btrfs/snapshots/2024-05-01_10-00-00/@home\nnot a line\n";
        let list = parse_subvolume_list(text);
        assert_eq!(list.len(), 4);
        assert_eq!(list[2], (300, "timeshift-btrfs/snapshots/2024-05-01_10-00-00/@".to_string()));
        assert!(parse_subvolume_list("").is_empty());
    }

    #[test]
    fn the_exclusive_size_of_each_subvolume_comes_from_its_qgroup() {
        let text = "Qgroupid    Referenced    Exclusive\n--------    ----------    ---------\n0/5              16384        16384\n0/300       2147483648    104857600\n0/301        536870912      4194304\n1/0         2684354560    109051904\n";
        let groups = parse_qgroup_exclusive(text);
        assert_eq!(groups, vec![(5, 16384), (300, 104857600), (301, 4194304)]);
        assert!(parse_qgroup_exclusive("ERROR: quota not enabled\n").is_empty());
    }

    #[test]
    fn a_list_without_a_table_has_no_snapshots() {
        assert!(parse_timeshift("Mode : BTRFS\nNo snapshots found\n").1.is_empty());
        assert!(parse_timeshift("").1.is_empty());
    }

    #[test]
    fn timeshift_names_are_told_by_their_shape() {
        assert!(is_timeshift_name("2024-05-01_10-00-00"));
        assert!(!is_timeshift_name("2024-05-01 10-00-00"));
        assert!(!is_timeshift_name("2024-05-01_10-00-0x"));
        assert!(!is_timeshift_name("../etc"));
        assert!(!is_timeshift_name(""));
    }

    #[test]
    fn snapper_lists_come_without_the_running_system() {
        let json = r#"{"root":[{"number":0,"type":"single","date":"","description":"current"},{"number":12,"type":"pre","date":"2024-05-01 10:00:00","cleanup":"number","description":"apt","used-space":4096},{"number":13,"type":"post","date":"2024-05-01 10:05:00","cleanup":"number","description":"apt"}]}"#;
        let entries = parse_snapper("root", json);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "12");
        assert_eq!(entries[0].kind, "pre");
        assert_eq!(entries[0].size_bytes, 4096);
        assert_eq!(entries[1].size_bytes, 0);
        assert_eq!(parse_snapper_configs(r#"{"configs":[{"config":"root","subvolume":"/"},{"config":"home","subvolume":"/home"}]}"#), vec!["root", "home"]);
        assert!(parse_snapper("root", "nothing").is_empty());
    }

    fn entry(tool: &str, config: &str, id: &str) -> SnapshotEntry {
        SnapshotEntry { tool: tool.into(), config: config.into(), id: id.into(), date: String::new(), kind: String::new(), tags: String::new(), description: String::new(), size_bytes: 0 }
    }

    #[test]
    fn the_newest_of_each_set_is_the_last_one() {
        let live = vec![entry("timeshift", "", "2024-05-01_10-00-00"), entry("timeshift", "", "2024-05-03_10-00-00"), entry("snapper", "root", "9"), entry("snapper", "root", "10"), entry("snapper", "home", "2")];
        assert_eq!(newest(&live, "timeshift", ""), Some("2024-05-03_10-00-00"));
        assert_eq!(newest(&live, "snapper", "root"), Some("10"));
        assert_eq!(newest(&live, "snapper", "home"), Some("2"));
    }

    #[test]
    fn a_request_must_be_well_formed() {
        let r = |tool: &str, config: &str, id: &str| SnapshotRef { tool: tool.into(), config: config.into(), id: id.into() };
        assert!(validate_ref(&r("timeshift", "", "2024-05-01_10-00-00")).is_ok());
        assert!(validate_ref(&r("timeshift", "", "../../etc")).is_err());
        assert!(validate_ref(&r("timeshift", "root", "2024-05-01_10-00-00")).is_err());
        assert!(validate_ref(&r("snapper", "root", "12")).is_ok());
        assert!(validate_ref(&r("snapper", "root", "0")).is_err());
        assert!(validate_ref(&r("snapper", "root", "1; reboot")).is_err());
        assert!(validate_ref(&r("snapper", "-x", "12")).is_err());
        assert!(validate_ref(&r("snapper", "", "12")).is_err());
        assert!(validate_ref(&r("btrfs", "", "12")).is_err());
    }

    #[test]
    fn the_comment_of_a_new_snapshot_is_plain_text() {
        assert!(valid_comment("Before the upgrade"));
        assert!(valid_comment("Antes de la actualización"));
        assert!(!valid_comment(""));
        assert!(!valid_comment("--delete"));
        assert!(!valid_comment("a; reboot"));
        assert!(!valid_comment(&"x".repeat(61)));
        assert!(validate_create("timeshift", "", "Test").is_ok());
        assert!(validate_create("snapper", "root", "Test").is_ok());
        assert!(validate_create("snapper", "", "Test").is_err());
    }
}
