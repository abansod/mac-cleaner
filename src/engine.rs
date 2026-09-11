use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::models::ScanResult;
use crate::safety::{delete_path_with_progress, is_safe_to_delete, safe_size};
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

pub fn delete_items_with_progress(
    items: &[PathBuf],
    expected_bytes: u64,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> DeleteOutcome {
    let total = if expected_bytes > 0 {
        expected_bytes
    } else {
        progress("Calculating size…", 0, 1);
        items.iter().map(|p| safe_size(p)).sum()
    };
    let total = total.max(1);
    progress("Deleting…", 0, total);

    let mut outcome = DeleteOutcome::default();
    let mut done = 0u64;
    let mut last_send = Instant::now() - Duration::from_secs(1);
    let mut last_message = String::new();

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

        let mut item_freed = 0u64;
        let result = delete_path_with_progress(path, &mut |message, bytes| {
            item_freed = item_freed.saturating_add(bytes);
            done = done.saturating_add(bytes);
            let force = message != last_message;
            last_message = message.to_string();
            if force || last_send.elapsed() >= Duration::from_millis(40) {
                let display_total = total.max(done);
                progress(message, done.min(display_total), display_total);
                last_send = Instant::now();
            }
        });
        outcome.freed = outcome.freed.saturating_add(item_freed);
        match result {
            Ok(()) => outcome.removed += 1,
            Err(err) => outcome.errors.push(format!("{}: {err}", path.display())),
        }
        let display_total = total.max(done).max(1);
        progress(
            if last_message.is_empty() {
                "Deleting…"
            } else {
                &last_message
            },
            done.min(display_total),
            display_total,
        );
        last_send = Instant::now();
    }

    let display_total = total.max(done).max(1);
    progress("Finishing…", display_total, display_total);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn delete_items_cleans_tmp_file() {
        let dir = PathBuf::from("/tmp").join(format!(
            "mac-cleaner-eng-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gone.txt");
        fs::write(&file, vec![0u8; 1024]).unwrap();
        let outcome = delete_items_with_progress(std::slice::from_ref(&file), 0, &mut |_, _, _| {});
        assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
        assert_eq!(outcome.removed, 1);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
