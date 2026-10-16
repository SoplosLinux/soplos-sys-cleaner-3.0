// Hardware detection and the tables that decide what is needed, ported from
// 2.x's scanner/hardware.py. Detection uses layers that complement each other:
//   0. dracut configuration: firmware the system itself declares it needs
//   1. lspci: every PCI device (and the GPU/network-class subsets)
//   2. lsusb: every USB device
//   3. loaded modules bound to a real device, plus firmware requested in the
//      kernel log
//   4. virtualisation: systemd-detect-virt, with DMI fallbacks
// Whatever no layer claims is a removal candidate. The lessons 2.x paid for
// are kept: a loaded module is not proof of hardware (it must be bound to a
// device), Intel needs a GPU- or network-class device rather than any 8086
// device, and virt-viewer style clients are never flagged.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::packages::{command_exists, installed_version, looks_like_kernel_version};

type Table = &'static [(&'static str, &'static [&'static str])];

pub const PROTECTED_FIRMWARE_ALWAYS: &[&str] = &["regulatory.db"];

const REALTEK_FIRMWARE: &[&str] = &["rtlwifi", "rtw88", "rtw89", "rtl_bt", "rtl8761b", "rtl8821c", "rtl8822c", "rtl8852a", "rtl8852b", "rtl8852c"];

pub const PCI_VENDOR_FIRMWARE: Table = &[
    ("1002", &["amdgpu", "radeon", "amd"]),
    ("8086", &["i915", "intel", "iwlwifi"]),
    ("10de", &["nvidia"]),
    ("10ec", REALTEK_FIRMWARE),
    ("168c", &["ath10k", "ath11k", "ath12k", "qca"]),
    ("14e4", &["brcm"]),
    ("14c3", &["mediatek"]),
    ("11ab", &["libertas", "mwl8k", "mwifiex"]),
    ("1b4b", &["libertas"]),
    ("15ad", &["vmwgfx"]),
    ("80ee", &[]),
    ("1af4", &[]),
];

pub const USB_VENDOR_FIRMWARE: Table = &[
    ("8087", &["intel", "iwlwifi"]),
    ("0bda", REALTEK_FIRMWARE),
    ("0cf3", &["qca", "ath3k"]),
    ("0a5c", &["brcm"]),
    ("0489", &["brcm"]),
    ("0e8d", &["mediatek"]),
    ("148f", &["rt2870", "rt73usb", "rt2500usb"]),
    ("1286", &["mwifiex"]),
    ("04ca", &["qca"]),
    ("0a12", &[]),
];

pub const PCI_VENDOR_PACKAGES: Table = &[
    (
        "10de",
        &[
            "nvidia-driver", "nvidia-driver-bin", "nvidia-driver-libs", "nvidia-kernel-dkms", "nvidia-kernel-support", "nvidia-alternative",
            "nvidia-egl-icd", "libglx-nvidia0", "libgl1-nvidia-glx", "libnvidia-glcore", "libnvidia-encode1", "libnvidia-cfg1", "libnvidia-ml1",
            "libnvidia-fbc1", "nvidia-vdpau-driver", "xserver-xorg-video-nvidia", "nvidia-smi", "nvidia-settings", "nvidia-modprobe",
            "nvidia-persistenced", "nvidia-vulkan-icd", "firmware-nvidia-graphics",
        ],
    ),
    ("1002", &["xserver-xorg-video-amdgpu", "xserver-xorg-video-ati", "xserver-xorg-video-radeon", "radeontop"]),
    ("8086", &["xserver-xorg-video-intel", "intel-media-va-driver", "intel-gpu-tools", "intel-opencl-icd"]),
    ("14e4", &["broadcom-sta-dkms", "bcmwl-kernel-source"]),
];

pub const USB_VENDOR_PACKAGES: Table = &[
    ("056a", &["xserver-xorg-input-wacom", "libwacom2", "libwacom-bin"]),
    ("03f0", &["hplip", "hplip-data", "printer-driver-hpijs"]),
    ("04b8", &["epson-inkjet-printer-escpr", "epson-inkjet-printer-escpr2"]),
    ("04a9", &["cnijfilter2", "scangearmp2"]),
    ("04f9", &["printer-driver-brlaser"]),
    ("04e8", &["printer-driver-splix"]),
    ("0482", &["printer-driver-c2esp"]),
];

pub const VM_GUEST_PACKAGES: Table = &[
    ("oracle", &["virtualbox-guest-x11", "virtualbox-guest-utils", "virtualbox-guest-dkms"]),
    ("vmware", &["xserver-xorg-video-vmware", "open-vm-tools", "open-vm-tools-desktop"]),
    ("kvm", &["spice-vdagent", "spice-webdavd"]),
    ("qemu", &["spice-vdagent", "spice-webdavd"]),
    ("microsoft", &["hyperv-daemons"]),
    ("xen", &["xen-utils-guest", "xe-guest-utilities"]),
];

// Host-only packages: useless unless this machine is itself a KVM/QEMU host.
// virt-manager, virt-viewer and libvirt-clients are deliberately absent: they
// are remote clients that work against any libvirt/SPICE host.
pub const KVM_PACKAGES: &[&str] = &[
    "qemu-system-x86", "qemu-system-common", "qemu-utils", "qemu-block-extra", "libvirt-daemon", "libvirt-daemon-system", "bridge-utils",
    "dnsmasq-base", "ovmf", "cpu-checker", "libguestfs-tools",
];
const KVM_FIRMWARE: &[&str] = &["qemu"];

// Package -> kernel modules it ships; used to clean the references left in
// /etc/modules and /etc/modules-load.d after a purge.
pub const PKG_MODULES: Table = &[
    ("virtualbox-guest-dkms", &["vboxguest", "vboxsf", "vboxvideo"]),
    ("virtualbox-guest-utils", &["vboxguest", "vboxsf"]),
    ("virtualbox-guest-x11", &["vboxvideo"]),
    ("open-vm-tools", &["vmw_vmci", "vmwgfx", "vsock", "vmw_balloon", "vmxnet3", "pvscsi"]),
    ("open-vm-tools-desktop", &["vmwgfx"]),
    ("xserver-xorg-video-vmware", &["vmwgfx"]),
    ("nvidia-kernel-dkms", &["nvidia", "nvidia_drm", "nvidia_modeset", "nvidia_uvm"]),
    ("nvidia-driver", &["nvidia", "nvidia_drm", "nvidia_modeset", "nvidia_uvm"]),
    ("broadcom-sta-dkms", &["wl"]),
    ("bcmwl-kernel-source", &["wl"]),
    ("hyperv-daemons", &["hv_vmbus", "hv_storvsc", "hv_netvsc", "hv_utils", "hv_balloon"]),
];

// Dedicated firmware-only packages, by firmware family. The generic bundles
// (firmware-linux-nonfree, firmware-misc-nonfree) are never purged: they hold
// firmware for many unrelated devices.
pub const FIRMWARE_PACKAGES: &[(&str, &str)] = &[
    ("amdgpu", "firmware-amd-graphics"),
    ("radeon", "firmware-amd-graphics"),
    ("i915", "firmware-intel-graphics"),
    ("iwlwifi", "firmware-iwlwifi"),
    ("intel", "firmware-intel-sound"),
    ("rtlwifi", "firmware-realtek"),
    ("rtw88", "firmware-realtek"),
    ("rtw89", "firmware-realtek"),
    ("rtl_bt", "firmware-realtek"),
    ("rtl8761b", "firmware-realtek"),
    ("rtl8821c", "firmware-realtek"),
    ("rtl8822c", "firmware-realtek"),
    ("rtl8852a", "firmware-realtek"),
    ("rtl8852b", "firmware-realtek"),
    ("rtl8852c", "firmware-realtek"),
    ("rtl_nic", "firmware-realtek"),
    ("brcm", "firmware-brcm80211"),
    ("ath10k", "firmware-atheros"),
    ("ath11k", "firmware-atheros"),
    ("ath12k", "firmware-atheros"),
    ("qca", "firmware-atheros"),
    ("mediatek", "firmware-mediatek"),
    ("mt76", "firmware-mediatek"),
    ("libertas", "firmware-libertas"),
    ("mwl8k", "firmware-libertas"),
    ("mwifiex", "firmware-libertas"),
    ("nvidia", "firmware-nvidia-graphics"),
];

pub const FIRMWARE_DIR: &str = "/lib/firmware";

pub fn lookup(table: Table, key: &str) -> &'static [&'static str] {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or(&[])
}

fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub struct Hardware {
    // False when lspci, lsusb or modinfo is missing, or lspci found nothing:
    // the result of every layer is then untrustworthy, so firmware and
    // drivers are treated as protected instead of as unused.
    pub detection_reliable: bool,
    pub pci_vendors: HashSet<String>,
    pub gpu_pci_vendors: HashSet<String>,
    pub network_pci_vendors: HashSet<String>,
    pub usb_vendors: HashSet<String>,
    pub kvm_present: bool,
    pub vm_guest: String,
    pub dracut_firmware_dirs: HashSet<String>,
    pub active_firmware_files: HashSet<String>,
}

/// Everything except the firmware layers, which take longer and only the
/// firmware module needs.
pub fn detect_basic() -> Hardware {
    let pci = pci_vendors();
    Hardware {
        detection_reliable: detection_reliable(&pci),
        pci_vendors: pci,
        gpu_pci_vendors: pci_vendors_by_class("03"),
        network_pci_vendors: pci_vendors_by_class("02"),
        usb_vendors: usb_vendors(),
        kvm_present: kvm_present(),
        vm_guest: vm_guest(),
        dracut_firmware_dirs: HashSet::new(),
        active_firmware_files: HashSet::new(),
    }
}

pub fn detect() -> Hardware {
    let pci = pci_vendors();
    Hardware {
        detection_reliable: detection_reliable(&pci),
        pci_vendors: pci,
        gpu_pci_vendors: pci_vendors_by_class("03"),
        network_pci_vendors: pci_vendors_by_class("02"),
        usb_vendors: usb_vendors(),
        kvm_present: kvm_present(),
        vm_guest: vm_guest(),
        dracut_firmware_dirs: dracut_firmware_dirs(),
        active_firmware_files: active_firmware_files(),
    }
}

// Every PCI machine has at least a host bridge, so an empty lspci result
// means the tool did not work, not that there is no hardware.
fn detection_reliable(pci: &HashSet<String>) -> bool {
    !pci.is_empty() && command_exists("lspci") && command_exists("lsusb") && command_exists("modinfo")
}

// "[10de:1c82]" inside an `lspci -nn` line.
fn vendor_in_brackets(line: &str) -> Option<String> {
    for (start, _) in line.match_indices('[') {
        let rest = &line[start + 1..];
        let bytes = rest.as_bytes();
        // "vvvv:dddd]": bytes 4 and 9 are ASCII, so the slices below are on
        // character boundaries.
        if bytes.len() >= 10 && bytes[4] == b':' && bytes[9] == b']' {
            let vendor = &rest[..4];
            let device = &rest[5..9];
            if vendor.chars().all(|c| c.is_ascii_hexdigit()) && device.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(vendor.to_lowercase());
            }
        }
    }
    None
}

fn pci_vendors() -> HashSet<String> {
    output_of("lspci", &["-nn"]).map(|text| text.lines().filter_map(vendor_in_brackets).collect()).unwrap_or_default()
}

// `lspci -n` prints "00:02.0 0300: 8086:1234": the class lets a real GPU or
// network card be told apart from the Intel chipset devices that every VM emulates.
fn pci_vendors_by_class(class_prefix: &str) -> HashSet<String> {
    let Some(text) = output_of("lspci", &["-n"]) else { return HashSet::new() };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let _slot = parts.next()?;
            let class = parts.next()?;
            let ids = parts.next()?;
            if class.starts_with(class_prefix) && class.ends_with(':') {
                Some(ids.split(':').next()?.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

fn usb_vendors() -> HashSet<String> {
    let Some(text) = output_of("lsusb", &[]) else { return HashSet::new() };
    text.lines()
        .filter_map(|line| {
            let after = line.split(" ID ").nth(1)?;
            let vendor = after.split(':').next()?;
            if vendor.len() == 4 && vendor.chars().all(|c| c.is_ascii_hexdigit()) {
                Some(vendor.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

fn kvm_present() -> bool {
    if Path::new("/dev/kvm").exists() {
        return true;
    }
    let modules = fs::read_to_string("/proc/modules").unwrap_or_default();
    modules.lines().filter_map(|l| l.split_whitespace().next()).any(|m| m == "kvm" || m == "kvm_intel" || m == "kvm_amd")
}

fn vm_guest() -> String {
    if let Some(text) = output_of("systemd-detect-virt", &[]) {
        let virt = text.trim().to_lowercase();
        if !virt.is_empty() {
            return virt;
        }
    }
    if let Ok(kind) = fs::read_to_string("/sys/hypervisor/type") {
        let kind = kind.trim().to_lowercase();
        if !kind.is_empty() {
            return kind;
        }
    }
    for file in ["/sys/class/dmi/id/product_name", "/sys/class/dmi/id/sys_vendor"] {
        let value = fs::read_to_string(file).unwrap_or_default().to_lowercase();
        for (needle, guest) in [("vmware", "vmware"), ("virtualbox", "oracle"), ("qemu", "kvm"), ("kvm", "kvm"), ("microsoft", "microsoft"), ("hyper-v", "microsoft"), ("xen", "xen"), ("parallels", "parallels")] {
            if value.contains(needle) {
                return guest.to_string();
            }
        }
    }
    "none".to_string()
}

fn first_component_under_firmware(path: &str) -> Option<String> {
    path.split('/').filter(|p| !p.is_empty() && *p != "lib" && *p != "firmware" && *p != "usr").next().map(|s| s.to_string())
}

// Layer 0: what the system's own dracut configuration puts in the initramfs.
fn dracut_firmware_dirs() -> HashSet<String> {
    let mut dirs = HashSet::new();
    let Ok(entries) = fs::read_dir("/etc/dracut.conf.d") else { return dirs };
    let mut modules = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let is_items = line.starts_with("install_items");
            let is_drivers = line.starts_with("add_drivers");
            if !is_items && !is_drivers {
                continue;
            }
            let Some(start) = line.find(['"', '\'']) else { continue };
            let Some(end) = line.rfind(['"', '\'']) else { continue };
            if end <= start {
                continue;
            }
            for token in line[start + 1..end].split_whitespace() {
                if is_items {
                    let token = token.trim_end_matches('/');
                    if let Some(rest) = token.strip_prefix("/lib/firmware/") {
                        if !rest.contains('/') && !rest.is_empty() {
                            dirs.insert(rest.to_string());
                        }
                    }
                } else {
                    modules.push(token.to_string());
                }
            }
        }
    }
    for module in modules {
        for file in modinfo_firmware(&module) {
            if let Some(top) = first_component_under_firmware(&file) {
                dirs.insert(top);
            }
        }
    }
    dirs
}

fn modinfo_firmware(module: &str) -> Vec<String> {
    output_of("modinfo", &["-F", "firmware", module]).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

// A module is bound to hardware when its drivers/ directory holds a bus
// address (PCI "0000:00:02.0", USB "1-2:1.0"). "Loaded" is not "in use": a
// module auto-probed with no matching device still lists the firmware of
// every chip it supports.
fn module_bound_to_device(module: &str) -> bool {
    let drivers = format!("/sys/module/{module}/drivers");
    let Ok(drivers) = fs::read_dir(drivers) else { return false };
    for driver in drivers.flatten() {
        if let Ok(entries) = fs::read_dir(driver.path()) {
            if entries.flatten().any(|e| e.file_name().to_string_lossy().contains(':')) {
                return true;
            }
        }
    }
    false
}

fn is_firmware_log_file(token: &str) -> bool {
    [".bin", ".fw", ".ucode", ".cis", ".dsp", ".elf", ".nvm", ".txt"].iter().any(|ext| token.to_lowercase().ends_with(ext))
}

// Firmware paths mentioned in kernel log lines such as
// "Direct firmware load for rtl_bt/rtl8761bu_fw.bin failed".
fn firmware_in_log(text: &str, out: &mut HashSet<String>) {
    for line in text.lines() {
        for marker in ["Direct firmware load for ", "loaded firmware image ", "loaded firmware ", "firmware: "] {
            if let Some(index) = line.find(marker) {
                let rest = &line[index + marker.len()..];
                let token = rest.split(|c: char| c.is_whitespace() || c == ':' || c == ',').next().unwrap_or("").trim_start_matches("/lib/firmware/");
                if !token.is_empty() && is_firmware_log_file(token) {
                    out.insert(token.to_string());
                }
                break;
            }
        }
    }
}

// Layer 3: firmware the running kernel really uses.
fn active_firmware_files() -> HashSet<String> {
    let mut files = HashSet::new();
    let modules = fs::read_to_string("/proc/modules").unwrap_or_default();
    for module in modules.lines().filter_map(|l| l.split_whitespace().next()) {
        if module_bound_to_device(module) {
            files.extend(modinfo_firmware(module));
        }
    }
    // The journal covers earlier boots too; dmesg is the fallback.
    match output_of("journalctl", &["-k", "--no-pager", "-o", "short-monotonic"]) {
        Some(text) if !text.trim().is_empty() => firmware_in_log(&text, &mut files),
        _ => {
            if let Some(text) = output_of("dmesg", &[]) {
                firmware_in_log(&text, &mut files);
            }
        }
    }
    files
}

/// Firmware directory names under /lib/firmware that this machine needs.
pub fn firmware_protection_set(hw: &Hardware) -> HashSet<String> {
    let mut protected: HashSet<String> = PROTECTED_FIRMWARE_ALWAYS.iter().map(|s| s.to_string()).collect();
    protected.extend(hw.dracut_firmware_dirs.iter().cloned());

    for vendor in &hw.pci_vendors {
        for family in lookup(PCI_VENDOR_FIRMWARE, vendor) {
            // Intel chipset, bridge and audio devices carry vendor 8086 even
            // in a VM with no Intel GPU or WiFi, so Intel families are gated
            // on a real device of the matching class.
            if vendor == "8086" {
                let class_vendors = match *family {
                    "i915" => Some(&hw.gpu_pci_vendors),
                    "iwlwifi" | "intel" => Some(&hw.network_pci_vendors),
                    _ => None,
                };
                if let Some(class_vendors) = class_vendors {
                    if !class_vendors.contains("8086") {
                        continue;
                    }
                }
            }
            protected.insert(family.to_string());
        }
    }
    if hw.kvm_present {
        protected.extend(KVM_FIRMWARE.iter().map(|s| s.to_string()));
    }
    for vendor in &hw.usb_vendors {
        protected.extend(lookup(USB_VENDOR_FIRMWARE, vendor).iter().map(|s| s.to_string()));
    }
    for file in &hw.active_firmware_files {
        if let Some(top) = first_component_under_firmware(file) {
            if Path::new(FIRMWARE_DIR).join(&top).is_dir() {
                protected.insert(top);
            }
        }
    }
    protected
}

/// Conservative on purpose: a direct match, or the family name starting with
/// or containing a protected name, counts as protected.
pub fn is_firmware_family_protected(family: &str, protection: &HashSet<String>) -> bool {
    let lower = family.to_lowercase();
    protection.contains(family) || protection.iter().any(|p| !p.is_empty() && (lower.starts_with(&p.to_lowercase()) || lower.contains(&p.to_lowercase())))
}

pub fn vendor_label(vendor: &str) -> String {
    match vendor {
        "10de" => "NVIDIA",
        "1002" => "AMD",
        "8086" => "Intel",
        "15ad" => "VMware",
        "80ee" => "VirtualBox",
        "1af4" => "KVM/QEMU",
        "14e4" => "Broadcom",
        "056a" => "Wacom",
        "03f0" => "HP",
        "04b8" => "Epson",
        "04a9" => "Canon",
        "04f9" => "Brother",
        "04e8" => "Samsung",
        "0482" => "Kyocera",
        other => return other.to_uppercase(),
    }
    .to_string()
}

fn guest_label(guest: &str) -> String {
    match guest {
        "oracle" => "VirtualBox",
        "vmware" => "VMware",
        "kvm" | "qemu" => "KVM/QEMU",
        "microsoft" => "Hyper-V",
        "xen" => "Xen",
        other => return other.to_uppercase(),
    }
    .to_string()
}

pub struct DriverCandidate {
    pub name: String,
    pub vendor: String,
}

/// Installed packages tied to hardware this machine does not have.
pub fn unnecessary_driver_packages(hw: &Hardware, installed: &HashMap<String, u64>) -> Vec<DriverCandidate> {
    let mut found: Vec<DriverCandidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |name: &str, vendor: String, found: &mut Vec<DriverCandidate>| {
        if installed.contains_key(name) && seen.insert(name.to_string()) {
            found.push(DriverCandidate { name: name.to_string(), vendor });
        }
    };

    for (vendor, packages) in PCI_VENDOR_PACKAGES {
        // Intel GPU packages are protected only by a real Intel GPU.
        let present = if *vendor == "8086" { &hw.gpu_pci_vendors } else { &hw.pci_vendors };
        if present.contains(*vendor) {
            continue;
        }
        for package in *packages {
            add(package, vendor_label(vendor), &mut found);
        }
    }
    for (vendor, packages) in USB_VENDOR_PACKAGES {
        if hw.usb_vendors.contains(*vendor) {
            continue;
        }
        for package in *packages {
            add(package, vendor_label(vendor), &mut found);
        }
    }

    // kvm and qemu guests share the same SPICE tools, so either one protects both.
    let protected_guests: Vec<&str> = match hw.vm_guest.as_str() {
        "kvm" | "qemu" => vec!["kvm", "qemu"],
        other => vec![other],
    };
    for (guest, packages) in VM_GUEST_PACKAGES {
        if protected_guests.contains(guest) {
            continue;
        }
        for package in *packages {
            add(package, guest_label(guest), &mut found);
        }
    }
    if !hw.kvm_present {
        for package in KVM_PACKAGES {
            add(package, "KVM/QEMU".to_string(), &mut found);
        }
    }
    found
}

/// Every package the driver module is allowed to purge.
pub fn all_driver_package_names() -> HashSet<&'static str> {
    let mut names: HashSet<&'static str> = HashSet::new();
    for table in [PCI_VENDOR_PACKAGES, USB_VENDOR_PACKAGES, VM_GUEST_PACKAGES] {
        for (_, packages) in table {
            names.extend(packages.iter().copied());
        }
    }
    names.extend(KVM_PACKAGES.iter().copied());
    names
}

pub struct DkmsOrphan {
    pub module: String,
    pub version: String,
    pub kernels: Vec<String>,
    pub dir: PathBuf,
}

fn dkms_package_candidates(module: &str) -> Vec<String> {
    let known: &[&str] = match module {
        "nvidia" => &["nvidia-kernel-dkms", "nvidia-dkms", "nvidia-driver"],
        "vboxguest" => &["virtualbox-guest-dkms"],
        "vboxhost" => &["virtualbox", "virtualbox-dkms"],
        "broadcom-sta" => &["broadcom-sta-dkms"],
        "wl" => &["broadcom-sta-dkms", "bcmwl-kernel-source"],
        "v4l2loopback" => &["v4l2loopback-dkms"],
        "zfs" => &["zfs-dkms"],
        "spl" => &["spl-dkms"],
        "bbswitch" => &["bbswitch-dkms"],
        "tp-smapi" => &["tp-smapi-dkms"],
        _ => &[],
    };
    if known.is_empty() {
        vec![format!("{module}-dkms"), module.to_string(), format!("linux-{module}")]
    } else {
        known.iter().map(|s| s.to_string()).collect()
    }
}

fn dir_has_kernel_module(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_kernel_module(&path) {
                return true;
            }
        } else {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".ko") || name.ends_with(".ko.xz") || name.ends_with(".ko.zst") {
                return true;
            }
        }
    }
    false
}

/// DKMS builds whose source package is gone. A build is legitimate when its
/// /usr/src/<module>-<version> source exists, or an owning package is
/// installed (with a matching version, or on a Soplos kernel whose installer
/// renames the DKMS version to the kernel name).
pub fn orphan_dkms(installed: &HashMap<String, u64>) -> Vec<DkmsOrphan> {
    let base = Path::new("/var/lib/dkms");
    let Ok(modules) = fs::read_dir(base) else { return Vec::new() };
    let mut orphans = Vec::new();
    for module_entry in modules.flatten() {
        let module_dir = module_entry.path();
        if !module_dir.is_dir() {
            continue;
        }
        let module = module_entry.file_name().to_string_lossy().into_owned();
        let Ok(versions) = fs::read_dir(&module_dir) else { continue };
        for version_entry in versions.flatten() {
            let version_dir = version_entry.path();
            if !version_dir.is_dir() {
                continue;
            }
            let version = version_entry.file_name().to_string_lossy().into_owned();
            if Path::new(&format!("/usr/src/{module}-{version}")).is_dir() {
                continue;
            }

            let mut package_installed = false;
            let mut version_matches = false;
            for candidate in dkms_package_candidates(&module) {
                if !installed.contains_key(&candidate) {
                    continue;
                }
                package_installed = true;
                match installed_version(&candidate) {
                    Some(package_version) if package_version.contains(&version) => {
                        version_matches = true;
                        break;
                    }
                    Some(_) => {}
                    None => {
                        version_matches = true;
                        break;
                    }
                }
            }
            if version_matches {
                continue;
            }
            if package_installed && (version.starts_with("kernel-") || Path::new(&format!("/lib/modules/{version}")).is_dir()) {
                continue;
            }

            let mut kernels = Vec::new();
            if let Ok(kernel_entries) = fs::read_dir(&version_dir) {
                for kernel_entry in kernel_entries.flatten() {
                    let name = kernel_entry.file_name().to_string_lossy().into_owned();
                    if kernel_entry.path().is_dir() && looks_like_kernel_version(&name) && dir_has_kernel_module(&kernel_entry.path()) {
                        kernels.push(name);
                    }
                }
            }
            if !kernels.is_empty() {
                kernels.sort();
                orphans.push(DkmsOrphan { module: module.clone(), version, kernels, dir: version_dir });
            }
        }
    }
    orphans.sort_by(|a, b| (&a.module, &a.version).cmp(&(&b.module, &b.version)));
    orphans
}
