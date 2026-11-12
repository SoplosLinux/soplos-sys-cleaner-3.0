// Who creates the folders of the home.
//
// Every folder that a program makes in the home folder of a user (in ~/.config, ~/.local/share,
// ~/.local/state, ~/.var/app, ~/snap, or a hidden one of the home itself) is made by a program.
// Nothing on the system keeps that link, and no list of programs can: new ones come out every
// day, in the formats of apt, Flatpak, snap, AppImage and the webapps. The only way to know is to
// see who makes the folder when it is made.
//
// `soplos-sys-cleaner-tracker` is a small service that runs as root. It asks the kernel to
// report the folders that are created (`fanotify`, which tells the process that did it, but only
// to a group that runs as root), keeps the ones made directly in the places above, and writes
// who made each of them to `/var/lib/soplos-sys-cleaner/creators.json`, which everyone can read.
// The cleaner reads that file: a folder whose creator is not installed any more is a leftover.
//
// What the service records is only the program: the path of its executable, or the id of the
// Flatpak, the name of the snap, the file of the AppImage, the script of an interpreter. It never
// reads a file inside the folders, and it never changes anything in them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where the service writes, and the cleaner reads.
pub const STORE: &str = "/var/lib/soplos-sys-cleaner/creators.json";

/// The program that made a folder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Creator {
    /// `exe` (an executable), `script` (a script that an interpreter runs), `flatpak`, `snap`
    /// or `appimage`.
    pub kind: String,
    /// The path of the executable, script or AppImage; the id of the Flatpak; the snap's name.
    pub program: String,
    /// The day it was made, `2026-11-12`.
    pub day: String,
}

/// The absolute path of each folder, and who made it.
pub type Creators = HashMap<String, Creator>;

/// Reads the file of the service. A file that is missing or damaged is an empty answer: nothing
/// is known, which the cleaner treats as "no information", never as "nobody made it".
pub fn read_store(path: &Path) -> Creators {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// The folders of a home where the creation of a folder is kept.
pub fn watched_roots(home: &Path) -> Vec<PathBuf> {
    vec![home.to_path_buf(), home.join(".config"), home.join(".local/share"), home.join(".local/state"), home.join(".var/app"), home.join("snap")]
}

// ---- Who a process is ---------------------------------------------------------------------

/// The programs of the base of the system: what the packages of priority required, important or
/// standard put in /bin, /usr/bin, /sbin and /usr/sbin (the shell, `mkdir`, `cp`, the interpreters
/// the system needs...). They are tools that other programs use, so the one that matters is the
/// program that started them. It is read from dpkg, so no program is named in the code.
pub fn base_system_programs() -> std::collections::HashSet<String> {
    let mut programs = std::collections::HashSet::new();
    let Ok(output) = std::process::Command::new("dpkg-query").args(["-W", "-f=${Package}\t${Priority}\t${Architecture}\n"]).output() else { return programs };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 3 || !matches!(fields[1], "required" | "important" | "standard") {
            continue;
        }
        for list in [format!("/var/lib/dpkg/info/{}.list", fields[0]), format!("/var/lib/dpkg/info/{}:{}.list", fields[0], fields[2])] {
            let Ok(text) = std::fs::read_to_string(&list) else { continue };
            for path in text.lines() {
                if ["/bin/", "/usr/bin/", "/sbin/", "/usr/sbin/"].iter().any(|p| path.starts_with(p)) {
                    if let Some(name) = path.rsplit('/').next().filter(|n| !n.is_empty()) {
                        programs.insert(name.to_string());
                    }
                }
            }
        }
    }
    programs
}

fn base_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// What `/proc/<pid>` says about a process.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Process {
    pub exe: String,
    pub args: Vec<String>,
    pub appimage: Option<String>,
    pub flatpak: Option<String>,
    pub snap: Option<String>,
    /// The script that the process runs, when it is one of a program and not a file that someone
    /// opened: an argument that is a file in a place where programs are installed (`/opt`, `/usr`,
    /// `/snap`) and that is not an executable.
    pub script: Option<String>,
    pub parent: u32,
}

/// The program that a process stands for, going up to the one that started it when the process
/// is only a tool of the base of the system (`mkdir`, a shell). `lookup` gives what is known about
/// a process, and `is_base` says whether an executable is one of the base of the system.
pub fn attribute(pid: u32, lookup: &dyn Fn(u32) -> Option<Process>, is_base: &dyn Fn(&str) -> bool, day: &str) -> Option<Creator> {
    let mut current = pid;
    for _ in 0..12 {
        let process = lookup(current)?;
        // The kind of package decides first: a sandboxed program is its Flatpak or its snap,
        // whatever executable it runs inside.
        if let Some(id) = &process.flatpak {
            return Some(Creator { kind: "flatpak".into(), program: id.clone(), day: day.into() });
        }
        if let Some(name) = &process.snap {
            return Some(Creator { kind: "snap".into(), program: name.clone(), day: day.into() });
        }
        if let Some(file) = &process.appimage {
            return Some(Creator { kind: "appimage".into(), program: file.clone(), day: day.into() });
        }
        // A program that an interpreter runs is its script, whatever the interpreter is.
        if let Some(script) = &process.script {
            return Some(Creator { kind: "script".into(), program: script.clone(), day: day.into() });
        }
        if !process.exe.is_empty() && !is_base(&base_name(&process.exe)) {
            return Some(Creator { kind: "exe".into(), program: process.exe.clone(), day: day.into() });
        }
        if process.parent <= 1 {
            return None;
        }
        current = process.parent;
    }
    None
}

#[cfg(target_os = "linux")]
pub mod service {
    use super::*;
    use std::collections::HashSet;
    use std::ffi::CString;
    use std::fs;
    use std::os::raw::{c_int, c_long, c_uint, c_void};
    use std::time::{Duration, Instant};

    // The numbers of <linux/fanotify.h>.
    const FAN_CLOEXEC: c_uint = 0x0000_0001;
    const FAN_CLASS_NOTIF: c_uint = 0x0000_0000;
    const FAN_REPORT_DIR_FID: c_uint = 0x0000_0400;
    const FAN_REPORT_NAME: c_uint = 0x0000_0800;
    const FAN_MARK_ADD: c_uint = 0x0000_0001;
    const FAN_MARK_FILESYSTEM: c_uint = 0x0000_0100;
    const FAN_CREATE: u64 = 0x0000_0100;
    const FAN_ONDIR: u64 = 0x4000_0000;
    const FAN_EVENT_INFO_TYPE_DFID_NAME: u8 = 2;
    const METADATA_LEN: usize = 24;

    /// Reads what `/proc/<pid>` says. A process that is gone is `None`.
    fn read_process(pid: u32) -> Option<Process> {
        let base = format!("/proc/{pid}");
        let exe = fs::read_link(format!("{base}/exe")).ok().map(|p| p.to_string_lossy().trim_end_matches(" (deleted)").to_string()).unwrap_or_default();
        let cmdline = fs::read(format!("{base}/cmdline")).unwrap_or_default();
        let args: Vec<String> = cmdline.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        let environ = fs::read(format!("{base}/environ")).unwrap_or_default();
        let var = |name: &str| -> Option<String> {
            let prefix = format!("{name}=");
            environ.split(|b| *b == 0).find_map(|entry| String::from_utf8_lossy(entry).strip_prefix(&prefix).map(str::to_string))
        };
        let cgroup = fs::read_to_string(format!("{base}/cgroup")).unwrap_or_default();
        // `snap.<name>.<app>-<uuid>.scope`
        let snap = cgroup.split('/').find_map(|part| part.strip_prefix("snap.").and_then(|rest| rest.split('.').next()).map(str::to_string));
        let stat = fs::read_to_string(format!("{base}/stat")).ok()?;
        // "pid (comm) state ppid ..." where comm may hold spaces and parentheses.
        let after = stat.rsplit_once(')')?.1;
        let parent = after.split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        let script = args.iter().skip(1).filter(|a| !a.starts_with('-')).find(|a| is_program_script(a)).cloned();
        Some(Process { exe, args, appimage: var("APPIMAGE").filter(|a| a.starts_with('/')), flatpak: var("FLATPAK_ID"), snap, script, parent })
    }

    /// Whether an argument is the script of a program: a file in a place where programs are
    /// installed and not an executable (an ELF file starts with the bytes 7F 45 4C 46).
    fn is_program_script(arg: &str) -> bool {
        if !["/opt/", "/usr/", "/snap/", "/var/lib/flatpak/"].iter().any(|p| arg.starts_with(p)) {
            return false;
        }
        let Ok(mut file) = fs::File::open(arg) else { return false };
        let mut magic = [0u8; 4];
        std::io::Read::read_exact(&mut file, &mut magic).is_ok() && magic != [0x7f, b'E', b'L', b'F']
    }

    /// The homes of the people of this computer.
    fn homes() -> Vec<PathBuf> {
        let Ok(passwd) = fs::read_to_string("/etc/passwd") else { return Vec::new() };
        passwd
            .lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.split(':').collect();
                let uid: u32 = f.get(2)?.parse().ok()?;
                (f.len() >= 6 && (1000..65534).contains(&uid) && Path::new(f[5]).is_dir()).then(|| PathBuf::from(f[5]))
            })
            .collect()
    }

    fn day_now() -> String {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
        // Days since 1970 to a calendar day (Howard Hinnant's algorithm).
        let z = secs / 86400 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
    }

    fn save(creators: &Creators) {
        let store = Path::new(STORE);
        if let Some(dir) = store.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let temp = store.with_extension("tmp");
        if let Ok(text) = serde_json::to_string(creators) {
            if fs::write(&temp, text).is_ok() {
                let _ = fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o644));
                let _ = fs::rename(&temp, store);
            }
        }
    }

    /// One report of the kernel: a folder (`name`) was made in the folder with this handle.
    struct Created {
        mask: u64,
        pid: u32,
        fsid: [i32; 2],
        handle: Vec<u8>,
        name: String,
    }

    fn le_u32(b: &[u8]) -> u32 {
        u32::from_ne_bytes([b[0], b[1], b[2], b[3]])
    }

    /// Splits what `read` returned into reports. Only the ones that carry a folder's handle and a
    /// name are kept.
    fn parse(buffer: &[u8]) -> Vec<Created> {
        let mut out = Vec::new();
        let mut at = 0usize;
        while at + METADATA_LEN <= buffer.len() {
            let event_len = le_u32(&buffer[at..]) as usize;
            let meta_len = u16::from_ne_bytes([buffer[at + 6], buffer[at + 7]]) as usize;
            if event_len < METADATA_LEN || at + event_len > buffer.len() {
                break;
            }
            let mask = u64::from_ne_bytes(buffer[at + 8..at + 16].try_into().unwrap());
            let pid = i32::from_ne_bytes(buffer[at + 20..at + 24].try_into().unwrap()).max(0) as u32;
            let mut info = at + meta_len;
            while info + 4 <= at + event_len {
                let kind = buffer[info];
                let len = u16::from_ne_bytes([buffer[info + 2], buffer[info + 3]]) as usize;
                if len < 4 || info + len > at + event_len {
                    break;
                }
                if kind == FAN_EVENT_INFO_TYPE_DFID_NAME && len >= 20 {
                    let fsid = [i32::from_ne_bytes(buffer[info + 4..info + 8].try_into().unwrap()), i32::from_ne_bytes(buffer[info + 8..info + 12].try_into().unwrap())];
                    let handle_bytes = le_u32(&buffer[info + 12..]) as usize;
                    let handle_end = info + 12 + 8 + handle_bytes;
                    if handle_end <= info + len {
                        let handle = buffer[info + 12..handle_end].to_vec();
                        let name_bytes = &buffer[handle_end..info + len];
                        let name = String::from_utf8_lossy(name_bytes.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
                        out.push(Created { mask, pid, fsid, handle, name });
                    }
                }
                info += len;
            }
            at += event_len;
        }
        out
    }

    extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
    }

    // x86_64 and aarch64 share the numbers of the generic table for these calls.
    #[cfg(target_arch = "x86_64")]
    const SYS_FANOTIFY_INIT: c_long = 300;
    #[cfg(target_arch = "x86_64")]
    const SYS_FANOTIFY_MARK: c_long = 301;
    #[cfg(target_arch = "x86_64")]
    const SYS_OPEN_BY_HANDLE_AT: c_long = 304;
    #[cfg(target_arch = "aarch64")]
    const SYS_FANOTIFY_INIT: c_long = 262;
    #[cfg(target_arch = "aarch64")]
    const SYS_FANOTIFY_MARK: c_long = 263;
    #[cfg(target_arch = "aarch64")]
    const SYS_OPEN_BY_HANDLE_AT: c_long = 265;

    const O_RDONLY: c_int = 0;
    const O_PATH: c_int = 0o10_000_000;
    const O_CLOEXEC: c_int = 0o2_000_000;
    const AT_FDCWD: c_int = -100;

    /// The path of the folder that a handle of the kernel stands for.
    fn path_of(mount_fd: c_int, handle: &[u8]) -> Option<PathBuf> {
        // `struct file_handle { u32 handle_bytes; i32 handle_type; u8 f_handle[]; }`
        let fd = unsafe { syscall(SYS_OPEN_BY_HANDLE_AT, mount_fd, handle.as_ptr(), O_PATH | O_CLOEXEC) } as c_int;
        if fd < 0 {
            return None;
        }
        let path = fs::read_link(format!("/proc/self/fd/{fd}")).ok();
        unsafe { close(fd) };
        path
    }

    /// Runs until the service is stopped. Only returns when it cannot start.
    pub fn run() -> Result<(), String> {
        // The folder where it was made (its handle) and the name of the new one.
        let flags = FAN_CLOEXEC | FAN_CLASS_NOTIF | FAN_REPORT_DIR_FID | FAN_REPORT_NAME;
        let group = unsafe { syscall(SYS_FANOTIFY_INIT, flags, O_RDONLY | O_CLOEXEC) } as c_int;
        if group < 0 {
            return Err(format!("fanotify_init failed: {}", std::io::Error::last_os_error()));
        }
        // One mark for each filesystem that holds a home, and one open folder of it to resolve
        // the handles that the kernel sends.
        let mut mounts: Vec<([i32; 2], c_int)> = Vec::new();
        let mut roots: HashSet<PathBuf> = HashSet::new();
        for home in homes() {
            for root in watched_roots(&home) {
                roots.insert(root);
            }
            let Ok(c_home) = CString::new(home.to_string_lossy().as_bytes()) else { continue };
            let marked = unsafe { syscall(SYS_FANOTIFY_MARK, group, FAN_MARK_ADD | FAN_MARK_FILESYSTEM, FAN_CREATE | FAN_ONDIR, AT_FDCWD, c_home.as_ptr()) };
            if marked < 0 {
                eprintln!("could not watch {}: {}", home.display(), std::io::Error::last_os_error());
                continue;
            }
            // An open folder of the filesystem, which the kernel needs to resolve its handles.
            let fd = match fs::File::open(&home) {
                Ok(file) => std::os::fd::IntoRawFd::into_raw_fd(file),
                Err(_) => continue,
            };
            if let Some(fsid) = fs_id(&home) {
                mounts.push((fsid, fd));
            }
        }
        if mounts.is_empty() {
            return Err("no home folder to watch".into());
        }

        let base = base_system_programs();
        let is_base = move |name: &str| base.contains(name);
        let mut creators = read_store(Path::new(STORE));
        let mut dirty = false;
        let mut last_save = Instant::now();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let got = unsafe { read(group, buffer.as_mut_ptr() as *mut c_void, buffer.len()) };
            if got < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("read failed: {}", std::io::Error::last_os_error()));
            }
            for event in parse(&buffer[..got as usize]) {
                if event.mask & FAN_ONDIR == 0 || event.name.is_empty() || event.name.contains('/') {
                    continue;
                }
                let Some(mount) = mounts.iter().find(|(fsid, _)| *fsid == event.fsid).map(|(_, fd)| *fd) else { continue };
                let Some(parent) = path_of(mount, &event.handle) else { continue };
                if !roots.contains(&parent) {
                    continue;
                }
                let folder = parent.join(&event.name).to_string_lossy().into_owned();
                if let Some(creator) = attribute(event.pid, &read_process, &is_base, &day_now()) {
                    creators.insert(folder, creator);
                    dirty = true;
                }
            }
            // A burst of folders is written once, not once for each.
            if dirty && last_save.elapsed() > Duration::from_secs(2) {
                save(&creators);
                dirty = false;
                last_save = Instant::now();
            }
        }
    }

    fn fs_id(path: &Path) -> Option<[i32; 2]> {
        let c = CString::new(path.to_string_lossy().as_bytes()).ok()?;
        let mut stat: [u8; 120] = [0; 120];
        extern "C" {
            fn statfs(path: *const std::os::raw::c_char, buf: *mut c_void) -> c_int;
        }
        let ok = unsafe { statfs(c.as_ptr(), stat.as_mut_ptr() as *mut c_void) };
        // `struct statfs` on 64-bit Linux: f_type, f_bsize, f_blocks, f_bfree, f_bavail, f_files,
        // f_ffree (8 bytes each), then f_fsid (two ints).
        (ok == 0).then(|| [i32::from_ne_bytes(stat[56..60].try_into().unwrap()), i32::from_ne_bytes(stat[60..64].try_into().unwrap())])
    }
}

/// Entry of the `soplos-sys-cleaner-tracker` program.
#[cfg(target_os = "linux")]
pub fn run() -> Result<(), String> {
    service::run()
}

#[cfg(not(target_os = "linux"))]
pub fn run() -> Result<(), String> {
    Err("the tracker only runs on Linux".into())
}
