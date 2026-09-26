use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rustix::fs::{self as rfs, Access, AtFlags, FileType, Mode, OFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    SystemCache,
    UserCache,
    Logs,
    Temp,
    Trash,
    Downloads,
    Browser,
    Xcode,
    Mail,
    OrphanedFiles,
    LoginItems,
    LargeOld,
    Duplicates,
    Language,
    LocalSnapshots,
    IosBackups,
    MessagesAttachments,
}

impl Category {
    pub const ALL: [Category; 17] = [
        Category::SystemCache,
        Category::UserCache,
        Category::Logs,
        Category::Temp,
        Category::Trash,
        Category::Downloads,
        Category::Browser,
        Category::Xcode,
        Category::Mail,
        Category::OrphanedFiles,
        Category::LoginItems,
        Category::LargeOld,
        Category::Duplicates,
        Category::Language,
        Category::LocalSnapshots,
        Category::IosBackups,
        Category::MessagesAttachments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::SystemCache => "System Caches",
            Category::UserCache => "User Caches",
            Category::Logs => "Logs",
            Category::Temp => "Temporary Files",
            Category::Trash => "Trash",
            Category::Downloads => "Installer & Download Junk",
            Category::Browser => "Browser Caches",
            Category::Xcode => "Xcode Junk",
            Category::Mail => "Mail Downloads",
            Category::OrphanedFiles => "Orphaned Files",
            Category::LoginItems => "Login Items",
            Category::LargeOld => "Large & Old Files",
            Category::Duplicates => "Duplicate Files",
            Category::Language => "Unused Language Files",
            Category::LocalSnapshots => "Time Machine Snapshots",
            Category::IosBackups => "iOS Device Backups",
            Category::MessagesAttachments => "Messages Attachments",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Category::SystemCache => {
                "Writable caches in /Library/Caches. Only items you can delete are listed."
            }
            Category::UserCache => {
                "App caches in ~/Library/Caches. Safe to remove; apps rebuild them as needed."
            }
            Category::Logs => "Application and system log folders you can write to.",
            Category::Temp => "Temporary files in /tmp and your user TMPDIR.",
            Category::Trash => "Items in ~/.Trash — permanently deleted, not recoverable from Trash.",
            Category::Downloads => {
                "Old DMG/PKG/ZIP installers in ~/Downloads (at least 7 days old)."
            }
            Category::Browser => {
                "Safari, Chrome, Firefox, Edge, Brave, and Arc cache data. Pages may reload slower once."
            }
            Category::Xcode => {
                "DerivedData, simulators, SwiftPM, CocoaPods, and other Xcode build junk."
            }
            Category::Mail => "Attachments Mail downloaded for preview.",
            Category::OrphanedFiles => {
                "Prefs, containers, caches, and other Library files for apps that no longer look installed. Verify first."
            }
            Category::LoginItems => {
                "Background launch agents/daemons and Open at Login entries left by uninstalled apps. Apple items are never listed."
            }
            Category::LargeOld => {
                "Files ≥50 MB that have not been touched in about 90 days."
            }
            Category::Duplicates => {
                "Identical files by size and content hash. Keep one copy; delete the rest."
            }
            Category::Language => {
                "Non-English .lproj bundles inside apps in ~/Applications."
            }
            Category::LocalSnapshots => {
                "Time Machine local snapshots via tmutil. The sealed macOS boot snapshot is never listed."
            }
            Category::IosBackups => {
                "Complete Finder/iTunes device backups. Removes the backup on this Mac only — not the phone, not macOS."
            }
            Category::MessagesAttachments => {
                "Old files under ~/Library/Messages/Attachments. chat.db and other Messages databases are never touched."
            }
        }
    }
}

/// How an item is reclaimed. Path delete is never used for APFS snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimOp {
    DeletePath,
    /// `tmutil deletelocalsnapshots <date>` — date must be `YYYY-MM-DD-HHMMSS`.
    TmLocalSnapshot {
        date: String,
    },
    /// `launchctl bootout` then remove the plist at `path` (and its privileged helper).
    LaunchJob {
        label: String,
        helper: Option<PathBuf>,
    },
    /// Remove an entry from the System Events "Open at Login" list.
    OpenAtLogin {
        name: String,
    },
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub path: PathBuf,
    pub size: u64,
    #[allow(dead_code)]
    pub category: Category,
    pub reason: String,
    #[allow(dead_code)]
    pub group_key: String,
    pub op: ReclaimOp,
}

impl FileItem {
    pub fn file(
        path: PathBuf,
        size: u64,
        category: Category,
        reason: impl Into<String>,
        group_key: impl Into<String>,
    ) -> Self {
        Self {
            path,
            size,
            category,
            reason: reason.into(),
            group_key: group_key.into(),
            op: ReclaimOp::DeletePath,
        }
    }

    pub fn exists(&self) -> bool {
        match &self.op {
            ReclaimOp::DeletePath => self.path.exists(),
            ReclaimOp::LaunchJob { .. } => self.path.symlink_metadata().is_ok(),
            // Dropped from the scan result after a successful removal call.
            ReclaimOp::TmLocalSnapshot { .. } | ReclaimOp::OpenAtLogin { .. } => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskPressure {
    pub mount: String,
    pub container_bytes: u64,
    pub container_free: u64,
}

#[derive(Debug, Clone)]
pub struct FileGroup {
    pub key: String,
    pub category: Category,
    pub title: String,
    pub description: String,
    pub items: Vec<FileItem>,
}

impl FileGroup {
    pub fn size(&self) -> u64 {
        self.stats().1
    }

    pub fn count(&self) -> usize {
        self.stats().0
    }

    /// Live `(count, size)` of items still on disk, checked in one pass.
    pub fn stats(&self) -> (usize, u64) {
        let mut runs = Vec::new();
        let mut start = 0;
        for i in 1..=self.items.len() {
            if i == self.items.len()
                || fast_parent(&self.items[i]) != fast_parent(&self.items[start])
            {
                runs.push(&self.items[start..i]);
                start = i;
            }
        }
        crate::fs_pool().install(|| {
            runs.into_par_iter()
                .map(|run| {
                    if run.len() < MIN_SHARED_PARENT || fast_parent(&run[0]).is_none() {
                        return live_stats(run.iter().filter(|item| item.exists()));
                    }
                    live_stats_in_shared_dir(run)
                })
                .reduce(|| (0, 0), add_stats)
        })
    }

    pub fn prune(&mut self) {
        self.items.retain(FileItem::exists);
    }
}

/// Runs shorter than this stat full paths; opening the parent would cost more than it saves.
const MIN_SHARED_PARENT: usize = 3;
/// Directory entries read per wanted name before giving up on the listing and stat-ing instead.
const LISTING_ENTRIES_PER_ITEM: usize = 16;

fn add_stats(a: (usize, u64), b: (usize, u64)) -> (usize, u64) {
    (a.0 + b.0, a.1 + b.1)
}

fn live_stats<'a>(items: impl Iterator<Item = &'a FileItem>) -> (usize, u64) {
    items.fold((0, 0), |acc, item| add_stats(acc, (1, item.size)))
}

/// Parent directory bytes of the item's path, when its name can be checked relative to it.
fn fast_parent(item: &FileItem) -> Option<&[u8]> {
    split_parent(&item.path).map(|(parent, _)| parent)
}

fn split_parent(path: &Path) -> Option<(&[u8], &[u8])> {
    let bytes = path.as_os_str().as_bytes();
    let slash = bytes.iter().rposition(|&b| b == b'/')?;
    let (parent, name) = (&bytes[..slash], &bytes[slash + 1..]);
    if name.is_empty() || name == b"." || name == b".." {
        return None;
    }
    Some((if parent.is_empty() { b"/" } else { parent }, name))
}

/// Existence check for items that all share one parent directory, opened once.
///
/// Names seen in the directory listing as non-symlinks exist. Anything else (not listed, a
/// symlink, or a name that differs only by case or Unicode normalization) is checked with
/// `accessat`, so the result matches `Path::exists`.
fn live_stats_in_shared_dir(items: &[FileItem]) -> (usize, u64) {
    let opened = items.first().and_then(fast_parent).and_then(|parent| {
        let fd = rfs::open(
            OsStr::from_bytes(parent),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()?;
        let dir = rfs::Dir::read_from(&fd).ok()?;
        Some((fd, dir))
    });
    let Some((fd, mut dir)) = opened else {
        return live_stats(items.iter().filter(|item| item.exists()));
    };

    let mut listed: HashMap<&[u8], bool> = items
        .iter()
        .filter(|item| item.op == ReclaimOp::DeletePath)
        .filter_map(|item| split_parent(&item.path).map(|(_, name)| (name, false)))
        .collect();
    let mut remaining = listed.len();
    let mut budget = 64 + LISTING_ENTRIES_PER_ITEM * listed.len();
    while remaining > 0 && budget > 0 {
        let Some(Ok(entry)) = dir.read() else { break };
        budget -= 1;
        let file_type = entry.file_type();
        if file_type == FileType::Symlink || file_type == FileType::Unknown {
            continue;
        }
        if let Some(seen) = listed.get_mut(entry.file_name().to_bytes()) {
            if !*seen {
                *seen = true;
                remaining -= 1;
            }
        }
    }

    drop(dir);
    live_stats(items.iter().filter(|item| {
        match item.op {
            ReclaimOp::DeletePath => match split_parent(&item.path) {
                Some((_, name)) => {
                    listed.get(name).copied().unwrap_or(false)
                        || rfs::accessat(
                            &fd,
                            OsStr::from_bytes(name),
                            Access::EXISTS,
                            AtFlags::empty(),
                        )
                        .is_ok()
                }
                None => item.exists(),
            },
            _ => item.exists(),
        }
    }))
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub groups: Vec<FileGroup>,
    pub warnings: Vec<String>,
    pub disk: Option<DiskPressure>,
}

impl ScanResult {
    pub fn total_size(&self) -> u64 {
        crate::fs_pool().install(|| self.groups.par_iter().map(FileGroup::size).sum())
    }

    pub fn total_files(&self) -> usize {
        crate::fs_pool().install(|| self.groups.par_iter().map(FileGroup::count).sum())
    }

    pub fn prune_empty(&mut self) {
        let groups = &mut self.groups;
        crate::fs_pool().install(|| groups.par_iter_mut().for_each(FileGroup::prune));
        self.groups.retain(|group| !group.items.is_empty());
        self.groups
            .sort_by_key(|b| Reverse(b.items.iter().map(|i| i.size).sum::<u64>()));
    }

    pub fn forget_paths(&mut self, paths: &[PathBuf]) {
        let gone: std::collections::HashSet<&PathBuf> = paths.iter().collect();
        if gone.is_empty() {
            return;
        }
        for group in &mut self.groups {
            group.items.retain(|item| !gone.contains(&item.path));
        }
    }

    pub fn group(&self, key: &str) -> Option<&FileGroup> {
        self.groups.iter().find(|group| group.key == key)
    }

    /// Non-empty groups matching `keep`, with their live size, largest first.
    fn live_groups(&self, keep: impl Fn(&FileGroup) -> bool + Sync) -> Vec<(&FileGroup, u64)> {
        let mut groups: Vec<(&FileGroup, u64)> = crate::fs_pool().install(|| {
            self.groups
                .par_iter()
                .filter(|group| keep(group))
                .filter_map(|group| match group.stats() {
                    (0, _) => None,
                    (_, size) => Some((group, size)),
                })
                .collect()
        });
        groups.sort_by_key(|&(_, size)| Reverse(size));
        groups
    }

    pub fn groups_in(&self, category: Category) -> Vec<&FileGroup> {
        self.live_groups(|group| group.category == category)
            .into_iter()
            .map(|(group, _)| group)
            .collect()
    }

    pub fn by_category(&self) -> BTreeMap<Category, Vec<&FileGroup>> {
        let mut map: BTreeMap<Category, Vec<&FileGroup>> = BTreeMap::new();
        for (group, _) in self.live_groups(|_| true) {
            map.entry(group.category).or_default().push(group);
        }
        map
    }

    pub fn categories_sorted(&self) -> Vec<Category> {
        let mut totals: BTreeMap<Category, u64> = BTreeMap::new();
        for (group, size) in self.live_groups(|_| true) {
            *totals.entry(group.category).or_default() += size;
        }
        let mut cats: Vec<(Category, u64)> = totals.into_iter().collect();
        cats.sort_by_key(|b| Reverse(b.1));
        cats.into_iter().map(|(cat, _)| cat).collect()
    }
}
