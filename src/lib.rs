mod cli;
mod engine;
mod login_items;
mod macos_space;
pub mod models;
pub mod safety;
pub mod scanners;
mod ui;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
