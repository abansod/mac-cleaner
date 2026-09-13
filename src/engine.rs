use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::macos_space;
use crate::models::{FileItem, ScanResult};
use crate::safety::safe_size;
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
    pub removed_paths: Vec<PathBuf>,
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
    result.disk = macos_space::disk_pressure();
    result.warnings = macos_space::scan_warnings(matches!(kind, ScanKind::Full));
    result
}

pub fn delete_items_with_progress(
    items: &[FileItem],
    expected_bytes: u64,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> DeleteOutcome {
    let total = if expected_bytes > 0 {
        expected_bytes
    } else {
        progress("Calculating size…", 0, 1);
        items
            .iter()
            .map(|item| item.size.max(safe_size(&item.path)))
            .sum()
    };
    let total = total.max(1);
    progress("Deleting…", 0, total);

    let mut outcome = DeleteOutcome::default();
    let mut done = 0u64;
    let mut last_send = Instant::now() - Duration::from_secs(1);
    let mut last_message = String::new();

    for item in items {
        if matches!(item.op, crate::models::ReclaimOp::DeletePath)
            && !item.path.exists()
            && item.path.symlink_metadata().is_err()
        {
            continue;
        }

        let mut item_freed = 0u64;
        let result = macos_space::reclaim(item, &mut |message, bytes| {
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
            Ok(freed) => {
                if item_freed == 0 && freed > 0 {
                    outcome.freed = outcome.freed.saturating_add(freed);
                    done = done.saturating_add(freed);
                }
                outcome.removed += 1;
                outcome.removed_paths.push(item.path.clone());
            }
            Err(err) => outcome
                .errors
                .push(format!("{}: {err}", item.path.display())),
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
        let item = FileItem::file(
            file.clone(),
            1024,
            crate::models::Category::Temp,
            "test",
            "test",
        );
        let outcome = delete_items_with_progress(std::slice::from_ref(&item), 0, &mut |_, _, _| {});
        assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
        assert_eq!(outcome.removed, 1);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
