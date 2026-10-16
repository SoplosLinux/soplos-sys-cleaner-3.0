use std::fs;
use std::process::Command;

/// Detects whether the process is running inside a virtual machine.
///
/// WebKitGTK's DMA-BUF renderer can fail silently on virtual GPUs (VMware,
/// VirtualBox, QEMU/KVM, Hyper-V), leaving the window blank with no error.
/// Callers use this to decide whether to force `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
pub fn is_running_in_vm() -> bool {
    if detect_virt_via_dmi() {
        return true;
    }
    detect_virt_via_systemd()
}

/// Falls back to GTK's built-in input method when IBus is selected but cannot
/// be reached.
///
/// With `GTK_IM_MODULE=ibus` and no reachable daemon, GTK keeps every key press
/// waiting for IBus until its queue overflows and drops them, so nothing can be
/// typed in the window. Must run before GTK starts, so it is called from main().
pub fn fix_unreachable_input_method() {
    if std::env::var("GTK_IM_MODULE").map(|v| v != "ibus").unwrap_or(true) {
        return;
    }
    if ibus_reachable() {
        return;
    }
    std::env::set_var("GTK_IM_MODULE", "gtk-im-context-simple");
}

fn ibus_reachable() -> bool {
    let socket_of = |address: &str| -> Option<String> {
        let rest = address.strip_prefix("unix:")?;
        let path = rest.split(',').find_map(|part| part.strip_prefix("path="))?;
        Some(path.to_string())
    };

    if let Ok(address) = std::env::var("IBUS_ADDRESS") {
        return socket_of(&address).map(|p| std::path::Path::new(&p).exists()).unwrap_or(false);
    }

    let Some(home) = std::env::var_os("HOME") else { return false };
    let dir = std::path::Path::new(&home).join(".config/ibus/bus");
    let Ok(entries) = fs::read_dir(dir) else { return false };
    entries.flatten().any(|entry| {
        fs::read_to_string(entry.path())
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find_map(|line| line.strip_prefix("IBUS_ADDRESS=").map(str::to_string))
            })
            .and_then(|address| socket_of(&address))
            .map(|path| std::path::Path::new(&path).exists())
            .unwrap_or(false)
    })
}

fn detect_virt_via_dmi() -> bool {
    const NEEDLES: [&str; 6] = ["vmware", "virtualbox", "qemu", "kvm", "xen", "innotek"];

    for path in [
        "/sys/class/dmi/id/sys_vendor",
        "/sys/class/dmi/id/product_name",
        "/sys/class/dmi/id/bios_vendor",
    ] {
        if let Ok(content) = fs::read_to_string(path) {
            let content = content.to_lowercase();
            if NEEDLES.iter().any(|needle| content.contains(needle)) {
                return true;
            }
        }
    }
    false
}

fn detect_virt_via_systemd() -> bool {
    Command::new("systemd-detect-virt")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
