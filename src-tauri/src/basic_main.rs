// The basic program: a minimal window with the health ring and the standard
// cleanup. It shares everything else with the advanced program.

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use soplos_sys_cleaner_lib::AppMode;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // The privileged helper lives in the advanced binary. This one must never
    // be started as root, so a request for it is refused outright.
    if args.iter().any(|a| a == "--run-ops") {
        eprintln!("soplos-sys-cleaner-basic: privileged operations run through soplos-sys-cleaner");
        std::process::exit(1);
    }

    soplos_sys_cleaner_lib::wait_for_parent(&args);

    if soplos_sys_cleaner_lib::is_running_in_vm() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    soplos_sys_cleaner_lib::fix_unreachable_input_method();

    soplos_sys_cleaner_lib::run(AppMode::Basic)
}
