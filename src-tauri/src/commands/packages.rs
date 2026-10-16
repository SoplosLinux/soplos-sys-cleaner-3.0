// Small helpers over dpkg shared by the kernel, driver and firmware modules.
// Nothing here is a Tauri command.

use std::collections::HashMap;
use std::process::Command;

/// Installed packages with their Installed-Size in KiB, from one dpkg-query call.
pub fn installed_packages() -> HashMap<String, u64> {
    let Ok(output) = Command::new("dpkg-query").args(["-W", "-f=${Package}\t${db:Status-Abbrev}\t${Installed-Size}\n"]).output() else {
        return HashMap::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut packages = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(name), Some(status)) = (parts.next(), parts.next()) else { continue };
        // "ii " is installed; "rc " is removed with configuration left.
        if !status.starts_with("ii") {
            continue;
        }
        let size_kib = parts.next().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
        // Multi-arch packages show up as "name:arch"; the plain name is enough here.
        packages.insert(name.split(':').next().unwrap_or(name).to_string(), size_kib);
    }
    packages
}

/// One-line descriptions for the given packages.
pub fn package_summaries(names: &[String]) -> HashMap<String, String> {
    let mut summaries = HashMap::new();
    if names.is_empty() {
        return summaries;
    }
    let Ok(output) = Command::new("dpkg-query").arg("-W").arg("-f=${Package}\t${binary:Summary}\n").args(names).output() else {
        return summaries;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((name, summary)) = line.split_once('\t') {
            summaries.insert(name.split(':').next().unwrap_or(name).to_string(), summary.to_string());
        }
    }
    summaries
}

pub fn installed_version(name: &str) -> Option<String> {
    let output = Command::new("dpkg-query").args(["-W", "-f=${Version}", name]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

pub fn running_kernel() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()).unwrap_or_default()
}

/// "6.1.0-27-amd64" or "7.2.9-soplos-bore-v4": digits, a dot, digits, then
/// anything. Metapackages such as linux-image-amd64 do not match.
pub fn looks_like_kernel_version(name: &str) -> bool {
    let major = name.chars().take_while(|c| c.is_ascii_digit()).count();
    if major == 0 {
        return false;
    }
    match name[major..].strip_prefix('.') {
        Some(rest) => rest.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false),
        None => false,
    }
}

/// Size of a directory tree in bytes, via du.
pub fn dir_size_bytes(path: &std::path::Path) -> u64 {
    Command::new("du")
        .args(["-sb", "--"])
        .arg(path)
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn command_exists(name: &str) -> bool {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let extra = ["/usr/sbin", "/sbin", "/usr/bin", "/bin"];
    std::env::split_paths(&paths).chain(extra.iter().map(std::path::PathBuf::from)).any(|dir| dir.join(name).is_file())
}
