use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::PathBuf;

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
        self.items
            .iter()
            .filter(|item| item.exists())
            .map(|item| item.size)
            .sum()
    }

    pub fn count(&self) -> usize {
        self.items.iter().filter(|item| item.exists()).count()
    }

    pub fn prune(&mut self) {
        self.items.retain(FileItem::exists);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub groups: Vec<FileGroup>,
    pub warnings: Vec<String>,
    pub disk: Option<DiskPressure>,
}

impl ScanResult {
    pub fn total_size(&self) -> u64 {
        self.groups.iter().map(FileGroup::size).sum()
    }

    pub fn total_files(&self) -> usize {
        self.groups.iter().map(FileGroup::count).sum()
    }

    pub fn prune_empty(&mut self) {
        for group in &mut self.groups {
            group.prune();
        }
        self.groups.retain(|group| group.count() > 0);
        self.groups.sort_by_key(|b| Reverse(b.size()));
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

    pub fn groups_in(&self, category: Category) -> Vec<&FileGroup> {
        let mut groups: Vec<&FileGroup> = self
            .groups
            .iter()
            .filter(|group| group.category == category && group.count() > 0)
            .collect();
        groups.sort_by_key(|b| Reverse(b.size()));
        groups
    }

    pub fn by_category(&self) -> BTreeMap<Category, Vec<&FileGroup>> {
        let mut map: BTreeMap<Category, Vec<&FileGroup>> = BTreeMap::new();
        for group in &self.groups {
            if group.count() == 0 {
                continue;
            }
            map.entry(group.category).or_default().push(group);
        }
        for groups in map.values_mut() {
            groups.sort_by_key(|b| Reverse(b.size()));
        }
        map
    }

    pub fn categories_sorted(&self) -> Vec<Category> {
        let mut cats: Vec<(Category, u64)> = self
            .by_category()
            .into_iter()
            .map(|(cat, groups)| (cat, groups.iter().map(|g| g.size()).sum()))
            .collect();
        cats.sort_by_key(|b| Reverse(b.1));
        cats.into_iter().map(|(cat, _)| cat).collect()
    }
}
