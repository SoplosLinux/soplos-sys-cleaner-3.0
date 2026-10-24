mod commands;
mod system_theme;
pub(crate) mod utils;

pub use commands::alerts::{run_check, run_notify};
pub use commands::launcher::wait_for_parent;
pub use commands::privileged::run_ops_file;
pub use utils::{fix_unreachable_input_method, is_running_in_vm};

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// The two programs of the package share this library. They differ in their
/// window and in the page it shows.
#[derive(Clone, Copy, PartialEq)]
pub enum AppMode {
    Advanced,
    Basic,
}

#[derive(Clone, serde::Serialize)]
struct AnotherInstance {
    // File name of the binary that was just launched.
    binary: String,
}

// The window is created here rather than from tauri.conf.json so each
// program can have its own size and page. The advanced one keeps the values
// of the configuration file.
fn create_main_window(app: &tauri::App, mode: AppMode) -> tauri::Result<()> {
    match mode {
        AppMode::Advanced => {
            if let Some(config) = app.config().app.windows.first() {
                WebviewWindowBuilder::from_config(app, config)?.build()?;
            }
        }
        AppMode::Basic => {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("basic.html".into()))
                .title("Soplos Sys Cleaner")
                .inner_size(780.0, 440.0)
                .min_inner_size(640.0, 420.0)
                .center()
                .build()?;
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(mode: AppMode) {
    let own_binary = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();

    tauri::Builder::default()
        // Must be the first plugin registered: it intercepts and exits a
        // second launch before anything else runs. Both programs share one
        // application identifier, so this also keeps them from running at the
        // same time: whichever starts second hands over to the first, which
        // focuses its window and says which program was refused.
        .plugin(tauri_plugin_single_instance::init(move |app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let launched = args.first().map(|a| std::path::Path::new(a).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default();
            if !launched.is_empty() && launched != own_binary {
                let _ = app.emit("another-instance", AnotherInstance { binary: launched });
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            create_main_window(app, mode)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_theme::get_system_theme,
            commands::environment::get_environment_info,
            commands::jobs::poll_job,
            commands::trash::scan_trash,
            commands::trash::empty_trash,
            commands::user_cache::scan_user_cache,
            commands::user_cache::delete_cache_entries,
            commands::flatpak::scan_flatpak_unused,
            commands::flatpak::flatpak_uninstall_user,
            commands::orphans::scan_orphan_packages,
            commands::residual::scan_residual_configs,
            commands::other_apps::scan_flatpak_apps,
            commands::other_apps::scan_snap_apps,
            commands::other_apps::scan_appimages,
            commands::other_apps::remove_appimages,
            commands::snap::scan_snap_old_revisions,
            commands::apps::scan_installed_apps,
            commands::apt_cache::scan_apt_cache,
            commands::apt_cache::scan_apt_debs,
            commands::temp_files::scan_temp_files,
            commands::logs::scan_journald,
            commands::privileged::run_privileged_ops,
            commands::kernels::scan_kernels,
            commands::drivers::scan_drivers,
            commands::firmware::scan_firmware,
            commands::languages::scan_languages,
            commands::docs::scan_docs,
            commands::disk::scan_disk,
            commands::disk::disk_home,
            commands::simulate::simulate_removal,
            commands::stats::get_stats,
            commands::stats::record_cleanup,
            commands::stats::record_scan,
            commands::stats::export_text_file,
            commands::stats::scan_storage,
            commands::system_info::get_system_info,
            commands::launcher::open_advanced,
            commands::launcher::advanced_shortcut_exists,
            commands::launcher::set_advanced_shortcut,
            commands::alerts::get_alerts,
            commands::alerts::set_alerts,
            commands::alerts::disable_alerts,
            commands::alerts::set_alert_texts,
            commands::alerts::alerts_service_active,
            commands::alerts::send_test_alert,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Soplos Sys Cleaner");
}
