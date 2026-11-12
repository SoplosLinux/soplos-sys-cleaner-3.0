// What uninstalled programs leave in the home folder: their folders in ~/.config,
// ~/.local/share, ~/.local/state, the hidden folders of the home itself, ~/.var/app (Flatpak)
// and ~/snap. Entirely user-scope, never needs root.
//
// The file system does not record which program made a folder, so a folder is listed only when
// the system itself proves that its program is gone. Nothing is guessed from a name that looks
// like another, from what a folder contains, or from how long ago it was used. The proofs:
//   - "flatpak": ~/.var/app/<id> of an application that is not installed. Flatpak names that
//                folder with the exact id of its application;
//   - "snap":    ~/snap/<name> of a snap that is not installed, named the same way by snapd;
//   - "package": a folder whose name, without its leading dot and in lower case, is exactly the
//                name of a package that dpkg removed (its log, or state `rc`), when nothing that
//                is installed has that same name: no package, no folder that a package installs
//                at the top of /usr/share, /usr/lib, /usr/libexec, /etc or /opt, no program, no
//                desktop file, no Flatpak (applications, runtimes and extensions), no snap, no
//                AppImage and no running process.
// Every other folder is left alone. A folder that a running program has open is locked.
//
// Cleaning moves the folder to the Trash with `gio trash`, so a mistake can be undone;
// it never deletes anything for good. The delete command checks every path again.

use super::user_cache::{dir_size_bytes, Live};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which folder of the home an entry is in.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Root {
    Config,
    Share,
    State,
    Home,
    Flatpak,
    Snap,
}

impl Root {
    fn label(self) -> &'static str {
        match self {
            Root::Config => "config",
            Root::Share => "share",
            Root::State => "state",
            Root::Home => "home",
            Root::Flatpak => "flatpak",
            Root::Snap => "snap",
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub name: String,
    pub path: String,
    /// The path with the home folder as `~`, for the table.
    pub display: String,
    /// `config`, `share`, `state`, `home`, `flatpak` or `snap`.
    pub location: String,
    pub size_bytes: u64,
    /// `package`, `flatpak` or `snap`.
    pub reason: String,
    /// The package that was removed, for `package`.
    pub package: Option<String>,
    /// The day it was removed (`2026-10-02`), when the log says it.
    pub removed_on: Option<String>,
    /// Why it is not cleaned: `open`.
    pub protected_reason: Option<String>,
}

fn xdg(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty() && Path::new(v).is_absolute()).map(PathBuf::from)
}

/// The folders that are looked in. The three of the XDG specification follow their
/// variables when they are set.
fn roots(home: &Path) -> Vec<(Root, PathBuf)> {
    roots_with(home, xdg("XDG_CONFIG_HOME"), xdg("XDG_DATA_HOME"), xdg("XDG_STATE_HOME"))
}

fn roots_with(home: &Path, config: Option<PathBuf>, share: Option<PathBuf>, state: Option<PathBuf>) -> Vec<(Root, PathBuf)> {
    vec![
        (Root::Config, config.unwrap_or_else(|| home.join(".config"))),
        (Root::Share, share.unwrap_or_else(|| home.join(".local/share"))),
        (Root::State, state.unwrap_or_else(|| home.join(".local/state"))),
        (Root::Home, home.to_path_buf()),
        (Root::Flatpak, home.join(".var/app")),
        (Root::Snap, home.join("snap")),
    ]
}

/// The form a name is compared in: without the leading dot and in lower case. Nothing else is
/// changed, so only the same name matches.
fn norm(name: &str) -> String {
    name.trim_start_matches('.').to_lowercase()
}

// ---- What is installed ---------------------------------------------------------

fn run_lines(program: &str, args: &[&str]) -> Option<Vec<String>> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

/// Installed packages, and the ones that were removed with their configuration left.
fn dpkg_state() -> (HashSet<String>, HashSet<String>) {
    let mut installed = HashSet::new();
    let mut config_only = HashSet::new();
    for line in run_lines("dpkg-query", &["-W", "-f=${Package}\t${db:Status-Abbrev}\n"]).unwrap_or_default() {
        let mut fields = line.split('\t');
        let (Some(name), Some(status)) = (fields.next(), fields.next()) else { continue };
        let name = name.split(':').next().unwrap_or(name).to_string();
        match status.trim() {
            "rc" => {
                config_only.insert(name);
            }
            s if s.starts_with('i') || s.starts_with('h') => {
                installed.insert(name);
            }
            _ => {}
        }
    }
    (installed, config_only)
}

/// The packages that dpkg's log says were removed, with the day of the last action,
/// when that action was a removal. A package installed again afterwards is not.
fn removed_from_log(lines: impl Iterator<Item = String>) -> HashMap<String, String> {
    let mut last: HashMap<String, (String, bool)> = HashMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let removed = match fields[2] {
            "remove" | "purge" => true,
            "install" | "upgrade" => false,
            _ => continue,
        };
        let name = fields[3].split(':').next().unwrap_or(fields[3]).to_string();
        last.insert(name, (fields[0].to_string(), removed));
    }
    last.into_iter().filter(|(_, (_, removed))| *removed).map(|(name, (day, _))| (name, day)).collect()
}

fn log_lines() -> Vec<String> {
    let mut files: Vec<PathBuf> = fs::read_dir("/var/log").map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("dpkg.log"))).collect()).unwrap_or_default();
    // The oldest first: dpkg.log.12.gz ... dpkg.log.1, then dpkg.log.
    let number = |p: &PathBuf| -> i32 {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name.trim_start_matches("dpkg.log").trim_start_matches('.').trim_end_matches(".gz").parse::<i32>().map(|n| -n).unwrap_or(1)
    };
    files.sort_by_key(number);
    let mut lines = Vec::new();
    for file in files {
        let text = if file.extension().is_some_and(|e| e == "gz") {
            Command::new("zcat").arg(&file).stderr(Stdio::null()).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        } else {
            fs::read_to_string(&file).ok()
        };
        lines.extend(text.unwrap_or_default().lines().map(str::to_string));
    }
    lines
}

/// The folders that installed packages put at the top of /usr/share, /usr/lib, /usr/libexec,
/// /etc and /opt (`/usr/share/gnupg/...` gives `gnupg`), from dpkg's file lists. Only the first
/// folder counts: one deeper down (`/usr/share/orca/scripts/apps/thunderbird`) is a file of
/// another program that talks about it.
fn package_top_folders(info: &Path, installed: &HashSet<String>) -> HashSet<String> {
    const PREFIXES: &[&str] = &["/usr/share/", "/usr/lib/", "/usr/libexec/", "/etc/", "/opt/"];
    let mut names = HashSet::new();
    let Ok(read) = fs::read_dir(info) else { return names };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "list") {
            continue;
        }
        // dpkg keeps the list of a package that was removed with its configuration left: only
        // the installed ones count.
        let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !installed.contains(stem.split(':').next().unwrap_or(&stem)) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            for prefix in PREFIXES {
                if let Some((folder, _)) = line.strip_prefix(prefix).and_then(|rest| rest.split_once('/')) {
                    if !folder.is_empty() {
                        names.insert(norm(folder));
                    }
                }
            }
        }
    }
    names
}

/// Adds the names a file of a program gives: its name, its name without the extension, the last
/// part of a reverse domain name (`org.mozilla.Thunderbird` gives `thunderbird`) and, for an
/// AppImage, the name before its version (`Stacer-1.8.0-x86_64.AppImage` gives `stacer`).
fn add_program_file(names: &mut HashSet<String>, file: &str) {
    let lower = norm(file);
    if lower.is_empty() {
        return;
    }
    let stem = [".desktop", ".appimage"].iter().find_map(|ext| lower.strip_suffix(ext)).unwrap_or(&lower).to_string();
    if lower.ends_with(".appimage") {
        if let Some(first) = stem.split(['-', '_', '.', ' ']).next().filter(|f| !f.is_empty()) {
            names.insert(first.to_string());
        }
    }
    if let Some(last) = stem.rsplit('.').next().filter(|l| !l.is_empty()) {
        names.insert(last.to_string());
    }
    names.insert(stem);
    names.insert(lower);
}

fn desktop_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
        home.join(".local/share/applications"),
        home.join(".local/share/flatpak/exports/share/applications"),
    ]
}

/// The programs a desktop file names: what it runs, what it tries, and its window class.
fn desktop_programs(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else { continue };
        match key {
            "StartupWMClass" | "TryExec" => names.push(value.rsplit('/').next().unwrap_or(value).to_string()),
            "Exec" => {
                // The program is the first word that is not `env` or a variable.
                if let Some(word) = value.split_whitespace().find(|w| *w != "env" && !w.contains('=')) {
                    let word = word.trim_matches('"');
                    names.push(word.rsplit('/').next().unwrap_or(word).to_string());
                }
            }
            _ => {}
        }
    }
    names
}

/// Every name that something installed or running has, in the form of `norm`.
fn installed_names(home: &Path, packages: &HashSet<String>, flatpak_refs: &HashSet<String>, snaps: &HashSet<String>, live: &Live) -> HashSet<String> {
    let mut names: HashSet<String> = packages.iter().map(|p| norm(p)).collect();
    names.extend(package_top_folders(Path::new("/var/lib/dpkg/info"), packages));
    for dir in desktop_dirs(home) {
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !file.ends_with(".desktop") {
                continue;
            }
            add_program_file(&mut names, &file);
            if let Ok(text) = fs::read_to_string(entry.path()) {
                for program in desktop_programs(&text) {
                    add_program_file(&mut names, &program);
                }
            }
        }
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/usr/bin", "/usr/sbin", "/usr/local/bin", "/snap/bin", "/opt", "/var/lib/flatpak/exports/bin"].iter().map(PathBuf::from));
    for sub in [".local/bin", "bin", "AppImages", "Applications", ".local/share/flatpak/exports/bin"] {
        dirs.push(home.join(sub));
    }
    for dir in dirs {
        if let Ok(read) = fs::read_dir(&dir) {
            for entry in read.flatten() {
                add_program_file(&mut names, &entry.file_name().to_string_lossy());
            }
        }
    }
    for id in flatpak_refs {
        add_program_file(&mut names, id);
    }
    names.extend(snaps.iter().map(|s| norm(s)));
    names.extend(live.comms.iter().map(|c| norm(c)));
    names
}

struct Context {
    /// Every name of something installed or running (see `installed_names`).
    installed: HashSet<String>,
    /// The packages that were removed, with the day when the log says it.
    removed: HashMap<String, String>,
    /// Installed Flatpak application ids, lower case; None when they cannot be known.
    flatpaks: Option<HashSet<String>>,
    /// Installed snaps; None when they cannot be known.
    snaps: Option<HashSet<String>>,
    live: Live,
}

impl Context {
    fn capture(home: &Path) -> Context {
        let (packages, config_only) = dpkg_state();
        let mut removed = removed_from_log(log_lines().into_iter());
        // A package that is installed is not removed, whatever an old line of the log says.
        removed.retain(|name, _| !packages.contains(name));
        for name in config_only {
            removed.entry(name).or_default();
        }
        // `flatpak` and `snap` that are installed but do not answer leave their folders
        // out of the scan; when they are not installed, every folder of theirs is a leftover.
        let lower_set = |lines: Vec<String>| -> HashSet<String> { lines.into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect() };
        let (flatpaks, flatpak_refs) = if command_exists("flatpak") {
            (run_lines("flatpak", &["list", "--app", "--columns=application"]).map(lower_set), run_lines("flatpak", &["list", "--columns=application"]).map(lower_set))
        } else {
            (Some(HashSet::new()), Some(HashSet::new()))
        };
        let snaps = if command_exists("snap") {
            run_lines("snap", &["list"]).map(|l| l.into_iter().skip(1).filter_map(|s| s.split_whitespace().next().map(|n| n.to_lowercase())).collect())
        } else {
            Some(HashSet::new())
        };
        let live = Live::capture();
        let mut installed = installed_names(home, &packages, &flatpak_refs.clone().unwrap_or_default(), &snaps.clone().unwrap_or_default(), &live);
        // When the list of Flatpaks or snaps cannot be read, a name of theirs cannot be ruled
        // out: no removed package counts as proof then.
        if flatpak_refs.is_none() || snaps.is_none() {
            installed.extend(removed.keys().map(|k| norm(k)));
        }
        Context { installed, removed, flatpaks, snaps, live }
    }
}

// ---- Scan ------------------------------------------------------------------------

/// The folders that are candidates in a root: real folders (never links), and in the
/// home only the ones that start with a dot.
fn candidates(root: Root, dir: &Path, home: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    // The places that the standards of freedesktop define (the base folders `~/.config`, `~/.local`,
    // `~/.cache`, and the Trash of the Trash specification), and `~/.var` and `~/snap`, hold
    // folders; they are not folders of a program.
    let cache = xdg("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
    let trash = xdg("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join("Trash");
    let bases: Vec<PathBuf> = roots(home).into_iter().map(|(_, dir)| dir).chain([cache, trash]).collect();
    let mut found: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| root != Root::Home || (e.file_name().to_string_lossy().starts_with('.') && e.file_name().len() > 1))
        .map(|e| e.path())
        .filter(|path| !bases.iter().any(|base| base == path || base.starts_with(path)))
        .collect();
    found.sort();
    found
}

fn display_of(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// What is left in one root, with its proof, before the sizes are measured.
fn classify(root: Root, dir: &Path, home: &Path, ctx: &Context) -> Vec<Leftover> {
    let mut out = Vec::new();
    for path in candidates(root, dir, home) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (reason, package, removed_on) = match root {
            Root::Flatpak | Root::Snap => {
                let known = if root == Root::Flatpak { &ctx.flatpaks } else { &ctx.snaps };
                match known {
                    Some(set) if !set.contains(&name.to_lowercase()) => (root.label().to_string(), None, None),
                    _ => continue,
                }
            }
            _ => {
                let key = norm(&name);
                if key.is_empty() || ctx.installed.contains(&key) {
                    continue;
                }
                match ctx.removed.get(&key) {
                    Some(day) => ("package".to_string(), Some(key.clone()), (!day.is_empty()).then(|| day.clone())),
                    None => continue,
                }
            }
        };
        out.push(Leftover {
            display: display_of(home, &path),
            location: root.label().to_string(),
            protected_reason: ctx.live.is_open(&path).then(|| "open".to_string()),
            path: path.to_string_lossy().into_owned(),
            name,
            size_bytes: 0,
            reason,
            package,
            removed_on,
        });
    }
    out
}

fn scan_blocking() -> Vec<Leftover> {
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let home = PathBuf::from(home);
    let ctx = Context::capture(&home);
    let mut found: Vec<Leftover> = Vec::new();
    for (root, dir) in roots(&home) {
        found.extend(classify(root, &dir, &home, &ctx));
    }
    // The sizes are measured with `du`, a few folders at a time.
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let chunk = found.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for part in found.chunks_mut(chunk) {
            scope.spawn(move || {
                for entry in part {
                    entry.size_bytes = dir_size_bytes(Path::new(&entry.path));
                }
            });
        }
    });
    found.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    found
}

#[tauri::command]
pub async fn scan_leftovers() -> Vec<Leftover> {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or_default()
}

// ---- Move to the Trash ---------------------------------------------------------------

/// Checks one path again, whatever the interface sent: a real folder directly inside one of
/// the roots, hidden when it is in the home itself, and not open by a running program.
fn check_path(roots: &[(Root, PathBuf)], path: &Path, live: &Live) -> Result<(), String> {
    let parent = path.parent().ok_or("it has no parent")?;
    let root = roots.iter().find(|(_, dir)| dir == parent).map(|(root, _)| *root).ok_or_else(|| format!("Refusing a path outside the folders of the home: {}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_dir() {
        return Err(format!("Not a folder: {}", path.display()));
    }
    if root == Root::Home && !(name.starts_with('.') && name.len() > 1) {
        return Err(format!("Refusing a folder of the home that is not hidden: {}", path.display()));
    }
    if live.is_open(path) {
        return Err(format!("A running program has it open: {}", path.display()));
    }
    Ok(())
}

#[tauri::command]
pub async fn trash_leftovers(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is not set")?);
        if !command_exists("gio") {
            return Err("gio is not installed: nothing was moved, and nothing is deleted for good".to_string());
        }
        let all = roots(&home);
        // The whole request is checked before anything moves: what it asks for must be a folder
        // of the home, and it must be a leftover now, by the same reading of the computer that
        // listed it. A folder whose program was installed since is not.
        let ctx = Context::capture(&home);
        for path in &paths {
            let target = Path::new(path);
            check_path(&all, target, &ctx.live)?;
            let (root, parent) = all.iter().find(|(_, dir)| Some(dir.as_path()) == target.parent()).map(|(r, d)| (*r, d.clone())).ok_or("outside the folders of the home")?;
            if !classify(root, &parent, &home, &ctx).iter().any(|l| Path::new(&l.path) == target && l.protected_reason.is_none()) {
                return Err(format!("It is not a leftover: {path}"));
            }
        }
        let mut failed = Vec::new();
        for path in &paths {
            let ok = Command::new("gio").args(["trash", "--"]).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if !ok && Path::new(path).exists() {
                failed.push(path.clone());
            }
        }
        if failed.is_empty() { Ok(()) } else { Err(format!("Could not move to the Trash: {}", failed.join(", "))) }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-lo-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn context(installed: &[&str], removed: &[(&str, &str)], flatpaks: &[&str]) -> Context {
        Context {
            installed: installed.iter().map(|n| norm(n)).collect(),
            removed: removed.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect(),
            flatpaks: Some(flatpaks.iter().map(|s| s.to_string()).collect()),
            snaps: Some(HashSet::new()),
            live: Live::default(),
        }
    }

    #[test]
    fn the_log_tells_what_was_removed_and_a_reinstall_cancels_it() {
        let log = [
            "2026-10-01 10:00:00 install vlc:amd64 <none> 3.0",
            "2026-10-02 11:00:00 remove vlc:amd64 3.0 <none>",
            "2026-10-03 09:00:00 install gimp:amd64 <none> 2.10",
            "2026-10-04 09:00:00 remove gimp:amd64 2.10 <none>",
            "2026-10-05 09:00:00 install gimp:amd64 <none> 2.10",
            "2026-10-06 09:00:00 status installed foo:amd64 1.0",
            "2026-10-07 09:00:00 purge kate:amd64 24 <none>",
        ];
        let removed = removed_from_log(log.iter().map(|s| s.to_string()));
        assert_eq!(removed.get("vlc").map(String::as_str), Some("2026-10-02"));
        assert_eq!(removed.get("kate").map(String::as_str), Some("2026-10-07"));
        assert!(!removed.contains_key("gimp") && !removed.contains_key("foo"));
    }

    #[test]
    fn only_the_exact_name_of_a_removed_package_is_proof() {
        let home = scratch("classify");
        let config = home.join(".config");
        for name in ["thunderbird", "libreoffice", "mystery", "vlc", "MangoHud"] {
            fs::create_dir_all(config.join(name)).unwrap();
        }
        fs::create_dir_all(home.join(".thunderbird")).unwrap();
        // A plain file is never a candidate.
        fs::write(config.join("thunderbird.conf"), "x").unwrap();
        // `vlc` was removed but is installed another way; `mangohud` was removed but a Flatpak
        // extension named like it is installed.
        let ctx = context(&["vlc", "mangohud"], &[("thunderbird", "2026-11-12"), ("libreoffice-calc", ""), ("vlc", ""), ("mangohud", "")], &[]);
        let mut found: Vec<(String, String)> = classify(Root::Config, &config, &home, &ctx).into_iter().map(|l| (l.display, l.reason)).collect();
        found.extend(classify(Root::Home, &home, &home, &ctx).into_iter().map(|l| (l.display, l.reason)));
        found.sort();
        assert_eq!(found, vec![("~/.config/thunderbird".to_string(), "package".to_string()), ("~/.thunderbird".to_string(), "package".to_string())]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn program_files_give_their_names() {
        let mut names = HashSet::new();
        add_program_file(&mut names, "Stacer-1.8.0-x86_64.AppImage");
        add_program_file(&mut names, "org.freedesktop.Platform.VulkanLayer.MangoHud");
        add_program_file(&mut names, "org.mozilla.Thunderbird.desktop");
        assert!(names.contains("stacer") && names.contains("mangohud") && names.contains("thunderbird"));
        let programs = desktop_programs("[Desktop Entry]\nName=VLC media player\nExec=env FOO=1 /usr/bin/vlc --started-from-file %U\nStartupWMClass=vlc-main\n[Desktop Action x]\nExec=other\n");
        assert_eq!(programs, vec!["vlc".to_string(), "vlc-main".to_string()]);
    }

    #[test]
    fn dpkg_file_lists_give_the_top_folders_of_installed_packages() {
        let info = scratch("dpkg");
        fs::write(info.join("gpgconf.list"), "/usr/share/gnupg\n/usr/share/gnupg/help.txt\n/usr/bin/gpgconf\n").unwrap();
        fs::write(info.join("orca.list"), "/usr/share/orca/scripts/apps/thunderbird/script.py\n").unwrap();
        fs::write(info.join("gone.list"), "/opt/gone/bin/gone\n").unwrap();
        let installed: HashSet<String> = ["gpgconf".to_string(), "orca".to_string()].into_iter().collect();
        let names = package_top_folders(&info, &installed);
        assert!(names.contains("gnupg") && names.contains("orca"));
        assert!(!names.contains("thunderbird") && !names.contains("gone") && !names.contains("gpgconf"));
        let _ = fs::remove_dir_all(&info);
    }

    #[test]
    fn flatpak_data_of_an_application_that_is_not_installed_is_a_leftover() {
        let home = scratch("flatpak");
        let apps = home.join(".var/app");
        fs::create_dir_all(apps.join("org.example.Gone")).unwrap();
        fs::create_dir_all(apps.join("org.example.Here")).unwrap();
        let found = classify(Root::Flatpak, &apps, &home, &context(&[], &[], &["org.example.here"]));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].name.as_str(), found[0].reason.as_str()), ("org.example.Gone", "flatpak"));
        // A flatpak that does not answer leaves its folder out of the scan.
        let mut unknown = context(&[], &[], &[]);
        unknown.flatpaks = None;
        assert!(classify(Root::Flatpak, &apps, &home, &unknown).is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn the_home_only_offers_hidden_folders() {
        let home = scratch("home");
        fs::create_dir_all(home.join(".oldtool")).unwrap();
        fs::create_dir_all(home.join("Documentos")).unwrap();
        fs::create_dir_all(home.join("Descargas")).unwrap();
        let names: Vec<String> = candidates(Root::Home, &home, &home).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec![".oldtool".to_string()]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn the_trash_command_refuses_what_it_must() {
        let home = scratch("check");
        let config = home.join(".config");
        fs::create_dir_all(config.join("old-editor")).unwrap();
        fs::create_dir_all(home.join("Documentos")).unwrap();
        fs::create_dir_all(home.join(".oldtool")).unwrap();
        fs::write(config.join("file"), "x").unwrap();
        let live = Live::default();
        let all = roots_with(&home, None, None, None);
        assert!(check_path(&all, &config.join("old-editor"), &live).is_ok());
        assert!(check_path(&all, &home.join(".oldtool"), &live).is_ok());
        // The user's own folders, files and anything outside are refused.
        assert!(check_path(&all, &home.join("Documentos"), &live).is_err());
        assert!(check_path(&all, &config.join("file"), &live).is_err());
        assert!(check_path(&all, Path::new("/etc"), &live).is_err());
        assert!(check_path(&all, &config.join("old-editor/inner"), &live).is_err());
        // A folder that a running program has open.
        let mut busy = Live::default();
        busy.open.insert(config.join("old-editor/data.db"));
        assert!(check_path(&all, &config.join("old-editor"), &busy).is_err());
        let _ = fs::remove_dir_all(&home);
    }
}
