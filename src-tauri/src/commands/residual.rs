// Residual configuration: packages that were removed but kept their
// configuration files, the `rc` state of dpkg (`apt remove` leaves it, `apt
// purge` does not). Purging deletes only the configuration the package left
// in the system; nothing in a user's home is touched.
//
// The privileged side asks dpkg again and refuses any package that is not in
// the `rc` state, so a request can never purge an installed package.

use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResidualPackage {
    pub name: String,
    pub version: String,
    pub summary: String,
    // Bytes of the configuration files that are still on disk.
    pub size_bytes: u64,
    // Part of the login screen, the boot loader or the base of the system.
    // Never purged: the purge script of such a package deletes files other
    // packages use (the display-manager.service link of the login screen in
    // use, /etc/default/grub).
    pub system_critical: bool,
}

// Packages whose purge script reaches outside the package. Purging an old
// display manager that another one replaced can delete the shared
// /etc/systemd/system/display-manager.service link (the next boot has no
// login screen), and purging grub-efi-amd64 deletes /etc/default/grub even
// when another GRUB package still uses it.
const CRITICAL_NAMES: &[&str] = &["sddm", "lightdm", "gdm", "gdm3", "xdm", "kdm", "lxdm", "slim", "greetd", "plasma-login-manager", "plasmalogin", "sudo", "dbus", "udev", "efibootmgr", "os-prober"];
const CRITICAL_PREFIXES: &[&str] = &["grub", "shim", "systemd", "initramfs-tools", "dracut", "polkit", "policykit", "libpam", "network-manager", "apparmor"];

fn is_system_critical(name: &str, provides: &str) -> bool {
    CRITICAL_NAMES.contains(&name)
        || CRITICAL_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
        || provides.split(',').any(|p| p.split_whitespace().next() == Some("x-display-manager"))
}

// Package names, as the other operations accept them.
fn valid_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && name.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

// "rc  name  version  summary  provides" rows of `dpkg-query -W`.
fn parse_rc(output: &str) -> Vec<(String, String, String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '\t').collect();
            if parts.len() < 5 || parts[0].trim() != "rc" || !valid_name(parts[1]) {
                return None;
            }
            Some((parts[1].to_string(), parts[2].trim().to_string(), parts[3].trim().to_string(), parts[4].trim().to_string()))
        })
        .collect()
}

// The Conffiles field: one " /path md5sum [obsolete]" entry per line.
fn conffile_paths(field: &str) -> Vec<String> {
    field.lines().filter_map(|l| l.split_whitespace().next()).filter(|p| p.starts_with('/')).map(str::to_string).collect()
}

fn residual_packages() -> Vec<(String, String, String, String)> {
    let Ok(output) = Command::new("dpkg-query")
        .args(["-W", "-f=${db:Status-Abbrev}\t${Package}\t${Version}\t${binary:Summary}\t${Provides}\n"])
        .output()
    else {
        return Vec::new();
    };
    parse_rc(&String::from_utf8_lossy(&output.stdout))
}

fn config_size(name: &str) -> u64 {
    let Ok(output) = Command::new("dpkg-query").args(["-W", "-f=${Conffiles}", name]).output() else { return 0 };
    conffile_paths(&String::from_utf8_lossy(&output.stdout))
        .iter()
        .filter_map(|path| std::fs::symlink_metadata(path).ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// The privileged side's check: every requested package must be in the rc
/// state right now, and none may be a login screen, boot loader or base
/// system package.
pub fn validate_residual(names: &[String]) -> Result<(), String> {
    let rc = residual_packages();
    if let Some(critical) = names.iter().find(|n| rc.iter().any(|(name, _, _, provides)| name == *n && is_system_critical(name, provides))) {
        return Err(format!("Refusing to purge a login screen, boot loader or system package: {critical}"));
    }
    let known: std::collections::HashSet<&String> = rc.iter().map(|(name, ..)| name).collect();
    match names.iter().find(|n| !known.contains(*n)) {
        Some(other) => Err(format!("Not a package with only its configuration left: {other}")),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn scan_residual_configs() -> Vec<ResidualPackage> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut found: Vec<ResidualPackage> = residual_packages()
            .into_iter()
            .map(|(name, version, summary, provides)| {
                let size_bytes = config_size(&name);
                let system_critical = is_system_critical(&name, &provides);
                ResidualPackage { name, version, summary, size_bytes, system_critical }
            })
            .collect();
        found.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.name.cmp(&b.name)));
        found
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rc_rows_are_read() {
        let output = "ii \tbash\t5.2\tGNU shell\t\nrc \tbrave-browser\t1.95\tBrave web browser\t\nrc \t-bad\t1\tx\t\nun \tfoo\t1\tx\t\nrc \tkitty-doc\t0.48\tKitty docs\t\n";
        let rc = parse_rc(output);
        assert_eq!(rc.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["brave-browser", "kitty-doc"]);
    }

    #[test]
    fn login_screens_boot_loaders_and_the_base_are_never_purged() {
        assert!(is_system_critical("sddm", ""));
        assert!(is_system_critical("plasma-login-manager", ""));
        assert!(is_system_critical("some-new-greeter", "x-display-manager"));
        assert!(is_system_critical("other", "foo, x-display-manager (= 1.0)"));
        assert!(is_system_critical("grub-efi-amd64", ""));
        assert!(is_system_critical("initramfs-tools-core", ""));
        assert!(is_system_critical("policykit-1", ""));
        assert!(is_system_critical("systemd-timesyncd", ""));
        assert!(!is_system_critical("brave-browser", ""));
        assert!(!is_system_critical("kitty-doc", "kitty-terminfo"));
        assert!(!is_system_critical("inkscape", ""));
    }

    #[test]
    fn conffile_lines_give_their_paths() {
        let field = " /etc/xdg/amarok_homerc 0123456789abcdef0123456789abcdef\n /etc/old.conf deadbeef obsolete\n";
        assert_eq!(conffile_paths(field), vec!["/etc/xdg/amarok_homerc".to_string(), "/etc/old.conf".to_string()]);
        assert!(conffile_paths("").is_empty());
    }
}
