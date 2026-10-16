// Drivers and tools for hardware this machine does not have (GPU stacks,
// printer drivers, hypervisor guest tools, KVM host packages) and orphaned
// DKMS builds, ported from 2.x. Detection and tables live in hardware.rs.

use serde::Serialize;
use std::collections::HashSet;

use super::hardware::{all_driver_package_names, detect_basic, orphan_dkms, unnecessary_driver_packages};
use super::packages::{dir_size_bytes, installed_packages, package_summaries};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriverEntry {
    pub name: String,
    pub description: String,
    pub vendor: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DkmsEntry {
    pub module: String,
    pub version: String,
    pub kernels: Vec<String>,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriverScan {
    pub packages: Vec<DriverEntry>,
    pub dkms: Vec<DkmsEntry>,
    pub vm_guest: String,
    // False when the hardware detection tools are missing: no package is then
    // listed, because "hardware not found" would mean nothing.
    pub detection_ok: bool,
}

fn scan_blocking() -> DriverScan {
    let hw = detect_basic();
    let installed = installed_packages();
    let candidates = if hw.detection_reliable { unnecessary_driver_packages(&hw, &installed) } else { Vec::new() };

    let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
    let summaries = package_summaries(&names);
    let mut packages: Vec<DriverEntry> = candidates
        .into_iter()
        .map(|c| DriverEntry {
            description: summaries.get(&c.name).cloned().unwrap_or_default(),
            size_bytes: installed.get(&c.name).copied().unwrap_or(0) * 1024,
            vendor: c.vendor,
            name: c.name,
        })
        .collect();
    packages.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    let dkms = orphan_dkms(&installed)
        .into_iter()
        .map(|o| DkmsEntry { size_bytes: dir_size_bytes(&o.dir), module: o.module, version: o.version, kernels: o.kernels })
        .collect();

    DriverScan { packages, dkms, vm_guest: hw.vm_guest, detection_ok: hw.detection_reliable }
}

#[tauri::command]
pub async fn scan_drivers() -> DriverScan {
    tauri::async_runtime::spawn_blocking(scan_blocking)
        .await
        .unwrap_or(DriverScan { packages: Vec::new(), dkms: Vec::new(), vm_guest: String::new(), detection_ok: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prints the scan of this machine: cargo test dump_live_scan -- --ignored --nocapture
    #[test]
    #[ignore = "prints the live scan"]
    fn dump_live_scan() {
        let scan = scan_blocking();
        println!("vm guest: {}", scan.vm_guest);
        for p in &scan.packages {
            println!("driver {} [{}] {} bytes: {}", p.name, p.vendor, p.size_bytes, p.description);
        }
        for d in &scan.dkms {
            println!("dkms {}/{} kernels={:?} {} bytes", d.module, d.version, d.kernels, d.size_bytes);
        }
    }
}

/// Privileged-side check: every package must be a known driver package and
/// the hardware that would need it must still be absent right now.
pub fn validate_driver_packages(packages: &[String]) -> Result<(), String> {
    if packages.is_empty() {
        return Err("No packages given".to_string());
    }
    let known = all_driver_package_names();
    if let Some(unknown) = packages.iter().find(|p| !known.contains(p.as_str())) {
        return Err(format!("Not a driver package this app manages: {unknown}"));
    }
    let hw = detect_basic();
    if !hw.detection_reliable {
        return Err("Hardware detection is not available (pciutils, usbutils and kmod are required)".to_string());
    }
    let installed = installed_packages();
    let unneeded: HashSet<String> = unnecessary_driver_packages(&hw, &installed).into_iter().map(|c| c.name).collect();
    if let Some(needed) = packages.iter().find(|p| !unneeded.contains(p.as_str())) {
        return Err(format!("The hardware for {needed} is present or the package is not installed"));
    }
    Ok(())
}

/// Privileged-side check: each DKMS build must be a detected orphan.
pub fn validate_dkms(items: &[(String, String)]) -> Result<(), String> {
    if items.is_empty() {
        return Err("No DKMS modules given".to_string());
    }
    let orphans = orphan_dkms(&installed_packages());
    for (module, version) in items {
        if !orphans.iter().any(|o| &o.module == module && &o.version == version) {
            return Err(format!("Not an orphaned DKMS module: {module}/{version}"));
        }
    }
    Ok(())
}
