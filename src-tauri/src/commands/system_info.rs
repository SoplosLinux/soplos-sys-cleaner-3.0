// Read-only system summary for the Summary page: distribution, kernel,
// uptime, CPU, memory, GPU and temperatures. Everything comes from /proc,
// /sys and /etc/os-release (plus one cached `lspci` call for the GPU name),
// no privileges needed. The CPU temperature lookup is ported from 2.x's
// overview tab, including its driver priority and its distrust of acpitz.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub distro: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub cpu_model: String,
    pub cpu_threads: u32,
    pub cpu_usage_percent: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub gpu_name: Option<String>,
    pub gpu_temp_c: Option<f32>,
}

// k10temp (AMD Ryzen Tctl) and coretemp (Intel) give real readings; acpitz
// is often a stub on many boards, so it is the last resort.
const HWMON_PRIORITY: [&str; 5] = ["k10temp", "coretemp", "nct6775", "it87", "acpitz"];
const GPU_HWMON: [&str; 3] = ["amdgpu", "nouveau", "radeon"];

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn hwmons() -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = read_trim(&path.join("name")) {
            found.push((name, path));
        }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

// Best temperature of one hwmon directory: a labelled Tctl/Package/Core 0
// input when there is one, otherwise the lowest-numbered input.
fn hwmon_temp(path: &Path) -> Option<f32> {
    let mut inputs: Vec<(u32, f32)> = Vec::new();
    for entry in fs::read_dir(path).ok()?.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Some(index) = file_name.strip_prefix("temp").and_then(|r| r.strip_suffix("_input")) else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        let Some(raw) = read_trim(&entry.path()).and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        inputs.push((index, raw as f32 / 1000.0));
    }
    inputs.sort_by_key(|(index, _)| *index);

    for (index, value) in &inputs {
        if let Some(label) = read_trim(&path.join(format!("temp{index}_label"))) {
            let label = label.to_lowercase();
            if ["tctl", "package", "core 0"].iter().any(|k| label.contains(k)) {
                return Some(*value);
            }
        }
    }
    inputs.first().map(|(_, value)| *value)
}

fn cpu_temp() -> Option<f32> {
    let found = hwmons();
    for driver in HWMON_PRIORITY {
        for (name, path) in &found {
            if name == driver {
                if let Some(value) = hwmon_temp(path) {
                    return Some(value);
                }
            }
        }
    }

    let mut zones: Vec<PathBuf> = fs::read_dir("/sys/class/thermal")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("thermal_zone")).unwrap_or(false))
        .collect();
    zones.sort();
    let mut acpitz = Vec::new();
    for zone in zones {
        let Some(kind) = read_trim(&zone.join("type")) else { continue };
        let Some(value) = read_trim(&zone.join("temp")).and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        let value = value as f32 / 1000.0;
        if kind == "acpitz" {
            acpitz.push(value);
        } else {
            return Some(value);
        }
    }
    acpitz.first().copied()
}

fn gpu_temp() -> Option<f32> {
    hwmons().iter().filter(|(name, _)| GPU_HWMON.contains(&name.as_str())).find_map(|(_, path)| hwmon_temp(path))
}

// Total and idle jiffies from the aggregate line of /proc/stat.
fn cpu_sample() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let values: Vec<u64> = line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    if values.len() < 5 {
        return None;
    }
    let total: u64 = values.iter().take(8).sum();
    let idle = values[3] + values[4];
    Some((total, idle))
}

static PREVIOUS_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

// Usage between two samples. The first call has nothing to compare with, so
// it takes a short second sample itself; later calls reuse the previous poll.
fn cpu_usage() -> Option<f32> {
    let previous = PREVIOUS_CPU.lock().ok()?.take();
    let (before, now) = match previous {
        Some(before) => (before, cpu_sample()?),
        None => {
            let before = cpu_sample()?;
            std::thread::sleep(Duration::from_millis(250));
            (before, cpu_sample()?)
        }
    };
    *PREVIOUS_CPU.lock().ok()? = Some(now);

    let total = now.0.saturating_sub(before.0);
    let idle = now.1.saturating_sub(before.1);
    if total == 0 {
        return None;
    }
    Some((1.0 - idle as f32 / total as f32).clamp(0.0, 1.0) * 100.0)
}

fn meminfo_kib(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn distro() -> String {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_default()
}

// GPU names change only when hardware does, so `lspci` runs once.
fn gpu_name() -> Option<String> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| {
        let output = Command::new("lspci").arg("-mm").output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut names = Vec::new();
        for line in text.lines() {
            // Fields are quoted: slot "class" "vendor" "device" ...
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() < 6 {
                continue;
            }
            let class = parts[1];
            if !(class.contains("VGA") || class.contains("3D controller") || class.contains("Display controller")) {
                continue;
            }
            let device = parts[5];
            let device = match (device.find('['), device.rfind(']')) {
                (Some(start), Some(end)) if end > start => &device[start + 1..end],
                _ => device,
            };
            names.push(format!("{} {}", parts[3], device));
        }
        if names.is_empty() {
            None
        } else {
            Some(names.join(" / "))
        }
    })
    .clone()
}

fn collect() -> SystemInfo {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu_model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let cpu_threads = cpuinfo.lines().filter(|l| l.starts_with("processor")).count() as u32;

    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_total = meminfo_kib(&meminfo, "MemTotal") * 1024;
    let mem_available = meminfo_kib(&meminfo, "MemAvailable") * 1024;
    let swap_total = meminfo_kib(&meminfo, "SwapTotal") * 1024;
    let swap_free = meminfo_kib(&meminfo, "SwapFree") * 1024;

    let uptime_secs = read_trim(Path::new("/proc/uptime"))
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
        .unwrap_or(0.0) as u64;

    SystemInfo {
        distro: distro(),
        kernel: read_trim(Path::new("/proc/sys/kernel/osrelease")).unwrap_or_default(),
        uptime_secs,
        cpu_model,
        cpu_threads,
        cpu_usage_percent: cpu_usage(),
        cpu_temp_c: cpu_temp(),
        mem_used_bytes: mem_total.saturating_sub(mem_available),
        mem_total_bytes: mem_total,
        swap_used_bytes: swap_total.saturating_sub(swap_free),
        swap_total_bytes: swap_total,
        gpu_name: gpu_name(),
        gpu_temp_c: gpu_temp(),
    }
}

#[tauri::command]
pub async fn get_system_info() -> Option<SystemInfo> {
    tauri::async_runtime::spawn_blocking(collect).await.ok()
}
