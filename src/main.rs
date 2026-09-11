mod cli;
mod engine;
mod models;
mod safety;
mod scanners;
mod ui;

fn main() {
    if !cfg!(target_os = "macos") {
        eprintln!("Warning: designed for macOS; some paths may be empty.");
    }
    if let Err(err) = cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
