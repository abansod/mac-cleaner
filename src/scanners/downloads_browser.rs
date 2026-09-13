use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{home_dir, list_children, safe_size};

use super::Scanner;

const INSTALLER_SUFFIXES: &[&str] = &[
    ".dmg", ".pkg", ".zip", ".tar", ".tar.gz", ".tgz", ".iso", ".app.zip",
];

pub struct DownloadsJunkScanner {
    min_age_days: u64,
    min_size: u64,
}

impl Default for DownloadsJunkScanner {
    fn default() -> Self {
        Self {
            min_age_days: 7,
            min_size: 1024 * 1024,
        }
    }
}

impl Scanner for DownloadsJunkScanner {
    fn name(&self) -> &'static str {
        "Installer & Download Junk"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Downloads for installers…");
        let downloads = home_dir().join("Downloads");
        if !downloads.exists() {
            return Vec::new();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now.saturating_sub(self.min_age_days * 86400);

        list_children(&downloads)
            .into_iter()
            .filter(|child| child.is_file())
            .filter_map(|child| {
                let name = child.file_name()?.to_string_lossy().to_lowercase();
                if !INSTALLER_SUFFIXES
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                {
                    return None;
                }
                let meta = child.metadata().ok()?;
                if meta.len() < self.min_size {
                    return None;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(now);
                if mtime > cutoff {
                    return None;
                }
                let age_days = now.saturating_sub(mtime) / 86400;
                let title = child
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(FileGroup {
                    key: format!("dl:{title}"),
                    category: Category::Downloads,
                    title: title.clone(),
                    description: format!("~/Downloads — {age_days} days old"),
                    items: vec![FileItem::file(
                        child,
                        meta.len(),
                        Category::Downloads,
                        format!("Installer/archive unused for {age_days} days"),
                        title,
                    )],
                })
            })
            .collect()
    }
}

pub struct BrowserCacheScanner;

impl Scanner for BrowserCacheScanner {
    fn name(&self) -> &'static str {
        "Browser Caches"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning browser caches…");
        let home = home_dir();
        let library = home.join("Library");
        let paths = [
            ("Safari", library.join("Caches").join("com.apple.Safari")),
            (
                "Chrome",
                library.join("Caches").join("Google").join("Chrome"),
            ),
            (
                "Chrome",
                library
                    .join("Application Support")
                    .join("Google")
                    .join("Chrome")
                    .join("Default")
                    .join("Code Cache"),
            ),
            (
                "Chrome",
                library
                    .join("Application Support")
                    .join("Google")
                    .join("Chrome")
                    .join("Default")
                    .join("Service Worker")
                    .join("CacheStorage"),
            ),
            ("Firefox", library.join("Caches").join("Firefox")),
            ("Edge", library.join("Caches").join("com.microsoft.edgemac")),
            (
                "Brave",
                library
                    .join("Caches")
                    .join("BraveSoftware")
                    .join("Brave-Browser"),
            ),
            (
                "Arc",
                library.join("Caches").join("company.thebrowser.Browser"),
            ),
        ];

        let mut seen = std::collections::HashSet::new();
        let mut groups = Vec::new();
        for (browser, path) in paths {
            let key = if path.exists() {
                path.canonicalize().unwrap_or_else(|_| path.clone())
            } else {
                path.clone()
            };
            if !seen.insert(key) || !path.exists() {
                continue;
            }
            let size = safe_size(&path);
            if size < 1024 * 10 {
                continue;
            }
            let leaf = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            groups.push(FileGroup {
                key: format!("browser:{browser}:{}", path.display()),
                category: Category::Browser,
                title: format!("{browser} — {leaf}"),
                description: format!("{browser} cache data (pages may reload slower once)"),
                items: vec![FileItem::file(
                    path,
                    size,
                    Category::Browser,
                    format!("{browser} cache"),
                    format!("{browser}:{leaf}"),
                )],
            });
        }
        groups
    }
}
