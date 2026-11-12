// The service that records who creates the folders of the home (see `tracker.rs`). It runs as
// root, started by systemd.

fn main() {
    if let Err(error) = soplos_sys_cleaner_lib::tracker::run() {
        eprintln!("soplos-sys-cleaner-tracker: {error}");
        std::process::exit(1);
    }
}
