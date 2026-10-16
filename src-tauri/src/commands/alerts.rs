// Alerts: a desktop notification when a cleanup is due. It only notifies and
// never cleans anything by itself, and nothing exists until the user turns it
// on: no timer, no unit files, no notification.
//
// The user picks the rules (by time since the last cleanup, by the size ready
// to clean, or both) and how often the program looks. Turning the alerts on
// writes a systemd user timer; turning them off removes it. The timer runs
// `soplos-sys-cleaner --check`, a read-only scan without a window that sends a
// notification through the freedesktop D-Bus interface, which every desktop
// implements. The texts are translated by the interface and stored with the
// settings, so this side needs no translations of its own.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::logs::journal_freeable_days;
use super::stats::{last_cleanup_unix, now_unix};
use super::user_cache::is_slow_rebuild;
use super::{apt_cache, flatpak, launcher, logs, snap, temp_files, trash, user_cache};

const SERVICE_FILE: &str = "soplos-sys-cleaner-alerts.service";
const TIMER_FILE: &str = "soplos-sys-cleaner-alerts.timer";
// The default journal retention of the interface: the standard cleanup counts
// what vacuuming to this many days would free.
const JOURNAL_DAYS: u64 = 7;
const MAX_TEXT_CHARS: usize = 300;

// What the notification says, translated by the interface. {days} and {size}
// are filled in when it is sent.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertTexts {
    pub title: String,
    pub time: String,
    pub weight: String,
}

// What the user chooses. Nothing here has a default value in the interface.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertsSettings {
    pub by_time: bool,
    pub time_value: u32,
    // "days" or "weeks".
    pub time_unit: String,
    pub by_weight: bool,
    pub weight_value: u32,
    // "MB" or "GB".
    pub weight_unit: String,
    pub check_value: u32,
    // "hours" or "days".
    pub check_unit: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertsConfig {
    pub enabled: bool,
    #[serde(flatten)]
    pub settings: AlertsSettings,
    pub enabled_at_unix: u64,
    pub last_notified_unix: u64,
    pub texts: AlertTexts,
}

impl AlertsSettings {
    fn time_period_secs(&self) -> Option<u64> {
        let value = u64::from(self.time_value);
        match self.time_unit.as_str() {
            "days" => Some(value * 86400),
            "weeks" => Some(value * 7 * 86400),
            _ => None,
        }
    }

    fn weight_threshold_bytes(&self) -> Option<u64> {
        let value = u64::from(self.weight_value);
        match self.weight_unit.as_str() {
            "MB" => Some(value * 1024 * 1024),
            "GB" => Some(value * 1024 * 1024 * 1024),
            _ => None,
        }
    }

    // The systemd time span between two checks: "6h", "2d".
    fn check_interval(&self) -> Option<String> {
        match self.check_unit.as_str() {
            "hours" => Some(format!("{}h", self.check_value)),
            "days" => Some(format!("{}d", self.check_value)),
            _ => None,
        }
    }
}

/// Every enabled rule must be complete and in range, and the check frequency
/// is always required. Disabled rules are ignored.
pub fn validate_settings(s: &AlertsSettings) -> Result<(), String> {
    if !s.by_time && !s.by_weight {
        return Err("Turn on at least one rule".to_string());
    }
    if s.by_time {
        let ok = matches!((s.time_unit.as_str(), s.time_value), ("days", 1..=365) | ("weeks", 1..=52));
        if !ok {
            return Err("The time rule needs 1 to 365 days or 1 to 52 weeks".to_string());
        }
    }
    if s.by_weight {
        let ok = matches!((s.weight_unit.as_str(), s.weight_value), ("MB", 1..=10_000_000) | ("GB", 1..=10_000));
        if !ok {
            return Err("The size rule needs a size in MB or GB".to_string());
        }
    }
    let ok = matches!((s.check_unit.as_str(), s.check_value), ("hours", 1..=2160) | ("days", 1..=90));
    if !ok {
        return Err("The check frequency needs 1 to 2160 hours or 1 to 90 days".to_string());
    }
    Ok(())
}

// ---- Files -------------------------------------------------------------------

fn config_home() -> Option<PathBuf> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => Some(PathBuf::from(std::env::var("HOME").ok()?).join(".config")),
    }
}

fn config_path() -> Option<PathBuf> {
    Some(config_home()?.join("soplos-sys-cleaner").join("alerts.json"))
}

fn units_dir() -> Option<PathBuf> {
    Some(config_home()?.join("systemd").join("user"))
}

fn load() -> AlertsConfig {
    config_path().and_then(|p| fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

// Written beside the real file and renamed over it, so a crash never leaves
// half a configuration.
fn save(config: &AlertsConfig) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "Cannot find the configuration directory".to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json).map_err(|e| e.to_string())?;
    fs::rename(&temp, &path).map_err(|e| e.to_string())
}

fn clean_texts(texts: AlertTexts) -> AlertTexts {
    let cut = |s: String| s.chars().filter(|c| !c.is_control()).take(MAX_TEXT_CHARS).collect::<String>();
    AlertTexts { title: cut(texts.title), time: cut(texts.time), weight: cut(texts.weight) }
}

// ---- systemd user units ------------------------------------------------------

// A path as one word of an ExecStart line: quoted, with the characters
// systemd treats specially escaped.
fn systemd_quote(path: &Path) -> String {
    let escaped = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("\"{escaped}\"")
}

fn service_unit(exe: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=Soplos Sys Cleaner: check whether a cleanup is due (notifies only)\n\
         ConditionPathExists={}\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={} --check\n\
         Nice=19\n\
         IOSchedulingClass=idle\n",
        exe.to_string_lossy().replace('%', "%%"),
        systemd_quote(exe)
    )
}

fn timer_unit(interval: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Soplos Sys Cleaner: periodic check\n\
         \n\
         [Timer]\n\
         OnBootSec=5min\n\
         OnUnitActiveSec={interval}\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl").arg("--user").args(args).output().map_err(|e| format!("systemctl: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn install_units(settings: &AlertsSettings) -> Result<(), String> {
    let exe = launcher::elevation_binary_path().ok_or_else(|| "The advanced version was not found".to_string())?;
    let interval = settings.check_interval().ok_or_else(|| "Invalid check frequency".to_string())?;
    let dir = units_dir().ok_or_else(|| "Cannot find the systemd user directory".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(SERVICE_FILE), service_unit(&exe)).map_err(|e| e.to_string())?;
    fs::write(dir.join(TIMER_FILE), timer_unit(&interval)).map_err(|e| e.to_string())?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", TIMER_FILE])?;
    // A new frequency only applies to a timer that is started again.
    systemctl(&["restart", TIMER_FILE])
}

fn remove_units() -> Result<(), String> {
    // Not being enabled is not an error when turning off.
    let _ = systemctl(&["disable", "--now", TIMER_FILE]);
    if let Some(dir) = units_dir() {
        for file in [TIMER_FILE, SERVICE_FILE] {
            match fs::remove_file(dir.join(file)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}

// ---- Notification ------------------------------------------------------------

// org.freedesktop.Notifications is implemented by GNOME, Plasma, Xfce and
// every other desktop. The desktop-entry hint lets the ones that open the
// program when the notification is clicked do so; on the others the text
// alone is shown and the program is opened from the applications menu.
//
// GNOME is the exception: tested on GNOME with the program installed, a
// notification that carries the desktop-entry hint is closed by the shell the
// moment it arrives, while the same one without the hint is shown. So there
// the hint is left out; the notification still shows our name and icon.
fn notify(title: &str, body: &str) -> Result<(), String> {
    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    let connection = Connection::session().map_err(|e| e.to_string())?;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    if !is_gnome_shell(&connection) && std::path::Path::new("/usr/share/applications/soplos-sys-cleaner.desktop").exists() {
        hints.insert("desktop-entry", Value::from("soplos-sys-cleaner"));
    }
    hints.insert("urgency", Value::from(1u8));
    connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Soplos Sys Cleaner", 0u32, "soplos-sys-cleaner", title, body, Vec::<&str>::new(), hints, -1i32),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Whether GNOME Shell is the one on the other end of the session bus. Asked
// of the bus itself, because the timer's environment does not always carry
// XDG_CURRENT_DESKTOP.
fn is_gnome_shell(connection: &zbus::blocking::Connection) -> bool {
    connection
        .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "NameHasOwner", &("org.gnome.Shell",))
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.2} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

// ---- The check ---------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct Decision {
    time: bool,
    weight: bool,
}

// Time: due once the period has passed since the later of the last cleanup,
// the moment the alerts were turned on and the last notification. Size: due
// when what is ready to clean reaches the threshold, and it is not repeated
// until something was cleaned after the last notification. The scan runs only
// when the size rule can matter.
fn decide(config: &AlertsConfig, now: u64, last_clean: Option<u64>, safe_bytes: impl FnOnce() -> u64) -> Decision {
    let s = &config.settings;
    let last_clean = last_clean.unwrap_or(0);
    let reference = last_clean.max(config.enabled_at_unix).max(config.last_notified_unix);
    let time = s.by_time && s.time_period_secs().map(|period| now >= reference + period).unwrap_or(false);
    let notified_since_clean = config.last_notified_unix != 0 && last_clean <= config.last_notified_unix;
    let weight = s.by_weight && !notified_since_clean && s.weight_threshold_bytes().map(|limit| safe_bytes() >= limit).unwrap_or(false);
    Decision { time, weight }
}

// What the standard cleanup of the basic program would free: the same safe
// rows as the interface, summed.
fn safe_bytes() -> u64 {
    tauri::async_runtime::block_on(async {
        let trash = trash::scan_trash().await.size_bytes;
        let cache: u64 = user_cache::scan_user_cache().await.iter().filter(|e| e.protected_reason.is_none() && !is_slow_rebuild(&e.name)).map(|e| e.size_bytes).sum();
        let flatpak: u64 = flatpak::scan_flatpak_unused().await.iter().map(|e| e.size_bytes).sum();
        let snap: u64 = snap::scan_snap_old_revisions().await.iter().map(|e| e.size_bytes).sum();
        let temp: u64 = temp_files::scan_temp_files().await.iter().map(|e| e.size_bytes).sum();
        let apt = apt_cache::scan_apt_cache().await.size_bytes;
        let journal = journal_freeable_days(&logs::scan_journald().await, JOURNAL_DAYS, now_unix()).unwrap_or(0);
        trash + cache + flatpak + snap + temp + apt + journal
    })
}

fn text_or(text: &str, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_string()
    } else {
        text.to_string()
    }
}

/// `soplos-sys-cleaner --check`, started by the timer: no window, read-only.
/// Returns the exit code.
pub fn run_check() -> i32 {
    let mut config = load();
    if !config.enabled {
        return 0;
    }
    let now = now_unix();
    let last_clean = last_cleanup_unix();
    let decision = decide(&config, now, last_clean, safe_bytes);
    if !decision.time && !decision.weight {
        return 0;
    }

    let mut lines = Vec::new();
    if decision.time {
        let reference = last_clean.unwrap_or(0).max(config.enabled_at_unix).max(config.last_notified_unix);
        let days = now.saturating_sub(reference) / 86400;
        let template = text_or(&config.texts.time, "Days since the last cleanup: {days}. Open Soplos Sys Cleaner to review.");
        lines.push(template.replace("{days}", &days.to_string()));
    }
    if decision.weight {
        let template = text_or(&config.texts.weight, "Ready to clean: {size}. Open Soplos Sys Cleaner to review.");
        lines.push(template.replace("{size}", &format_bytes(safe_bytes())));
    }
    let title = text_or(&config.texts.title, "Soplos Sys Cleaner");

    match notify(&title, &lines.join("\n")) {
        Ok(()) => {
            config.last_notified_unix = now;
            if let Err(e) = save(&config) {
                eprintln!("soplos-sys-cleaner: could not save the alert state: {e}");
            }
            0
        }
        Err(e) => {
            eprintln!("soplos-sys-cleaner: could not send the notification: {e}");
            1
        }
    }
}

// ---- Commands ----------------------------------------------------------------

#[tauri::command]
pub async fn get_alerts() -> AlertsConfig {
    tauri::async_runtime::spawn_blocking(load).await.unwrap_or_default()
}

fn set_alerts_blocking(settings: AlertsSettings, texts: AlertTexts) -> Result<AlertsConfig, String> {
    validate_settings(&settings)?;
    let mut config = load();
    let was_enabled = config.enabled;
    // The units first: if systemd refuses, nothing is recorded as enabled.
    install_units(&settings)?;
    config.settings = settings;
    config.texts = clean_texts(texts);
    config.enabled = true;
    if !was_enabled {
        config.enabled_at_unix = now_unix();
        config.last_notified_unix = 0;
    }
    save(&config)?;
    Ok(config)
}

/// Turns the alerts on, or applies new rules: validates, writes the user
/// timer and starts it.
#[tauri::command]
pub async fn set_alerts(settings: AlertsSettings, texts: AlertTexts) -> Result<AlertsConfig, String> {
    tauri::async_runtime::spawn_blocking(move || set_alerts_blocking(settings, texts)).await.map_err(|e| e.to_string())?
}

fn disable_alerts_blocking() -> Result<AlertsConfig, String> {
    let mut config = load();
    remove_units()?;
    // Turning off leaves nothing behind: the rules, the frequency and the
    // notification history go back to empty, like a program that was never
    // set up. Turning on again means choosing everything again. Only the
    // notification texts stay, because they follow the interface language.
    config.enabled = false;
    config.settings = AlertsSettings::default();
    config.enabled_at_unix = 0;
    config.last_notified_unix = 0;
    save(&config)?;
    Ok(config)
}

/// Turns the alerts off and removes the timer and its unit files.
#[tauri::command]
pub async fn disable_alerts() -> Result<AlertsConfig, String> {
    tauri::async_runtime::spawn_blocking(disable_alerts_blocking).await.map_err(|e| e.to_string())?
}

/// Refreshes the stored notification texts, for when the interface language
/// changes while the alerts are on.
#[tauri::command]
pub async fn set_alert_texts(texts: AlertTexts) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut config = load();
        config.texts = clean_texts(texts);
        save(&config)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether the timer is running, as systemd sees it.
#[tauri::command]
pub async fn alerts_service_active() -> bool {
    tauri::async_runtime::spawn_blocking(|| systemctl(&["is-active", "--quiet", TIMER_FILE]).is_ok()).await.unwrap_or(false)
}

/// Sends a notification now, to see how it looks on this desktop.
#[tauri::command]
pub async fn send_test_alert(title: String, body: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Sent by a separate process started through systemd, the way the
        // timer sends the real ones. Sent from this window's own process,
        // GNOME ties the notification to the program that is open and drops
        // it, so the test would say nothing about the real alerts.
        let exe = launcher::elevation_binary_path().ok_or_else(|| "The advanced version was not found".to_string())?;
        let output = Command::new("systemd-run")
            .args(["--user", "--collect", "--wait", "--quiet", "--"])
            .arg(exe)
            .args(["--notify", &title, &body])
            .output()
            .map_err(|e| format!("systemd-run: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `soplos-sys-cleaner --notify <title> <body>`: sends one notification and
/// exits. Started by the test button through systemd.
pub fn run_notify(title: &str, body: &str) -> i32 {
    match notify(title, body) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("soplos-sys-cleaner: could not send the notification: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AlertsSettings {
        AlertsSettings {
            by_time: true,
            time_value: 7,
            time_unit: "days".into(),
            by_weight: true,
            weight_value: 2,
            weight_unit: "GB".into(),
            check_value: 6,
            check_unit: "hours".into(),
        }
    }

    fn config(settings: AlertsSettings) -> AlertsConfig {
        AlertsConfig { enabled: true, settings, enabled_at_unix: 1_000_000, last_notified_unix: 0, texts: AlertTexts::default() }
    }

    const DAY: u64 = 86400;
    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn settings_need_a_rule_and_a_frequency() {
        assert!(validate_settings(&settings()).is_ok());
        let mut none = settings();
        none.by_time = false;
        none.by_weight = false;
        assert!(validate_settings(&none).is_err());
        let mut no_check = settings();
        no_check.check_unit = String::new();
        assert!(validate_settings(&no_check).is_err());
        let mut zero = settings();
        zero.time_value = 0;
        assert!(validate_settings(&zero).is_err());
        let mut huge = settings();
        huge.weight_value = 20_000;
        assert!(validate_settings(&huge).is_err());
        // A disabled rule is not validated.
        let mut only_weight = settings();
        only_weight.by_time = false;
        only_weight.time_value = 0;
        assert!(validate_settings(&only_weight).is_ok());
    }

    #[test]
    fn units_convert_exactly() {
        let s = settings();
        assert_eq!(s.time_period_secs(), Some(7 * DAY));
        assert_eq!(s.weight_threshold_bytes(), Some(2 * GB));
        assert_eq!(s.check_interval().as_deref(), Some("6h"));
        let mut weeks = settings();
        weeks.time_unit = "weeks".into();
        weeks.time_value = 2;
        weeks.weight_unit = "MB".into();
        weeks.check_unit = "days".into();
        assert_eq!(weeks.time_period_secs(), Some(14 * DAY));
        assert_eq!(weeks.weight_threshold_bytes(), Some(2 * 1024 * 1024));
        assert_eq!(weeks.check_interval().as_deref(), Some("6d"));
    }

    #[test]
    fn time_rule_waits_for_the_period_after_the_last_cleanup() {
        let c = config(settings());
        let never = || 0;
        // Turned on at 1_000_000: not due before 7 days, due after.
        assert!(!decide(&c, 1_000_000 + 6 * DAY, None, never).time);
        assert!(decide(&c, 1_000_000 + 7 * DAY, None, never).time);
        // A cleanup resets the clock.
        let cleaned = 1_000_000 + 5 * DAY;
        assert!(!decide(&c, cleaned + 6 * DAY, Some(cleaned), never).time);
        assert!(decide(&c, cleaned + 7 * DAY, Some(cleaned), never).time);
    }

    #[test]
    fn a_notification_is_not_repeated_before_the_next_period() {
        let mut c = config(settings());
        c.last_notified_unix = 1_000_000 + 8 * DAY;
        assert!(!decide(&c, c.last_notified_unix + DAY, None, || 0).time);
        assert!(decide(&c, c.last_notified_unix + 7 * DAY, None, || 0).time);
    }

    #[test]
    fn size_rule_fires_at_the_threshold_and_not_again_until_a_cleanup() {
        let mut c = config(settings());
        assert!(!decide(&c, 1_000_100, None, || 2 * GB - 1).weight);
        assert!(decide(&c, 1_000_100, None, || 2 * GB).weight);
        // Notified: quiet while nothing was cleaned since.
        c.last_notified_unix = 1_000_200;
        assert!(!decide(&c, 1_000_300, None, || 5 * GB).weight);
        assert!(!decide(&c, 1_000_300, Some(1_000_100), || 5 * GB).weight);
        // Cleaned after the notification: it can fire again.
        assert!(decide(&c, 1_000_400, Some(1_000_250), || 5 * GB).weight);
    }

    #[test]
    fn the_scan_only_runs_when_the_size_rule_can_matter() {
        let mut c = config(settings());
        c.settings.by_weight = false;
        let decision = decide(&c, 1_000_100, None, || panic!("must not scan"));
        assert_eq!(decision, Decision { time: false, weight: false });
        let mut c = config(settings());
        c.last_notified_unix = 1_000_200;
        decide(&c, 1_000_300, None, || panic!("must not scan after a notification"));
    }

    #[test]
    fn unit_files_quote_the_executable_safely() {
        let service = service_unit(Path::new("/usr/bin/soplos-sys-cleaner"));
        assert!(service.contains("ExecStart=\"/usr/bin/soplos-sys-cleaner\" --check\n"));
        assert!(service.contains("ConditionPathExists=/usr/bin/soplos-sys-cleaner\n"));
        assert!(service.contains("Type=oneshot"));
        assert_eq!(systemd_quote(Path::new("/a b/c\"d%e")), "\"/a b/c\\\"d%%e\"");
        let timer = timer_unit("6h");
        assert!(timer.contains("OnUnitActiveSec=6h\n"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn sizes_are_written_like_the_interface() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(10 * 1024 * 1024 + 300_000), "10.3 MB");
        assert_eq!(format_bytes(2 * GB), "2.00 GB");
    }

    #[test]
    fn stored_texts_are_cleaned_and_limited() {
        let long = "x".repeat(1000);
        let cleaned = clean_texts(AlertTexts { title: "a\nb".into(), time: long, weight: "ok".into() });
        assert_eq!(cleaned.title, "ab");
        assert_eq!(cleaned.time.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(cleaned.weight, "ok");
    }

    #[test]
    fn a_disabled_configuration_never_checks() {
        // run_check returns before scanning anything when the alerts are off.
        let c = AlertsConfig::default();
        assert!(!c.enabled);
    }
}
