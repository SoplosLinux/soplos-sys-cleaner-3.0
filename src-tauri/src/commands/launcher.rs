// How the two programs of the package relate. The advanced program is
// `soplos-sys-cleaner` and the basic one is `soplos-sys-cleaner-basic`; they
// live in the same directory and share one application identifier, so only
// one of them can be running at a time.
//
// - The basic program can open the advanced one (and closes itself); the
//   advanced one never opens the basic one.
// - Privileged work always runs through the advanced binary, because the
//   polkit action is tied to its path.
// - The package installs a launcher for the basic program only. The basic
//   program can add or remove a user launcher for the advanced one.

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const ADVANCED_BINARY: &str = "soplos-sys-cleaner";
pub const BASIC_BINARY: &str = "soplos-sys-cleaner-basic";

const SHORTCUT_FILE: &str = "soplos-sys-cleaner-advanced.desktop";

// The advanced binary next to the one that is running.
pub fn advanced_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(ADVANCED_BINARY);
    candidate.is_file().then_some(candidate)
}

/// The binary the privileged helper is started from: always the advanced one.
/// None when the basic program cannot find it, in which case nothing is
/// elevated (the basic binary must never be started as root).
pub fn elevation_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.file_name().map(|n| n == BASIC_BINARY).unwrap_or(false) {
        advanced_binary_path()
    } else {
        Some(exe)
    }
}

/// Starts the advanced program and closes this one. The new process is told
/// to wait until this one has exited, because both share one single-instance
/// identity and the second to start would otherwise just hand over to the first.
#[tauri::command]
pub fn open_advanced(app: tauri::AppHandle) -> Result<(), String> {
    let advanced = advanced_binary_path().ok_or_else(|| "The advanced version was not found".to_string())?;
    Command::new(advanced)
        .arg("--wait-for-pid")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so closing this program does not take it along.
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// Called first thing by both programs: when started by the other one, wait
/// for it to exit (up to 20 seconds) before opening a window.
pub fn wait_for_parent(args: &[String]) {
    let Some(pid) = args.iter().position(|a| a == "--wait-for-pid").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u32>().ok()) else {
        return;
    };
    let proc_dir = format!("/proc/{pid}");
    for _ in 0..200 {
        if !std::path::Path::new(&proc_dir).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ---- Optional launcher for the advanced program ------------------------------

fn shortcut_path() -> Option<PathBuf> {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").ok()?).join(".local/share"),
    };
    Some(base.join("applications").join(SHORTCUT_FILE))
}

// TryExec makes desktops hide the entry on their own if the program is
// uninstalled, so the user file never turns into a broken icon.
const SHORTCUT: &str = "[Desktop Entry]
Type=Application
Version=1.0
Name=Soplos Sys Cleaner (Advanced)
Name[es]=Soplos Sys Cleaner (avanzado)
Name[de]=Soplos Sys Cleaner (Erweitert)
Name[fr]=Soplos Sys Cleaner (avancé)
Name[it]=Soplos Sys Cleaner (avanzato)
Name[pt]=Soplos Sys Cleaner (avançado)
Name[ro]=Soplos Sys Cleaner (avansat)
Name[ru]=Soplos Sys Cleaner (расширенный)
GenericName=System Cleaner
Comment=Advanced version of Soplos Sys Cleaner
Exec=soplos-sys-cleaner
TryExec=soplos-sys-cleaner
Icon=soplos-sys-cleaner
Terminal=false
Categories=System;Settings;
StartupNotify=true
StartupWMClass=soplos-sys-cleaner
";

#[tauri::command]
pub fn advanced_shortcut_exists() -> bool {
    shortcut_path().map(|p| p.is_file()).unwrap_or(false)
}

#[tauri::command]
pub fn set_advanced_shortcut(enabled: bool) -> Result<(), String> {
    let path = shortcut_path().ok_or_else(|| "Cannot find the applications directory".to_string())?;
    if enabled {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&path, SHORTCUT).map_err(|e| e.to_string())
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_is_a_valid_desktop_entry() {
        assert!(SHORTCUT.starts_with("[Desktop Entry]\n"));
        assert!(SHORTCUT.contains("\nExec=soplos-sys-cleaner\n"));
        assert!(SHORTCUT.contains("\nTryExec=soplos-sys-cleaner\n"));
        // It must launch the advanced binary, never the basic one.
        assert!(!SHORTCUT.contains(BASIC_BINARY));
    }

    #[test]
    fn waiting_for_a_missing_pid_returns_at_once() {
        // A pid that cannot exist: the wait ends on the first check.
        let started = std::time::Instant::now();
        wait_for_parent(&["x".into(), "--wait-for-pid".into(), "4294967290".into()]);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // No flag, no wait.
        wait_for_parent(&["x".into()]);
    }
}
