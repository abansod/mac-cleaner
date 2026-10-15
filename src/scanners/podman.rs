use super::Scanner;
use crate::models::{Category, FileGroup, FileItem};
use crate::podman::{candidates, is_running, Roots};
use crate::safety::{allocated_size, format_bytes, is_safe_to_delete};
pub struct PodmanCacheScanner;
pub struct PodmanDiskScanner;
fn scan(cache: bool, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
    progress("Scanning Podman machine data…");
    if is_running() {
        return Vec::new();
    }
    let category = if cache {
        Category::PodmanCache
    } else {
        Category::PodmanDisks
    };
    let paths = match candidates(&Roots::detect(), cache) {
        Ok(paths) => paths,
        Err(error) => {
            progress(&format!("Skipping Podman cleanup: {error}"));
            return Vec::new();
        }
    };
    let reason = if cache {
        "Downloaded Podman VM image; downloaded again when needed"
    } else {
        "No configured Podman machine references this disk; deletes all contained images, containers, and volumes"
    };
    paths
        .into_iter()
        .filter(|path| is_safe_to_delete(path))
        .filter_map(|path| {
            let size = allocated_size(&path);
            if size == 0 {
                return None;
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            let logical = path.metadata().map(|m| m.len()).unwrap_or(0);
            Some(FileGroup {
                key: format!("podman:{}", path.display()),
                category,
                title: name.clone(),
                description: format!(
                    "{} used of {} virtual · {}. {}",
                    format_bytes(size),
                    format_bytes(logical),
                    path.display(),
                    reason
                ),
                items: vec![FileItem::file(path, size, category, reason, name)],
            })
        })
        .collect()
}
impl Scanner for PodmanCacheScanner {
    fn name(&self) -> &'static str {
        "Podman Cache"
    }
    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        scan(true, progress)
    }
}
impl Scanner for PodmanDiskScanner {
    fn name(&self) -> &'static str {
        "Unused Podman VM Disks"
    }
    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        scan(false, progress)
    }
}
