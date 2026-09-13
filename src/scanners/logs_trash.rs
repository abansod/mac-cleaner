use std::path::PathBuf;

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{home_dir, is_writable, library_dir, list_children, safe_size};

use super::Scanner;

pub struct LogScanner;

impl Scanner for LogScanner {
    fn name(&self) -> &'static str {
        "Logs"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning logs…");
        let roots = [
            library_dir().join("Logs"),
            home_dir().join("Library").join("Logs"),
            PathBuf::from("/Library/Logs"),
            PathBuf::from("/private/var/log"),
        ];

        let mut seen = std::collections::HashSet::new();
        let mut groups = Vec::new();
        for root in roots {
            let key = root.canonicalize().unwrap_or_else(|_| root.clone());
            if !seen.insert(key) || !root.exists() || !root.is_dir() {
                continue;
            }
            for child in list_children(&root) {
                let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let name = name.to_string();
                let size = safe_size(&child);
                if size < 1024 || !is_writable(&child) {
                    continue;
                }
                groups.push(FileGroup {
                    key: format!("log:{}:{name}", root.display()),
                    category: Category::Logs,
                    title: name.clone(),
                    description: format!("Logs in {}", root.display()),
                    items: vec![FileItem::file(
                        child,
                        size,
                        Category::Logs,
                        "Log files",
                        name,
                    )],
                });
            }
        }
        groups
    }
}

pub struct TrashScanner;

impl Scanner for TrashScanner {
    fn name(&self) -> &'static str {
        "Trash"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Trash…");
        let trash = home_dir().join(".Trash");
        if !trash.exists() {
            return Vec::new();
        }

        list_children(&trash)
            .into_iter()
            .filter_map(|child| {
                let size = safe_size(&child);
                if size == 0 && !child.exists() {
                    return None;
                }
                let name = child
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(FileGroup {
                    key: format!("trash:{name}"),
                    category: Category::Trash,
                    title: name.clone(),
                    description: "~/Trash — permanently delete".into(),
                    items: vec![FileItem::file(
                        child,
                        size,
                        Category::Trash,
                        "Item in Trash",
                        name,
                    )],
                })
            })
            .collect()
    }
}
