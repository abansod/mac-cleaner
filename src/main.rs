fn main() {
    if !cfg!(target_os = "macos") {
        eprintln!("Warning: designed for macOS; some paths may be empty.");
    }
    if let Err(err) = mac_cleaner::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
