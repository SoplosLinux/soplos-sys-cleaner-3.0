// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use soplos_sys_cleaner_lib::AppMode;

fn main() {
    // Privileged-helper mode: this same binary, re-invoked by pkexec under
    // the polkit action declared in org.soplos.syscleaner.policy, instead of
    // a separate helper executable. Must be checked before Tauri starts,
    // since this path never opens a window: it validates and runs the named
    // operations in the request file and exits. See commands::privileged.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--run-ops" {
        std::process::exit(soplos_sys_cleaner_lib::run_ops_file(&args[2]));
    }

    // Started by the alerts timer: a read-only check without a window that
    // sends a notification when a cleanup is due. See commands::alerts.
    if args.len() == 2 && args[1] == "--check" {
        std::process::exit(soplos_sys_cleaner_lib::run_check());
    }

    // Started by the alerts page's test button, through systemd: one
    // notification, no window.
    if args.len() == 4 && args[1] == "--notify" {
        std::process::exit(soplos_sys_cleaner_lib::run_notify(&args[2], &args[3]));
    }

    // Started by the basic program: wait for it to exit first.
    soplos_sys_cleaner_lib::wait_for_parent(&args);

    // WebKitGTK's DMA-BUF renderer can leave the window blank on virtual GPUs.
    // Must be set before the webview initializes, so it happens here in main().
    if soplos_sys_cleaner_lib::is_running_in_vm() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    soplos_sys_cleaner_lib::fix_unreachable_input_method();

    soplos_sys_cleaner_lib::run(AppMode::Advanced)
}
