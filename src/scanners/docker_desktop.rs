use std::path::PathBuf;

use crate::docker_desktop::{
    app_support, cache_root, container_data, is_running, unused_disk_images, DockerState,
    ELECTRON_CACHE_DIRS,
};
use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{allocated_size, format_bytes, home_dir, is_safe_to_delete, list_children};

use super::Scanner;

const MIN_CACHE_ITEM: u64 = 1024;
const RUNNING_NOTE: &str = " Docker Desktop is running; quit it before deleting.";

pub struct DockerCacheScanner;

impl Scanner for DockerCacheScanner {
    fn name(&self) -> &'static str {
        "Docker Desktop Cache"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Docker Desktop cache…");
        let home = home_dir();
        let mut items = Vec::new();
        for child in list_children(&cache_root(&home)) {
            push_cache_item(&mut items, child, "Docker Desktop app cache");
        }
        let support = app_support(&home);
        for name in ELECTRON_CACHE_DIRS {
            push_cache_item(
                &mut items,
                support.join(name),
                "Docker Desktop dashboard (Electron) cache",
            );
        }
        if items.is_empty() {
            return Vec::new();
        }
        let size: u64 = items.iter().map(|i| i.size).sum();
        let mut description = format!(
            "{} · {} item(s). Rebuilt on the next launch; images, containers, and volumes are untouched.",
            format_bytes(size),
            items.len()
        );
        if is_running() {
            description.push_str(RUNNING_NOTE);
        }
        vec![FileGroup {
            key: "docker-cache".into(),
            category: Category::DockerCache,
            title: "Docker Desktop cache".into(),
            description,
            items,
        }]
    }
}

pub struct DockerDiskScanner;

impl Scanner for DockerDiskScanner {
    fn name(&self) -> &'static str {
        "Unused Docker Desktop Disk Images"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Docker Desktop disk images…");
        let state = DockerState::detect();
        unused_disk_images(state)
            .into_iter()
            .filter(|disk| is_safe_to_delete(&disk.path))
            .filter_map(|disk| {
                let size = allocated_size(&disk.path);
                if size == 0 {
                    return None;
                }
                let logical = disk.path.symlink_metadata().map(|m| m.len()).unwrap_or(0);
                let name = disk
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let location = disk
                    .path
                    .strip_prefix(container_data(&home_dir()))
                    .map(|rel| rel.display().to_string())
                    .unwrap_or_else(|_| disk.path.display().to_string());
                let mut description = format!(
                    "{}. {} used of {} virtual. Deleting removes every image, container, and volume stored in it.",
                    disk.reason,
                    format_bytes(size),
                    format_bytes(logical)
                );
                if state.running {
                    description.push_str(RUNNING_NOTE);
                }
                Some(FileGroup {
                    key: format!("docker-disk:{}", disk.path.display()),
                    category: Category::DockerDisks,
                    title: format!("Disk image — {location}"),
                    description,
                    items: vec![FileItem::file(
                        disk.path,
                        size,
                        Category::DockerDisks,
                        disk.reason,
                        name,
                    )],
                })
            })
            .collect()
    }
}

fn push_cache_item(items: &mut Vec<FileItem>, path: PathBuf, reason: &str) {
    if path.file_name().and_then(|n| n.to_str()) == Some(".DS_Store") {
        return;
    }
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
        Category::DockerCache,
        reason,
        name,
    ));
}
