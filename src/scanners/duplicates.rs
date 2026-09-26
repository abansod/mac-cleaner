use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{home_dir, list_children, safe_size};

use super::Scanner;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    ".Trash",
    "Library",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    "DerivedData",
];

const SKIP_SUFFIXES: &[&str] = &[".ds_store", ".localized", ".sock", ".pyc"];
const HASH_CHUNK: usize = 1024 * 1024;

pub struct DuplicateScanner {
    roots: Vec<PathBuf>,
    min_size: u64,
    max_files: usize,
}

impl Default for DuplicateScanner {
    fn default() -> Self {
        Self {
            roots: default_roots(),
            min_size: 100 * 1024,
            max_files: 50_000,
        }
    }
}

impl DuplicateScanner {
    pub fn with_extra_root(extra: Option<PathBuf>) -> Self {
        let mut scanner = Self::default();
        if let Some(path) = extra {
            scanner.roots.insert(0, path);
        }
        scanner
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Self::default()
        }
    }
}

impl Scanner for DuplicateScanner {
    fn name(&self) -> &'static str {
        "Duplicate Files"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Indexing files for duplicates…");
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        let mut count = 0usize;

        'outer: for root in &self.roots {
            if !root.exists() {
                continue;
            }
            progress(&format!("Walking {}…", root.display()));
            for path in walk_files(root) {
                let Ok(meta) = path.metadata() else {
                    continue;
                };
                if meta.len() < self.min_size {
                    continue;
                }
                by_size.entry(meta.len()).or_default().push(path);
                count += 1;
                if count >= self.max_files {
                    break 'outer;
                }
            }
        }

        let candidates: Vec<PathBuf> = by_size
            .into_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .flat_map(|(_, paths)| paths)
            .collect();
        progress(&format!("Hashing {} candidates…", candidates.len()));

        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for (i, path) in candidates.iter().enumerate() {
            if let Some(digest) = quick_hash(path) {
                by_hash.entry(digest).or_default().push(path.clone());
            }
            if i > 0 && i % 50 == 0 {
                progress(&format!("Hashed {i} files…"));
            }
        }

        let mut groups = Vec::new();
        let mut dup_idx = 0usize;
        for (digest, paths) in by_hash {
            let mut unique = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for path in paths {
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                if seen.insert(key) {
                    unique.push(path);
                }
            }
            if unique.len() < 2 {
                continue;
            }
            dup_idx += 1;
            let mut items = Vec::new();
            for path in unique {
                let Ok(meta) = path.metadata() else {
                    continue;
                };
                items.push(FileItem::file(
                    path,
                    meta.len(),
                    Category::Duplicates,
                    format!("Duplicate set #{dup_idx}"),
                    digest.chars().take(12).collect::<String>(),
                ));
            }
            if items.len() < 2 {
                continue;
            }
            let reclaimable = items.iter().skip(1).map(|i| i.size).sum::<u64>();
            let name = items[0]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| items[0].path.display().to_string());
            let short: String = digest.chars().take(12).collect();
            groups.push(FileGroup {
                key: format!("dup:{}", digest.chars().take(16).collect::<String>()),
                category: Category::Duplicates,
                title: format!("{name} ×{}", items.len()),
                description: format!(
                    "Identical files — reclaim ~{} by keeping one. Hash {short}…",
                    crate::safety::format_bytes(reclaimable)
                ),
                items,
            });
        }
        groups.sort_by_key(|b| Reverse(b.size()));
        groups
    }
}

pub struct LargeOldScanner {
    roots: Vec<PathBuf>,
    min_size: u64,
    min_age_days: u64,
    limit: usize,
}

impl Default for LargeOldScanner {
    fn default() -> Self {
        Self {
            roots: default_roots(),
            min_size: 50 * 1024 * 1024,
            min_age_days: 90,
            limit: 100,
        }
    }
}

impl Scanner for LargeOldScanner {
    fn name(&self) -> &'static str {
        "Large & Old Files"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning large & old files…");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now.saturating_sub(self.min_age_days * 86400);
        let mut found = Vec::new();

        for root in &self.roots {
            if !root.exists() {
                continue;
            }
            progress(&format!("Scanning {}…", root.display()));
            for path in walk_files(root) {
                let Ok(meta) = path.metadata() else {
                    continue;
                };
                if meta.len() < self.min_size {
                    continue;
                }
                let age_ref = meta
                    .accessed()
                    .or_else(|_| meta.modified())
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(now);
                if age_ref > cutoff {
                    continue;
                }
                let age_days = now.saturating_sub(age_ref) / 86400;
                found.push(FileItem::file(
                    path.clone(),
                    meta.len(),
                    Category::LargeOld,
                    format!("≥50MB, untouched ~{age_days}d"),
                    path.display().to_string(),
                ));
            }
        }

        found.sort_by_key(|b| Reverse(b.size));
        found.truncate(self.limit);
        found
            .into_iter()
            .map(|item| {
                let parent = item
                    .path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let title = item
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| item.path.display().to_string());
                FileGroup {
                    key: format!("large:{}", item.path.display()),
                    category: Category::LargeOld,
                    title,
                    description: format!("{parent} — {}", item.reason),
                    items: vec![item],
                }
            })
            .collect()
    }
}

const KEEP_LANGS: &[&str] = &["en", "en_us", "en-us", "base", "english"];

pub struct LanguageFileScanner;

impl Scanner for LanguageFileScanner {
    fn name(&self) -> &'static str {
        "Unused Language Files"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning language files…");
        let apps = home_dir().join("Applications");
        if !apps.exists() {
            return Vec::new();
        }

        let mut groups = Vec::new();
        for app in list_children(&apps) {
            if app.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let resources = app.join("Contents").join("Resources");
            if !resources.exists() {
                continue;
            }
            let lprojs: Vec<PathBuf> = list_children(&resources)
                .into_iter()
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("lproj"))
                .filter(|p| {
                    let stem = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    !KEEP_LANGS.contains(&stem.as_str())
                })
                .collect();
            if lprojs.is_empty() {
                continue;
            }
            let app_name = file_name(&app);
            let mut items = Vec::new();
            for path in lprojs {
                let size = safe_size(&path);
                if size < 100 {
                    continue;
                }
                items.push(FileItem::file(
                    path,
                    size,
                    Category::Language,
                    format!("Localization in {app_name}"),
                    app_name.clone(),
                ));
            }
            if items.is_empty() {
                continue;
            }
            groups.push(FileGroup {
                key: format!("lang:{app_name}"),
                category: Category::Language,
                title: app_name,
                description: "Non-English localization bundles in user Applications".into(),
                items,
            });
        }
        groups
    }
}

fn default_roots() -> Vec<PathBuf> {
    let home = home_dir();
    [
        "Downloads",
        "Documents",
        "Desktop",
        "Pictures",
        "Movies",
        "Music",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect()
}

fn walk_files(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.')
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            !name.starts_with('.') && !SKIP_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        })
}

fn quick_hash(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut hasher = Sha256::new();
    hasher.update(size.to_string().as_bytes());
    if size <= (HASH_CHUNK as u64) * 2 {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        hasher.update(&buf);
    } else {
        let mut buf = vec![0u8; HASH_CHUNK];
        file.read_exact(&mut buf).ok()?;
        hasher.update(&buf);
        file.seek(SeekFrom::End(-(HASH_CHUNK as i64))).ok()?;
        file.read_exact(&mut buf).ok()?;
        hasher.update(&buf);
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
