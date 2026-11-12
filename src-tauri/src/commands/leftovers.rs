// What uninstalled programs leave in the home folder: their folders in ~/.config,
// ~/.local/share, ~/.local/state, the dot folders of the home itself, ~/.var/app (Flatpak)
// and ~/snap. Entirely user-scope, never needs root.
//
// Every folder is made by a program. If that program is not installed, the folder is a leftover.
// A folder is claimed (and left alone) when something installed owns it:
//   - its name is the one of a package, of a desktop file, of a program in the PATH, of a
//     Flatpak, of a snap, of a running process, or of a folder that a package installs in
//     /usr/share, /usr/lib, /opt or /etc (dpkg's file lists);
//   - its entries have the name of a program that a person runs: a program that shares a folder
//     with others makes one of its own in it (`~/.gemini/antigravity`).
// A name that is only written inside another program says nothing about who makes the folder:
// an editor and its extensions name `.gradle`, `.dotnet` or `.cline` because they talk about
// those tools. And how long ago a folder was used says nothing either: an editor touches
// `~/.gradle` every day without Gradle being installed, and an installed program can stay
// unopened for months.
// What nothing claims is a leftover, and the table says what is known about it:
//   - "creator":   the service `soplos-sys-cleaner-tracker` saw which program made the folder,
//                  and that program is not installed any more. This is the proof that works for
//                  every kind of package and for programs of any date; the others are what can be
//                  known about the folders that were made before the service ran;
//   - "refs":      the files of the folder name where their program is (`/opt/brave.com`,
//                  `/usr/bin/foo`) and none of those exists any more; if one of them exists the
//                  program is installed and the folder is its;
//   - "package":   dpkg's log says a package with that name was removed (or only its
//                  configuration is left, state `rc`);
//   - "flatpak":   ~/.var/app/<id> of an application that is not installed;
//   - "snap":      ~/snap/<name> of a snap that is not installed;
//   - "unclaimed": nothing installed owns it.
// The folders of the desktop itself (settings of Plasma, GNOME, Xfce, the audio server, the
// input methods...) are never listed, and a folder that a running program has open is locked.
//
// Cleaning moves the folder to the Trash with `gio trash`, so a mistake can be undone;
// it never deletes anything for good. The delete command checks every path again.

use super::user_cache::{dir_size_bytes, Live};
use crate::tracker::{read_store, Creator, Creators, STORE};
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
    /// `package`, `flatpak`, `snap` or `unclaimed`.
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

// ---- Names ---------------------------------------------------------------------

/// The forms a name is compared in: lower case, without the leading dot, with
/// punctuation as dashes, with no punctuation at all, and the last part of a
/// reverse domain name (`org.kde.dolphin` gives `dolphin`).
fn keys(name: &str) -> Vec<String> {
    let lower = name.trim_start_matches('.').to_lowercase();
    let dashed: String = lower.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    let mut out = vec![dashed.clone(), dashed.chars().filter(|c| c.is_alphanumeric()).collect()];
    if let Some(last) = dashed.rsplit('.').next() {
        if last != dashed {
            out.push(last.to_string());
        }
    }
    out.retain(|k| k.chars().count() >= 3);
    out.sort();
    out.dedup();
    out
}

fn squashed(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The identifier of an application (`org.soplos.softwarecenter`) and the name of its
/// package (`soplos-software-center`) contain each other once the punctuation is gone.
/// Seven letters at least, so a short name does not match everything.
fn contain_each_other(a: &[String], b: &[String]) -> bool {
    let long = |list: &[String]| -> Vec<String> { list.iter().map(|k| squashed(k)).filter(|k| k.chars().count() >= 7).collect() };
    let (a, b) = (long(a), long(b));
    a.iter().any(|x| b.iter().any(|y| x.contains(y.as_str()) || y.contains(x.as_str())))
}

#[derive(Default)]
struct Claims {
    exact: HashSet<String>,
    /// The forms of seven letters or more with no punctuation, for `contain_each_other`.
    long: HashSet<String>,
}

impl Claims {
    fn add(&mut self, name: &str) {
        for key in keys(name) {
            let flat = squashed(&key);
            if flat.chars().count() >= 7 {
                self.long.insert(flat);
            }
            self.exact.insert(key);
        }
    }

    /// Whether this exact name (in any of its forms) is one that was added.
    fn has_exact(&self, name: &str) -> bool {
        keys(name).iter().any(|k| self.exact.contains(k))
    }

    /// Whether something installed or running has this name, or a name that
    /// extends it with a dash (`libreoffice` and `libreoffice-writer`).
    fn has(&self, name: &str) -> bool {
        let found = keys(name);
        if found.iter().any(|k| self.exact.contains(k)) {
            return true;
        }
        let wanted: Vec<String> = found.iter().map(|k| squashed(k)).filter(|k| k.chars().count() >= 7).collect();
        if wanted.iter().any(|w| self.long.iter().any(|c| c.contains(w.as_str()) || w.contains(c.as_str()))) {
            return true;
        }
        found.iter().filter(|k| k.chars().count() >= 4).any(|k| {
            self.exact.iter().any(|claim| {
                (claim.len() > k.len() && claim.starts_with(k.as_str()) && claim.as_bytes()[k.len()] == b'-')
                    || (k.len() > claim.len() && claim.chars().count() >= 4 && k.starts_with(claim.as_str()) && k.as_bytes()[claim.len()] == b'-')
            })
        })
    }
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

/// The names a desktop file gives: its own, the program it runs, the window class and
/// the icon (when it is a name and not a path).
fn desktop_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "Name" | "StartupWMClass" | "X-Flatpak" | "TryExec" => names.push(value.rsplit('/').next().unwrap_or(value).to_string()),
            "Icon" if !value.starts_with('/') => names.push(value.to_string()),
            "Exec" => {
                // The program is the first word that is not `env` or a variable.
                if let Some(word) = value.split_whitespace().find(|w| *w != "env" && !w.contains('=')) {
                    names.push(word.rsplit('/').next().unwrap_or(word).to_string());
                }
            }
            _ => {}
        }
    }
    names
}

/// The names that identify the program of a desktop file: the program it runs, its window class,
/// its Flatpak id. Not its display name or its icon, which are words (`Settings`, `text-editor`).
fn desktop_program_names(text: &str) -> Vec<String> {
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
            "StartupWMClass" | "X-Flatpak" | "TryExec" => names.push(value.rsplit('/').next().unwrap_or(value).to_string()),
            "Exec" => {
                if let Some(word) = value.split_whitespace().find(|w| *w != "env" && !w.contains('=')) {
                    names.push(word.rsplit('/').next().unwrap_or(word).to_string());
                }
            }
            _ => {}
        }
    }
    names
}

/// The folder names that the installed packages put in the system, from dpkg's file lists: a
/// package that installs /usr/share/<name> or /opt/<name> is the one that creates
/// ~/.config/<name>.
fn package_folder_names(info: &Path, installed: &HashSet<String>, skip: &HashSet<String>) -> HashSet<String> {
    let mut names = HashSet::new();
    let Ok(read) = fs::read_dir(info) else { return names };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "list") {
            continue;
        }
        // Only a package that is installed counts: dpkg keeps the file list of a package that was
        // removed and left its configuration (state `rc`). And a package that nothing needs is not
        // a program of the person (see `orphans`).
        let package = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let package = package.split(':').next().unwrap_or(&package);
        if !installed.contains(package) || skip.contains(package) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            for prefix in ["/usr/share/", "/usr/lib/", "/usr/libexec/", "/opt/", "/etc/"] {
                let Some(rest) = line.strip_prefix(prefix) else { continue };
                // Every folder on the way (`/etc/xdg/menus/x.menu` gives `xdg` and `menus`): a
                // program that installs a folder is the one that makes it in the home.
                let parts: Vec<&str> = rest.split('/').collect();
                for (i, part) in parts.iter().enumerate() {
                    if part.is_empty() || (i + 1 == parts.len() && i > 0) {
                        continue;
                    }
                    // The files at the top of a folder (`/etc/foo.conf`) say their name without
                    // the extension.
                    names.insert(part.split('.').next().filter(|n| !n.is_empty()).unwrap_or(part).to_string());
                    names.insert(part.to_string());
                }
            }
        }
    }
    names
}

/// The programs that the editors of the VS Code family have installed as extensions: the
/// extension of a folder `~/.<editor>/extensions/<publisher>.<name>-<version>` is a program too,
/// and it makes folders of its own (`vscjava.vscode-gradle` makes `~/.gradle`). Each word of the
/// id and the `name` and `displayName` of its `package.json` are its names.
fn extension_names(home: &Path) -> Vec<String> {
    let mut names = Vec::new();
    // The words of each extension's folder. A word that most extensions have (`universal`,
    // `linux`, `x64`, the name of a publisher) names none of them, and is dropped: what is left
    // is what is special to each.
    let mut words_of: Vec<Vec<String>> = Vec::new();
    let Ok(editors) = fs::read_dir(home) else { return names };
    for editor in editors.flatten() {
        if !editor.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(extensions) = fs::read_dir(editor.path().join("extensions")) else { continue };
        for extension in extensions.flatten() {
            let id = extension.file_name().to_string_lossy().into_owned();
            let mut words: Vec<String> = id
                .split(['.', '-', '_'])
                .filter(|w| w.chars().count() >= 4 && !w.chars().all(|c| c.is_ascii_digit()))
                .map(|w| w.to_lowercase())
                .collect();
            words.sort();
            words.dedup();
            words_of.push(words);
            if let Ok(text) = fs::read_to_string(extension.path().join("package.json")) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    for key in ["name", "displayName"] {
                        if let Some(value) = json[key].as_str().filter(|v| !v.starts_with('%')) {
                            names.push(value.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for words in &words_of {
        for word in words {
            *count.entry(word.as_str()).or_default() += 1;
        }
    }
    let total = words_of.len();
    for words in &words_of {
        for word in words {
            if count[word.as_str()] * 5 <= total * 2 {
                names.push(word.clone());
            }
        }
    }
    names
}

struct Context {
    claims: Claims,
    removed: HashMap<String, String>,
    /// Installed Flatpak application ids, lower case; None when they cannot be known.
    flatpaks: Option<HashSet<String>>,
    /// Installed snaps; None when they cannot be known.
    snaps: Option<HashSet<String>>,
    live: Live,
    /// The names of the programs that a person runs: what the menu starts, what is installed in
    /// /opt, the AppImages, the Flatpaks and the snaps. Narrower than `claims`, which also has
    /// every package and every program in the PATH.
    apps: Claims,
    /// Who made each folder, from the service `soplos-sys-cleaner-tracker` (see `tracker.rs`).
    creators: Creators,
    /// What the files of each folder point to (see `program_references`), read once.
    references: HashMap<PathBuf, HashSet<String>>,
    /// Programs that say nothing about who made a folder: the ones of the base of the system, and
    /// the ones that many folders point to (a program that half the folders name is a tool that
    /// everything uses, not the program that made them).
    common: HashSet<String>,
}

impl Context {
    fn capture(home: &Path) -> Context {
        let (installed, config_only) = dpkg_state();
        // What apt itself would autoremove: libraries and tools that were installed for a program
        // that is gone. They are on the disk but they are not what a person has installed, and the
        // folders that their files carry the name of (`/usr/lib/libreoffice` in the Java library
        // of a suite that was removed) are not theirs.
        let orphans: HashSet<String> = crate::commands::orphans::autoremove_names().unwrap_or_default().into_iter().map(|n| n.split(':').next().unwrap_or(&n).to_string()).collect();
        let mut claims = Claims::default();
        for name in installed.iter().filter(|n| !orphans.contains(*n)) {
            claims.add(name);
        }
        // The folders that the installed packages put in the system: a package that installs
        // /usr/share/<name> or /opt/<name> is the one that creates ~/.config/<name>.
        for name in package_folder_names(Path::new("/var/lib/dpkg/info"), &installed, &orphans) {
            claims.add(&name);
        }
        let mut apps = Claims::default();
        for dir in desktop_dirs(home) {
            let Ok(read) = fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                if let Some(stem) = path.file_stem() {
                    claims.add(&stem.to_string_lossy());
                    apps.add(&stem.to_string_lossy());
                }
                if let Ok(text) = fs::read_to_string(&path) {
                    for name in desktop_names(&text) {
                        claims.add(&name);
                    }
                    for name in desktop_program_names(&text) {
                        apps.add(&name);
                    }
                }
            }
        }
        let mut bin_dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        bin_dirs.extend(["/usr/bin", "/usr/local/bin", "/snap/bin"].iter().map(PathBuf::from));
        bin_dirs.push(home.join(".local/bin"));
        bin_dirs.push(home.join("bin"));
        // The programs that install themselves in their own folder of the home
        // (`~/.cargo/bin`, `~/.bun/bin`, `~/.opencode/bin`, `~/go/bin`...). The session that
        // starts this program does not always have them in its PATH.
        bin_dirs.push(home.join("go/bin"));
        if let Ok(read) = fs::read_dir(home) {
            for entry in read.flatten() {
                if entry.file_name().to_string_lossy().starts_with('.') && entry.file_type().is_ok_and(|t| t.is_dir()) {
                    bin_dirs.push(entry.path().join("bin"));
                }
            }
        }
        for dir in bin_dirs {
            if let Ok(read) = fs::read_dir(&dir) {
                for entry in read.flatten() {
                    claims.add(&entry.file_name().to_string_lossy());
                }
            }
        }
        for dir in [PathBuf::from("/opt"), home.join("Applications"), home.join("AppImages")] {
            if let Ok(read) = fs::read_dir(&dir) {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    claims.add(name.trim_end_matches(".AppImage").trim_end_matches(".appimage"));
                    apps.add(name.trim_end_matches(".AppImage").trim_end_matches(".appimage"));
                }
            }
        }
        // The extensions of the editors are installed programs too.
        for name in extension_names(home) {
            claims.add(&name);
            apps.add(&name);
        }
        let live = Live::capture();
        for comm in &live.comms {
            claims.add(comm);
        }

        let mut removed = removed_from_log(log_lines().into_iter());
        // A package that is installed is not removed, whatever an old line of the log says.
        removed.retain(|name, _| !installed.contains(name));
        for name in config_only {
            removed.entry(name).or_default();
        }

        // `flatpak` and `snap` that are installed but do not answer leave their folders
        // out of the scan; when they are not installed, every folder of theirs is a leftover.
        let flatpaks = if command_exists("flatpak") {
            run_lines("flatpak", &["list", "--app", "--columns=application"]).map(|l| l.into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect())
        } else {
            Some(HashSet::new())
        };
        let snaps = if command_exists("snap") {
            run_lines("snap", &["list"]).map(|l| l.into_iter().skip(1).filter_map(|s| s.split_whitespace().next().map(|n| n.to_lowercase())).collect())
        } else {
            Some(HashSet::new())
        };
        // What is installed through Flatpak or snap also claims the folders named like it.
        for name in flatpaks.iter().chain(snaps.iter()).flatten() {
            claims.add(name);
            apps.add(name);
        }
        Context { claims, removed, flatpaks, snaps, live, apps, creators: read_store(Path::new(STORE)), references: HashMap::new(), common: crate::tracker::base_system_programs() }
    }

    /// The package of the removal log that this folder belongs to, with the day.
    fn removed_package(&self, name: &str) -> Option<(String, String)> {
        let wanted: Vec<String> = keys(name);
        self.removed.iter().find(|(package, _)| keys(package).iter().any(|k| wanted.contains(k)) || contain_each_other(&wanted, &keys(package))).map(|(package, day)| (package.clone(), day.clone()))
    }

    /// Whether the program that made a folder is still on the computer. When it cannot be told
    /// (the list of Flatpaks did not answer, the executable was in a temporary folder), the answer
    /// is yes: nothing is offered for removal on a doubt.
    fn creator_installed(&self, creator: &Creator) -> bool {
        match creator.kind.as_str() {
            "flatpak" => self.flatpaks.as_ref().is_none_or(|set| set.contains(&creator.program.to_lowercase())),
            "snap" => self.snaps.as_ref().is_none_or(|set| set.contains(&creator.program.to_lowercase())),
            _ => {
                let program = Path::new(&creator.program);
                // A path in a temporary folder or inside a mounted AppImage is not where the
                // program lives.
                program.starts_with("/tmp") || program.starts_with("/proc") || creator.program.contains("/.mount_") || program.exists()
            }
        }
    }

    fn claimed(&self, name: &str) -> bool {
        self.claims.has(name)
    }
}

// ---- What the folder says about its program ---------------------------------------------

/// The paths of programs that the files of a folder name: `/opt/<program>`, `/usr/bin/<program>`,
/// `/usr/local/bin/<program>` and `/snap/bin/<program>`. A program writes in its folder where to
/// find itself: its executable, the folder it is installed in, what it starts. The files are
/// read in a bounded way (a number of files, a size of each) and links are not followed.
fn program_references(folder: &Path, common: &HashSet<String>) -> HashSet<String> {
    const PREFIXES: &[&str] = &["/opt/", "/usr/bin/", "/usr/local/bin/", "/snap/bin/"];
    const SKIP_DIRS: &[&str] = &["node_modules", "Cache", "cache", "GPUCache", "Code Cache", "blob_storage", "caches", "CacheStorage", "Service Worker", "IndexedDB"];
    // What is read of a folder is bounded: the files that say where a program is are small text
    // files (settings, launchers, lists), so a file is read only up to 64 KB, a big one is not
    // read, a binary one (it has a zero byte at its start) is dropped after its first bytes, and
    // the whole folder gets a budget of bytes.
    const MAX_FILE: u64 = 1_000_000;
    const READ_PER_FILE: u64 = 64_000;
    const BUDGET: usize = 3_000_000;
    let mut spent = 0usize;
    let mut found: HashSet<String> = HashSet::new();
    let mut stack = vec![(folder.to_path_buf(), 0usize)];
    let mut files = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if depth < 5 && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((entry.path(), depth + 1));
                }
                continue;
            }
            files += 1;
            if files > 300 || spent > BUDGET {
                return found;
            }
            if entry.metadata().is_ok_and(|m| m.len() > MAX_FILE || m.len() == 0) {
                continue;
            }
            let mut buffer = Vec::new();
            if let Ok(file) = fs::File::open(entry.path()) {
                use std::io::Read;
                let _ = file.take(READ_PER_FILE).read_to_end(&mut buffer);
            }
            spent += buffer.len();
            if buffer.iter().take(512).any(|b| *b == 0) {
                continue;
            }
            let mut i = 0;
            while i < buffer.len() {
                if buffer[i] != b'/' {
                    i += 1;
                    continue;
                }
                // A path starts where a word starts: after a space, a quote, `=`, `:`... and not
                // in the middle of an address (`https://host/opt/page`).
                if i > 0 && (buffer[i - 1].is_ascii_alphanumeric() || matches!(buffer[i - 1], b'.' | b'_' | b'-' | b'/' | b'%' | b'~')) {
                    i += 1;
                    continue;
                }
                let rest = &buffer[i..];
                let Some(prefix) = PREFIXES.iter().find(|p| rest.starts_with(p.as_bytes())) else {
                    i += 1;
                    continue;
                };
                let name: String = rest[prefix.len()..].iter().take_while(|b| b.is_ascii_alphanumeric() || matches!(**b, b'.' | b'_' | b'+' | b'-')).map(|b| *b as char).collect();
                if name.chars().count() >= 3 && !common.contains(&name) {
                    found.insert(format!("{prefix}{name}"));
                }
                i += prefix.len();
            }
        }
    }
    found
}

/// What the files of a folder point to: the programs that exist, and the ones that do not.
fn referenced_programs(references: &HashSet<String>, common: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let mut alive = Vec::new();
    let mut dead = Vec::new();
    for path in references.iter().filter(|p| !common.contains(base_of(p).as_str())).cloned() {
        if Path::new(&path).exists() {
            alive.push(path);
        } else {
            dead.push(path);
        }
    }
    alive.sort();
    dead.sort();
    (alive, dead)
}

// ---- Scan ------------------------------------------------------------------------

/// The names inside a folder that an installed program has: a program that shares a folder
/// with others makes a folder of its own in it (`~/.gemini/antigravity`), so the folder is the
/// one of an installed program when its entries have exactly the name of one of the programs
/// that a person runs. Short and common names (`config`, `native`, `wrapper`) say nothing.
fn names_an_installed_program(path: &Path, claims: &Claims) -> bool {
    let Ok(read) = fs::read_dir(path) else { return false };
    read.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.rsplit_once('.').map(|(stem, _)| stem.to_string()).unwrap_or(name.clone());
        stem.chars().count() >= 7 && claims.has_exact(&stem)
    })
}

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

fn base_of(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn display_of(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// What is left in one root, with the reason, before the sizes are measured.
fn classify(root: Root, dir: &Path, home: &Path, ctx: &Context) -> Vec<Leftover> {
    let mut out = Vec::new();
    let known: Option<&HashSet<String>> = match root {
        Root::Flatpak => match &ctx.flatpaks {
            Some(set) => Some(set),
            None => return out,
        },
        Root::Snap => match &ctx.snaps {
            Some(set) => Some(set),
            None => return out,
        },
        _ => None,
    };
    for path in candidates(root, dir, home) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (reason, package, removed_on) = match (root, known) {
            (Root::Flatpak, Some(set)) | (Root::Snap, Some(set)) => {
                if set.contains(&name.to_lowercase()) {
                    continue;
                }
                (root.label().to_string(), None, None)
            }
            _ => {
                // Who made the folder, when the service saw it: if that program is installed the
                // folder is its, and if it is not the folder is a leftover, with no more proof
                // than that needed.
                if let Some(creator) = ctx.creators.get(path.to_string_lossy().as_ref()) {
                    if ctx.creator_installed(creator) {
                        continue;
                    }
                    let program = match creator.kind.as_str() {
                        "exe" | "script" | "appimage" => base_of(&creator.program),
                        _ => creator.program.clone(),
                    };
                    out.push(Leftover {
                        name: name.clone(),
                        path: path.to_string_lossy().into_owned(),
                        display: display_of(home, &path),
                        location: root.label().to_string(),
                        size_bytes: 0,
                        reason: "creator".to_string(),
                        package: Some(program),
                        removed_on: None,
                        protected_reason: ctx.live.is_open(&path).then(|| "open".to_string()),
                    });
                    continue;
                }
                if ctx.claimed(&name) {
                    continue;
                }
                // The folder of a program that shares it (`~/.gemini/antigravity`): the program
                // is installed.
                if names_an_installed_program(&path, &ctx.apps) {
                    continue;
                }
                // What its own files point to: a program that exists owns it; programs that
                // are all gone are the proof that it was left behind.
                let (alive, dead) = referenced_programs(&ctx.references.get(&path).cloned().unwrap_or_default(), &ctx.common);
                if !alive.is_empty() {
                    continue;
                }
                if let Some(gone) = dead.first() {
                    out.push(Leftover {
                        name: name.clone(),
                        path: path.to_string_lossy().into_owned(),
                        display: display_of(home, &path),
                        location: root.label().to_string(),
                        size_bytes: 0,
                        reason: "refs".to_string(),
                        package: Some(base_of(gone)),
                        removed_on: None,
                        protected_reason: ctx.live.is_open(&path).then(|| "open".to_string()),
                    });
                    continue;
                }
                match ctx.removed_package(&name) {
                    Some((package, day)) => ("package".to_string(), Some(package), (!day.is_empty()).then_some(day)),
                    None => ("unclaimed".to_string(), None, None),
                }
            }
        };
        let protected_reason = ctx.live.is_open(&path).then(|| "open".to_string());
        let display = display_of(home, &path);
        out.push(Leftover { name, path: path.to_string_lossy().into_owned(), display, location: root.label().to_string(), size_bytes: 0, reason, package, removed_on, protected_reason });
    }
    out
}

/// What the files of each folder point to, read in parallel.
fn read_references(folders: &[PathBuf], common: &HashSet<String>) -> HashMap<PathBuf, HashSet<String>> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let chunk = folders.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = folders.chunks(chunk).map(|part| scope.spawn(move || part.iter().map(|f| (f.clone(), program_references(f, common))).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Everything that decides whether a folder is a leftover, read from the computer.
fn prepare(home: &Path) -> Context {
    let mut ctx = Context::capture(home);
    // The files of a folder are read only when nothing cheaper explains it: not when its name is
    // claimed, when the service saw who made it, or when it holds the folder of a program.
    let mut folders: Vec<PathBuf> = Vec::new();
    for (root, dir) in roots(home) {
        if matches!(root, Root::Flatpak | Root::Snap) {
            continue;
        }
        for path in candidates(root, &dir, home) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !ctx.claimed(&name) && !ctx.creators.contains_key(path.to_string_lossy().as_ref()) && !names_an_installed_program(&path, &ctx.apps) {
                folders.push(path);
            }
        }
    }
    ctx.references = read_references(&folders, &ctx.common);
    // A program that many folders point to is a tool that everything uses, not the program that
    // made them.
    let mut count: HashMap<String, usize> = HashMap::new();
    for references in ctx.references.values() {
        for path in references {
            *count.entry(base_of(path)).or_default() += 1;
        }
    }
    for (program, folders) in count {
        if folders >= 4 {
            ctx.common.insert(program);
        }
    }
    ctx
}

fn scan_blocking() -> Vec<Leftover> {
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let home = PathBuf::from(home);
    let ctx = prepare(&home);
    let mut found: Vec<Leftover> = Vec::new();
    for (root, dir) in roots(&home) {
        found.extend(classify(root, &dir, &home, &ctx));
    }
    // One `du` for each folder, in parallel, as the scan of ~/.cache does.
    let paths: Vec<String> = found.iter().map(|entry| entry.path.clone()).collect();
    let sizes: Vec<u64> = std::thread::scope(|scope| {
        let handles: Vec<_> = paths.iter().map(|path| scope.spawn(move || dir_size_bytes(Path::new(path)))).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or(0)).collect()
    });
    for (entry, size) in found.iter_mut().zip(sizes) {
        entry.size_bytes = size;
    }
    found.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    found
}

#[tauri::command]
pub async fn scan_leftovers() -> Vec<Leftover> {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or_default()
}

// ---- Move to the Trash ---------------------------------------------------------------

/// Checks one path again, whatever the interface sent: a real folder directly inside one of
/// the roots, not one of the desktop's own, and not open by a running program.
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
        let live = Live::capture();
        let all = roots(&home);
        // The whole request is checked before anything moves: what it asks for must be a folder
        // of the home, and it must be a leftover now, by the same reading of the computer that
        // listed it. A folder that a program installed since, or that the desktop needs, is not.
        let ctx = prepare(&home);
        for path in &paths {
            let target = Path::new(path);
            check_path(&all, target, &live)?;
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

    fn context(claims: &[&str], removed: &[(&str, &str)], flatpaks: &[&str]) -> Context {
        let mut c = Claims::default();
        for name in claims {
            c.add(name);
        }
        Context {
            claims: c,
            removed: removed.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect(),
            flatpaks: Some(flatpaks.iter().map(|s| s.to_string()).collect()),
            snaps: Some(HashSet::new()),
            live: Live::default(),
            apps: {
                let mut a = Claims::default();
                for name in claims {
                    a.add(name);
                }
                a
            },
            creators: Creators::new(),
            references: HashMap::new(),
            common: HashSet::new(),
        }
    }

    #[test]
    fn names_are_compared_in_several_forms() {
        assert!(keys("Mozilla Firefox").contains(&"mozilla-firefox".to_string()));
        assert!(keys("Mozilla Firefox").contains(&"mozillafirefox".to_string()));
        assert!(keys("org.kde.dolphin").contains(&"dolphin".to_string()));
        assert!(keys(".vlc").contains(&"vlc".to_string()));
        // Two letters say nothing.
        assert!(keys("go").is_empty());
    }

    #[test]
    fn a_claim_covers_the_folder_with_its_name_or_one_that_extends_it() {
        let mut c = Claims::default();
        c.add("libreoffice-writer");
        c.add("vlc");
        c.add("dolphin");
        assert!(c.has("libreoffice") && c.has(".vlc") && c.has("org.kde.dolphin"));
        assert!(!c.has("vim") && !c.has("libre"));
        // An application identifier and the name of its package contain each other.
        c.add("soplos-software-center");
        assert!(c.has("org.soplos.softwarecenter") && !c.has("org.soplos.other"));
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
    fn desktop_files_give_their_names() {
        let names = desktop_names("[Desktop Entry]\nName=VLC media player\nExec=env FOO=1 /usr/bin/vlc --started-from-file %U\nIcon=vlc\nStartupWMClass=vlc\n[Desktop Action x]\nName=Other\n");
        assert!(names.contains(&"vlc".to_string()) && names.contains(&"VLC media player".to_string()));
        assert!(!names.contains(&"Other".to_string()));
    }

    #[test]
    fn only_what_nothing_claims_is_listed_and_with_its_reason() {
        let home = scratch("classify");
        let config = home.join(".config");
        for name in ["vlc", "old-editor", "mystery", "claimed-app"] {
            fs::create_dir_all(config.join(name)).unwrap();
        }
        // A plain file is never a candidate.
        fs::write(config.join("somerc"), "x").unwrap();
        let ctx = context(&["claimed-app"], &[("old-editor", "2026-09-30")], &[]);
        for name in ["vlc", "old-editor", "mystery"] {
            fs::write(config.join(name).join("f"), "x").unwrap();
        }
        let found = classify(Root::Config, &config, &home, &ctx);
        let by_name = |n: &str| found.iter().find(|l| l.name == n);
        assert_eq!(by_name("old-editor").map(|l| (l.reason.as_str(), l.removed_on.clone())), Some(("package", Some("2026-09-30".to_string()))));
        assert_eq!(by_name("mystery").map(|l| l.reason.as_str()), Some("unclaimed"));
        assert_eq!(by_name("mystery").map(|l| l.display.as_str()), Some("~/.config/mystery"));
        // vlc is not claimed here either, so it is listed as a suspicion.
        assert!(by_name("vlc").is_some());
        assert!(by_name("claimed-app").is_none() && by_name("somerc").is_none());
        let _ = fs::remove_dir_all(&home);
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
    fn a_folder_with_an_entry_named_like_an_installed_program_is_that_programs() {
        let home = scratch("inside");
        fs::create_dir_all(home.join(".gemini/antigravity-ide")).unwrap();
        fs::create_dir_all(home.join(".gemini/config")).unwrap();
        fs::create_dir_all(home.join(".gradle/caches")).unwrap();
        fs::create_dir_all(home.join(".gradle/daemon")).unwrap();
        for name in [".gemini", ".gradle"] {
            fs::write(home.join(name).join("f"), "x").unwrap();
        }
        let ctx = context(&["antigravity-ide"], &[], &[]);
        let names: Vec<String> = classify(Root::Home, &home, &home, &ctx).into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec![".gradle".to_string()]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn dpkg_file_lists_give_the_folders_that_packages_install() {
        let info = scratch("dpkg");
        fs::write(info.join("antigravity.list"), "/opt/antigravity\n/opt/antigravity/bin/antigravity\n/usr/share/applications/antigravity.desktop\n/etc/foo.conf\n/usr/bin/tool\n").unwrap();
        fs::write(info.join("notalist.txt"), "/opt/ignored\n").unwrap();
        let names = package_folder_names(&info, &["antigravity".to_string()].into_iter().collect(), &HashSet::new());
        assert!(names.contains("antigravity") && names.contains("foo") && names.contains("foo.conf"));
        assert!(!names.contains("ignored") && !names.contains("tool"));
        let _ = fs::remove_dir_all(&info);
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
        // The user's own folders, the desktop's folders, files and anything outside are refused.
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
