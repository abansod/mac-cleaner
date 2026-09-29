use std::path::Path;

use crate::colima::{cache_root, colima_homes, profile_name, unused_disks, DATADISK_FILE};
use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{allocated_size, format_bytes, list_children};

use super::Scanner;

const MIN_CACHE_ITEM: u64 = 1024;

pub struct ColimaCacheScanner;

impl Scanner for ColimaCacheScanner {
    fn name(&self) -> &'static str {
        "Colima Cache"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Colima cache…");
        let root = cache_root();
        let mut items = Vec::new();
        for child in list_children(&root) {
            if is_ds_store(&child) {
                continue;
            }
            // `caches/` holds one file per downloaded VM image; list each separately.
            if child.file_name().and_then(|n| n.to_str()) == Some("caches") && child.is_dir() {
                for entry in list_children(&child) {
                    if !is_ds_store(&entry) {
                        push_cache_item(&mut items, entry, "Downloaded Colima VM image");
                    }
                }
                continue;
            }
            push_cache_item(&mut items, child, "Colima cache data");
        }
        if items.is_empty() {
            return Vec::new();
        }
        let size: u64 = items.iter().map(|i| i.size).sum();
        vec![FileGroup {
            key: "colima-cache".into(),
            category: Category::ColimaCache,
            title: "Colima download cache".into(),
            description: format!(
                "{} · {} item(s) in ~/Library/Caches/colima. Running VMs are unaffected; images are re-downloaded when needed.",
                format_bytes(size),
                items.len()
            ),
            items,
        }]
    }
}

pub struct ColimaDiskScanner;

impl Scanner for ColimaDiskScanner {
    fn name(&self) -> &'static str {
        "Unused Colima Data Disks"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Colima data disks…");
        let mut groups = Vec::new();
        for home in colima_homes() {
            for disk in unused_disks(&home) {
                let size = allocated_size(&disk);
                if size == 0 {
                    continue;
                }
                let name = disk
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let profile = profile_name(&name);
                let logical = disk
                    .join(DATADISK_FILE)
                    .symlink_metadata()
                    .map(|m| m.len())
                    .unwrap_or(0);
                groups.push(FileGroup {
                    key: format!("colima-disk:{}", disk.display()),
                    category: Category::ColimaDisks,
                    title: format!("Data disk — profile {profile}"),
                    description: format!(
                        "Container data left by deleted Colima profile “{profile}” ({} used of {} virtual). No instance uses it. Deleting removes its images, containers, and volumes.",
                        format_bytes(size),
                        format_bytes(logical)
                    ),
                    items: vec![FileItem::file(
                        disk,
                        size,
                        Category::ColimaDisks,
                        "Unused Colima data disk (not attached to any instance)",
                        name,
                    )],
                });
            }
        }
        groups
    }
}

fn push_cache_item(items: &mut Vec<FileItem>, path: std::path::PathBuf, reason: &str) {
    let Ok(meta) = path.symlink_metadata() else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    let size = allocated_size(&path);
    if size < MIN_CACHE_ITEM {
        return;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    items.push(FileItem::file(
        path,
        size,
        Category::ColimaCache,
        reason,
        name,
    ));
}

fn is_ds_store(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some(".DS_Store")
}
