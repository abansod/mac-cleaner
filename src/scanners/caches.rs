use std::path::{Path, PathBuf};

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{is_writable, library_dir, list_children, safe_size};

use super::Scanner;

const SKIP_CACHE_NAMES: &[&str] = &[".DS_Store", "CloudKit"];

pub struct UserCacheScanner;

impl Scanner for UserCacheScanner {
    fn name(&self) -> &'static str {
        "User Caches"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning user caches…");
        let caches = library_dir().join("Caches");
        if !caches.exists() {
            return Vec::new();
        }

        list_children(&caches)
            .into_iter()
            .filter(|child| {
                child
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| !SKIP_CACHE_NAMES.contains(&name))
            })
            .filter_map(|child| {
                let size = safe_size(&child);
                if size < 1024 {
                    return None;
                }
                let name = file_name(&child);
                Some(single_group(
                    format!("user-cache:{name}"),
                    Category::UserCache,
                    name.clone(),
                    "~/Library/Caches — safe to remove; apps rebuild as needed",
                    child,
                    size,
                    "User cache data that apps can regenerate",
                    name,
                ))
            })
            .collect()
    }
}

pub struct SystemCacheScanner;

impl Scanner for SystemCacheScanner {
    fn name(&self) -> &'static str {
        "System Caches"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning system caches…");
        let path = PathBuf::from("/Library/Caches");
        if !path.exists() {
            return Vec::new();
        }

        list_children(&path)
            .into_iter()
            .filter(|child| child.is_dir() && is_writable(child))
            .filter_map(|child| {
                let size = safe_size(&child);
                if size < 1024 * 100 {
                    return None;
                }
                let name = file_name(&child);
                Some(single_group(
                    format!("sys-cache:{name}"),
                    Category::SystemCache,
                    name.clone(),
                    "/Library/Caches — only writable caches are listed",
                    child,
                    size,
                    "Shared system/app cache",
                    name,
                ))
            })
            .collect()
    }
}

pub struct TempScanner;

impl Scanner for TempScanner {
    fn name(&self) -> &'static str {
        "Temporary Files"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning temporary files…");
        let mut candidates = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/private/tmp"),
            PathBuf::from("/private/var/tmp"),
        ];
        if let Some(tmpdir) = std::env::var_os("TMPDIR") {
            candidates.push(PathBuf::from(tmpdir));
        }

        let mut seen = std::collections::HashSet::new();
        let mut groups = Vec::new();
        for base in candidates {
            let key = base.canonicalize().unwrap_or_else(|_| base.clone());
            if !seen.insert(key) || !base.exists() {
                continue;
            }
            for child in list_children(&base) {
                if !is_writable(&child) {
                    continue;
                }
                if !child.is_file() && !child.is_dir() {
                    continue;
                }
                let size = safe_size(&child);
                if size < 4096 {
                    continue;
                }
                let name = file_name(&child);
                groups.push(single_group(
                    format!("temp:{}", child.display()),
                    Category::Temp,
                    name,
                    format!("Temporary item in {}", base.display()),
                    child.clone(),
                    size,
                    "Temporary file",
                    child.display().to_string(),
                ));
            }
        }
        groups
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[allow(clippy::too_many_arguments)]
fn single_group(
    key: String,
    category: Category,
    title: String,
    description: impl Into<String>,
    path: PathBuf,
    size: u64,
    reason: &str,
    group_key: String,
) -> FileGroup {
    FileGroup {
        key,
        category,
        title,
        description: description.into(),
        items: vec![FileItem {
            path,
            size,
            category,
            reason: reason.to_string(),
            group_key,
        }],
    }
}
