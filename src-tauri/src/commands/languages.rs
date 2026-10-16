// Language files: translations (/usr/share/locale), desktop help
// (/usr/share/help), KDE HTML help (/usr/share/doc/HTML) and translated
// manual pages (/usr/share/man/<language>).
//
// One row per language, grouping its regional variants (es, es_ES, es_AR...)
// and summing everything that belongs to it. What is protected is decided per
// exact code: the languages the user has active (and the codes gettext falls
// back to: es_ES -> es), and English. A language whose active variant is kept
// still lists the other variants (es_MX, es_AR) as removable.
//
// The privileged side recomputes the groups itself and only ever deletes the
// paths it found: the request carries language codes, never paths.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LOCALE_DIR: &str = "/usr/share/locale";
const HELP_DIR: &str = "/usr/share/help";
const KDE_HTML_DIR: &str = "/usr/share/doc/HTML";
const MAN_DIR: &str = "/usr/share/man";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LanguageEntry {
    // Base language code: "es", "ca", "pt".
    pub base: String,
    pub remove_codes: Vec<String>,
    pub keep_codes: Vec<String>,
    // Where the removable files live: locale, help, kde, man.
    pub places: Vec<String>,
    // Bytes that removing would free; the kept size when nothing is removable.
    pub size_bytes: u64,
    // Set when nothing can be removed: "active" or "english".
    pub protected_by: Option<String>,
}

struct Found {
    code: String,
    place: &'static str,
    path: PathBuf,
}

// "es", "pt_BR", "ca_ES@valencia", "sr@ijekavianlatin", "es.ISO8859-1".
// man1, cat1, "C" and the like are not languages.
fn base_of(code: &str) -> Option<String> {
    let base = code.split(['_', '@', '.', '-']).next()?;
    if (2..=3).contains(&base.len()) && base.chars().all(|c| c.is_ascii_lowercase()) {
        Some(base.to_string())
    } else {
        None
    }
}

fn is_language_code(name: &str) -> bool {
    name.len() <= 24 && name.chars().all(|c| c.is_ascii_alphanumeric() || "_@.-".contains(c)) && base_of(name).is_some()
}

fn subdirs(path: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(path) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = fs::symlink_metadata(e.path()).ok()?;
            if meta.is_dir() {
                Some((e.file_name().to_string_lossy().into_owned(), e.path()))
            } else {
                None
            }
        })
        .collect()
}

fn found_paths() -> Vec<Found> {
    let mut found = Vec::new();
    for (name, path) in subdirs(Path::new(LOCALE_DIR)) {
        if is_language_code(&name) {
            found.push(Found { code: name, place: "locale", path });
        }
    }
    for (_, app) in subdirs(Path::new(HELP_DIR)) {
        for (name, path) in subdirs(&app) {
            if is_language_code(&name) {
                found.push(Found { code: name, place: "help", path });
            }
        }
    }
    for (name, path) in subdirs(Path::new(KDE_HTML_DIR)) {
        if is_language_code(&name) {
            found.push(Found { code: name, place: "kde", path });
        }
    }
    for (name, path) in subdirs(Path::new(MAN_DIR)) {
        if is_language_code(&name) {
            found.push(Found { code: name, place: "man", path });
        }
    }
    found
}

fn sizes(paths: &[&Path]) -> HashMap<PathBuf, u64> {
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

// ---- Active languages ------------------------------------------------------

// "es_ES.UTF-8" -> the codes gettext tries, most specific first:
// es_ES, es.
fn fallback_chain(value: &str) -> Vec<String> {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    let without_charset = match value.split_once('.') {
        Some((head, tail)) => match tail.split_once('@') {
            Some((_, modifier)) => format!("{head}@{modifier}"),
            None => head.to_string(),
        },
        None => value.to_string(),
    };
    if without_charset.is_empty() || without_charset == "C" || without_charset == "POSIX" {
        return Vec::new();
    }
    let (lang_terr, modifier) = match without_charset.split_once('@') {
        Some((head, modifier)) => (head.to_string(), Some(modifier.to_string())),
        None => (without_charset.clone(), None),
    };
    let lang = lang_terr.split('_').next().unwrap_or("").to_string();
    let mut chain = vec![without_charset.clone()];
    if let Some(modifier) = &modifier {
        chain.push(format!("{lang}@{modifier}"));
    }
    chain.push(lang_terr);
    chain.push(lang);
    let mut seen = HashSet::new();
    chain.retain(|c| !c.is_empty() && seen.insert(c.clone()));
    chain
}

fn add_locale_value(set: &mut HashSet<String>, value: &str) {
    // LANGUAGE is a colon separated list; LANG and LC_* hold a single value.
    for part in value.split(':') {
        set.extend(fallback_chain(part));
    }
}

// KEY=VALUE files such as /etc/default/locale and plasma-localerc.
fn add_from_file(set: &mut HashSet<String>, path: &Path) {
    let Ok(text) = fs::read_to_string(path) else { return };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key == "LANG" || key == "LANGUAGE" || key.starts_with("LC_") {
            add_locale_value(set, value);
        }
    }
}

// The user who asked for the cleanup: under pkexec that is PKEXEC_UID, whose
// home and account are looked up in /etc/passwd.
fn requesting_user() -> Option<(String, PathBuf)> {
    let uid = std::env::var("PKEXEC_UID").ok();
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 6 {
            continue;
        }
        let matches = match &uid {
            Some(uid) => fields[2] == uid,
            None => std::env::var("USER").map(|u| u == fields[0]).unwrap_or(false),
        };
        if matches {
            return Some((fields[0].to_string(), PathBuf::from(fields[5])));
        }
    }
    None
}

/// Codes that must never be removed: the active language of the system and of
/// the user (every place a desktop keeps it), and English.
pub fn protected_codes() -> HashSet<String> {
    let mut set = HashSet::new();
    for (key, value) in std::env::vars() {
        if key == "LANG" || key == "LANGUAGE" || key.starts_with("LC_") {
            add_locale_value(&mut set, &value);
        }
    }
    add_from_file(&mut set, Path::new("/etc/default/locale"));
    add_from_file(&mut set, Path::new("/etc/locale.conf"));
    if let Some((user, home)) = requesting_user() {
        add_from_file(&mut set, &home.join(".config/plasma-localerc"));
        add_from_file(&mut set, &home.join(".config/locale.conf"));
        // GNOME keeps the account language here.
        if let Ok(text) = fs::read_to_string(format!("/var/lib/AccountsService/users/{user}")) {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("Language=") {
                    add_locale_value(&mut set, value);
                }
            }
        }
    }
    set
}

fn is_english(code: &str) -> bool {
    base_of(code).as_deref() == Some("en") && !code.contains('@')
}

// ---- Groups ----------------------------------------------------------------

struct Group {
    base: String,
    removable: Vec<Found>,
    kept: Vec<Found>,
    english: bool,
}

fn groups() -> Vec<Group> {
    let protected = protected_codes();
    let mut by_base: HashMap<String, (Vec<Found>, Vec<Found>)> = HashMap::new();
    for item in found_paths() {
        let Some(base) = base_of(&item.code) else { continue };
        let keep = protected.contains(&item.code) || is_english(&item.code);
        let entry = by_base.entry(base).or_default();
        if keep {
            entry.1.push(item);
        } else {
            entry.0.push(item);
        }
    }
    by_base
        .into_iter()
        .map(|(base, (removable, kept))| Group { english: base == "en", base, removable, kept })
        .collect()
}

fn scan_blocking() -> Vec<LanguageEntry> {
    let groups = groups();
    let all_paths: Vec<&Path> = groups.iter().flat_map(|g| g.removable.iter().chain(g.kept.iter())).map(|f| f.path.as_path()).collect();
    let sizes = sizes(&all_paths);
    let size_of = |items: &[Found]| items.iter().map(|f| sizes.get(&f.path).copied().unwrap_or(0)).sum::<u64>();
    let unique = |items: &[Found], f: &dyn Fn(&Found) -> String| -> Vec<String> {
        let mut values: Vec<String> = items.iter().map(f).collect();
        values.sort();
        values.dedup();
        values
    };

    let mut entries: Vec<LanguageEntry> = groups
        .iter()
        .map(|g| {
            // Variants that are only empty folders (what localepurge leaves
            // behind) free nothing, so they do not count as removable: the
            // language is shown as protected instead of disappearing.
            let nothing_to_remove = g.removable.is_empty() || (size_of(&g.removable) == 0 && !g.kept.is_empty());
            let shown = if nothing_to_remove { &g.kept } else { &g.removable };
            LanguageEntry {
                base: g.base.clone(),
                remove_codes: unique(&g.removable, &|f| f.code.clone()),
                keep_codes: unique(&g.kept, &|f| f.code.clone()),
                places: unique(shown, &|f| f.place.to_string()),
                size_bytes: size_of(shown),
                protected_by: if nothing_to_remove { Some(if g.english { "english" } else { "active" }.to_string()) } else { None },
            }
        })
        .collect();
    // Variants that are only empty folders free nothing and are not listed.
    entries.retain(|e| e.protected_by.is_some() || e.size_bytes > 0);
    entries.sort_by(|a, b| a.protected_by.is_some().cmp(&b.protected_by.is_some()).then(b.size_bytes.cmp(&a.size_bytes)).then(a.base.cmp(&b.base)));
    entries
}

#[tauri::command]
pub async fn scan_languages() -> Vec<LanguageEntry> {
    tauri::async_runtime::spawn_blocking(scan_blocking).await.unwrap_or_default()
}

/// Privileged-side check: every requested language must exist and have
/// something removable, i.e. it is not entirely protected.
pub fn validate_languages(bases: &[String]) -> Result<(), String> {
    if bases.is_empty() {
        return Err("No languages given".to_string());
    }
    let groups = groups();
    for base in bases {
        if !(2..=3).contains(&base.len()) || !base.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(format!("Not a language code: {base}"));
        }
        match groups.iter().find(|g| &g.base == base) {
            Some(group) if !group.removable.is_empty() => {}
            Some(_) => return Err(format!("This language is protected: {base}")),
            None => return Err(format!("Unknown language: {base}")),
        }
    }
    Ok(())
}

/// Deletes only the paths the privileged side itself found for the requested
/// languages, never anything the request named.
pub fn remove_languages(bases: &[String]) -> bool {
    let mut all_ok = true;
    for group in groups().into_iter().filter(|g| bases.contains(&g.base)) {
        for item in group.removable {
            // A symlink or file in a language directory slot is left alone.
            let is_dir = fs::symlink_metadata(&item.path).map(|m| m.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            if let Err(e) = fs::remove_dir_all(&item.path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("soplos-sys-cleaner: could not delete {}: {e}", item.path.display());
                    all_ok = false;
                }
            }
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
        let mut protected: Vec<String> = protected_codes().into_iter().collect();
        protected.sort();
        println!("protected codes: {protected:?}");
        for e in scan_blocking() {
            println!("{} remove={:?} keep={:?} places={:?} size={} protected_by={:?}", e.base, e.remove_codes, e.keep_codes, e.places, e.size_bytes, e.protected_by);
        }
    }

    #[test]
    fn language_codes_are_recognised() {
        assert_eq!(base_of("es_ES").as_deref(), Some("es"));
        assert_eq!(base_of("ca_ES@valencia").as_deref(), Some("ca"));
        assert_eq!(base_of("sr@ijekavianlatin").as_deref(), Some("sr"));
        assert_eq!(base_of("pt_BR").as_deref(), Some("pt"));
        assert_eq!(base_of("man1"), None);
        assert_eq!(base_of("cat1"), None);
        assert_eq!(base_of("C"), None);
        assert!(is_language_code("zh_CN"));
        assert!(!is_language_code("man5"));
        assert!(!is_language_code("../etc"));
    }

    #[test]
    fn fallback_chain_covers_what_gettext_tries() {
        assert_eq!(fallback_chain("es_ES.UTF-8"), vec!["es_ES", "es"]);
        assert!(fallback_chain("ca_ES.UTF-8@valencia").contains(&"ca@valencia".to_string()));
        assert!(fallback_chain("C.UTF-8").is_empty());
        assert!(fallback_chain("POSIX").is_empty());
        assert!(fallback_chain("").is_empty());
    }

    #[test]
    fn english_is_protected_but_its_quote_variants_are_not() {
        assert!(is_english("en"));
        assert!(is_english("en_GB"));
        assert!(!is_english("en@quot"));
        assert!(!is_english("es"));
    }

    #[test]
    fn requests_with_bad_codes_are_refused() {
        assert!(validate_languages(&[]).is_err());
        assert!(validate_languages(&["../etc".to_string()]).is_err());
        assert!(validate_languages(&["ES".to_string()]).is_err());
    }
}
