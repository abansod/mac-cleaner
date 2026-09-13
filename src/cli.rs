use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::engine::{self, ScanKind};
use crate::models::Category;
use crate::safety::format_bytes;
use crate::ui;

#[derive(Parser)]
#[command(
    name = "mac-cleaner",
    version,
    about = "Free disk space on macOS — review junk, clutter, and duplicates before you delete.",
    long_about = None
)]
struct Cli {
    /// Faster junk-only scan (skips duplicates, orphaned files, large files, languages)
    #[arg(long, short = 's', global = true)]
    smart: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Interactive scan (default when no command is given)
    Scan {
        #[arg(long, short, value_enum, default_value_t = Mode::Full)]
        mode: Mode,
        /// Print findings instead of opening the interactive UI
        #[arg(long)]
        list: bool,
    },
    /// Print scan results without the interactive UI
    List {
        #[arg(long, short, value_enum)]
        mode: Option<Mode>,
    },
    /// Delete matching groups after confirmation
    Clean {
        /// Category name substring (e.g. "Caches", "Trash")
        #[arg(long, short)]
        category: Option<String>,
        /// Skip confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Find duplicate files (interactive)
    Duplicates {
        /// Extra folder to include
        #[arg(long, short)]
        path: Option<PathBuf>,
    },
    /// List cleanup categories
    Categories,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum Mode {
    #[default]
    Full,
    Smart,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => ui::run(kind(cli.smart)),
        Some(Command::Scan { mode, list }) => {
            let kind = kind(cli.smart || matches!(mode, Mode::Smart));
            if list {
                print_list(kind)
            } else {
                ui::run(kind)
            }
        }
        Some(Command::List { mode }) => {
            let smart = cli.smart || matches!(mode, Some(Mode::Smart));
            print_list(kind(smart))
        }
        Some(Command::Clean { category, yes }) => clean(kind(cli.smart), category, yes),
        Some(Command::Duplicates { path }) => ui::run(ScanKind::Duplicates { extra_root: path }),
        Some(Command::Categories) => {
            for cat in Category::ALL {
                println!("  • {} — {}", cat.label(), cat.hint());
            }
            Ok(())
        }
    }
}

fn kind(smart: bool) -> ScanKind {
    if smart {
        ScanKind::Smart
    } else {
        ScanKind::Full
    }
}

fn print_list(kind: ScanKind) -> Result<()> {
    let mut last = String::new();
    let result = engine::scan(kind, &mut |message, _, _| {
        if message != last {
            eprint!("\r\x1b[K{message}");
            let _ = io::stderr().flush();
            last = message.to_string();
        }
    });
    eprintln!();
    for warning in &result.warnings {
        eprintln!("{warning}");
    }
    if let Some(disk) = &result.disk {
        println!(
            "APFS container {} — {} free of {}",
            disk.mount,
            format_bytes(disk.container_free),
            format_bytes(disk.container_bytes)
        );
    }
    if result.groups.is_empty() {
        println!("Nothing found.");
        return Ok(());
    }

    println!("{}", engine::summarize(&result));
    println!();
    for category in result.categories_sorted() {
        let groups = result.groups_in(category);
        let size: u64 = groups.iter().map(|g| g.size()).sum();
        println!(
            "== {}  ({} · {} groups)",
            category.label(),
            format_bytes(size),
            groups.len()
        );
        for group in groups {
            println!(
                "  {:<40} {:>6}  {:>10}",
                truncate(&group.title, 40),
                group.count(),
                format_bytes(group.size())
            );
            if group.items.len() <= 3 {
                for item in &group.items {
                    println!("      {}", item.path.display());
                }
            }
        }
        println!();
    }
    Ok(())
}

fn clean(kind: ScanKind, category: Option<String>, yes: bool) -> Result<()> {
    let mut last = String::new();
    let mut result = engine::scan(kind, &mut |message, _, _| {
        if message != last {
            eprint!("\r\x1b[K{message}");
            let _ = io::stderr().flush();
            last = message.to_string();
        }
    });
    eprintln!();
    result.prune_empty();
    if result.groups.is_empty() {
        println!("Nothing to clean.");
        return Ok(());
    }

    let mut groups = result.groups.clone();
    if let Some(needle) = category.as_deref() {
        let needle = needle.to_lowercase();
        groups.retain(|g| {
            g.category.label().to_lowercase().contains(&needle)
                || g.title.to_lowercase().contains(&needle)
        });
        if groups.is_empty() {
            bail!("No groups matched «{needle}».");
        }
    }

    let total: u64 = groups.iter().map(|g| g.size()).sum();
    println!(
        "Will delete {} groups ({}).",
        groups.len(),
        format_bytes(total)
    );
    if !yes && !confirm_line("Proceed?") {
        return Ok(());
    }

    let items: Vec<_> = groups
        .iter()
        .flat_map(|g| g.items.iter().cloned())
        .collect();
    let mut last = String::new();
    let outcome =
        engine::delete_items_with_progress(&items, total, &mut |message, done, expected| {
            let line = format!(
                "{message}  {} / {}",
                format_bytes(done),
                format_bytes(expected)
            );
            if line != last {
                eprint!("\r\x1b[K{line}");
                let _ = io::stderr().flush();
                last = line;
            }
        });
    eprintln!();
    for err in &outcome.errors {
        eprintln!("{err}");
    }
    println!(
        "Deleted {} items, freed {}.",
        outcome.removed,
        format_bytes(outcome.freed)
    );
    Ok(())
}

fn confirm_line(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
