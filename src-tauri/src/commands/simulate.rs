// Dry run of a package removal with `apt-get -s`, shown before anything is
// removed. The privileged side never trusts it: this only lets the user see
// the full list (dependants included) and blocks requests that would remove
// something the system cannot lose.

use serde::Serialize;
use std::process::Command;

use super::packages::{looks_like_kernel_version, running_kernel};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Simulation {
    pub removes: Vec<String>,
    pub blocked: Option<String>,
    pub error: Option<String>,
}

// Removing any of these would break privilege escalation, packaging or boot.
const NEVER_REMOVE: &[&str] = &["pkexec", "policykit-1", "polkitd", "dpkg", "apt", "systemd", "libc6", "soplos-sys-cleaner"];

fn valid_package_name(name: &str) -> bool {
    name.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) && name.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

// The reason a package must not be removed, if any.
fn blocked_reason(package: &str, running: &str) -> Option<String> {
    if NEVER_REMOVE.contains(&package) {
        return Some(package.to_string());
    }
    if package == format!("linux-image-{running}") || package == format!("linux-modules-{running}") {
        return Some(package.to_string());
    }
    // linux-image-amd64 style metapackages: removing one stops kernel updates.
    if let Some(rest) = package.strip_prefix("linux-image-") {
        if !looks_like_kernel_version(rest) {
            return Some(package.to_string());
        }
    }
    None
}

// Parses "Purg name [version] ..." and "Remv name [version] ..." lines.
fn parse_removals(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.starts_with("Purg ") || l.starts_with("Remv "))
        .filter_map(|l| l.split_whitespace().nth(1).map(|n| n.to_string()))
        .collect()
}

fn simulate(packages: &[String], purge: bool) -> Simulation {
    if packages.is_empty() || packages.iter().any(|p| !valid_package_name(p)) {
        return Simulation { removes: Vec::new(), blocked: None, error: Some("Invalid package list".to_string()) };
    }
    let action = if purge { "purge" } else { "remove" };
    let output = match Command::new("apt-get").env("LC_ALL", "C").args(["-s", action, "--"]).args(packages).output() {
        Ok(output) => output,
        Err(e) => return Simulation { removes: Vec::new(), blocked: None, error: Some(e.to_string()) },
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.lines().chain(stdout.lines()).find(|l| l.starts_with("E:")).unwrap_or("apt-get could not simulate the removal").to_string();
        return Simulation { removes: Vec::new(), blocked: None, error: Some(message) };
    }
    let removes = parse_removals(&stdout);
    let running = running_kernel();
    let blocked = removes.iter().find_map(|p| blocked_reason(p, &running));
    Simulation { removes, blocked, error: None }
}

#[tauri::command]
pub async fn simulate_removal(packages: Vec<String>, purge: bool) -> Simulation {
    tauri::async_runtime::spawn_blocking(move || simulate(&packages, purge))
        .await
        .unwrap_or(Simulation { removes: Vec::new(), blocked: None, error: Some("The simulation did not finish".to_string()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removals_are_parsed_from_apt_output() {
        let out = "Reading package lists...\nPurg linux-image-6.1.0-9-amd64 [6.1.27-1]\nRemv libfoo [1.0] [bar]\nConf x\n";
        assert_eq!(parse_removals(out), vec!["linux-image-6.1.0-9-amd64", "libfoo"]);
    }

    #[test]
    fn protected_packages_are_blocked() {
        assert!(blocked_reason("pkexec", "6.1.0-27-amd64").is_some());
        assert!(blocked_reason("linux-image-6.1.0-27-amd64", "6.1.0-27-amd64").is_some());
        assert!(blocked_reason("linux-image-amd64", "6.1.0-27-amd64").is_some());
        assert!(blocked_reason("linux-image-6.1.0-9-amd64", "6.1.0-27-amd64").is_none());
        assert!(blocked_reason("nvidia-driver", "6.1.0-27-amd64").is_none());
    }

    #[test]
    fn package_names_are_whitelisted() {
        assert!(valid_package_name("libreoffice-writer"));
        assert!(!valid_package_name("-y"));
        assert!(!valid_package_name("a;b"));
        assert_eq!(simulate(&["-y".to_string()], true).error.as_deref(), Some("Invalid package list"));
    }
}
