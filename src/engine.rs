use std::path::PathBuf;

use crate::models::ScanResult;
use crate::safety::{delete_path, is_safe_to_delete, safe_size};
use crate::scanners::{all_scanners, duplicate_scanner, smart_scanners, Scanner};

#[derive(Debug, Clone)]
pub enum ScanKind {
    Full,
    Smart,
    Duplicates { extra_root: Option<PathBuf> },
}

#[derive(Debug, Default)]
pub struct DeleteOutcome {
    pub removed: usize,
    pub freed: u64,
    pub errors: Vec<String>,
}

pub fn scan(kind: ScanKind, progress: &mut dyn FnMut(&str, usize, usize)) -> ScanResult {
    let scanners: Vec<Box<dyn Scanner>> = match &kind {
        ScanKind::Full => all_scanners(),
        ScanKind::Smart => smart_scanners(),
        ScanKind::Duplicates { extra_root } => {
            vec![Box::new(duplicate_scanner(extra_root.clone()))]
        }
    };

    let total = scanners.len().max(1);
    let mut result = ScanResult::default();
    for (index, scanner) in scanners.iter().enumerate() {
        progress(scanner.name(), index, total);
        let groups = scanner.scan(&mut |message| progress(message, index, total));
        result.groups.extend(groups);
    }
    result.prune_empty();
    result
}

pub fn delete_items(items: &[PathBuf]) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    for path in items {
        if !path.exists() && path.symlink_metadata().is_err() {
            continue;
        }
        if !is_safe_to_delete(path) {
            outcome
                .errors
                .push(format!("Refused (protected): {}", path.display()));
            continue;
        }
        let size = safe_size(path);
        match delete_path(path) {
            Ok(()) => {
                outcome.removed += 1;
                outcome.freed = outcome.freed.saturating_add(size);
            }
            Err(err) => outcome.errors.push(format!("{}: {err}", path.display())),
        }
    }
    outcome
}

pub fn summarize(result: &ScanResult) -> String {
    format!(
        "Found {} items in {} groups ({})",
        result.total_files(),
        result.groups.len(),
        crate::safety::format_bytes(result.total_size())
    )
}
