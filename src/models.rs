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
    Leftovers,
    LargeOld,
    Duplicates,
    Language,
}

impl Category {
    pub const ALL: [Category; 13] = [
        Category::SystemCache,
        Category::UserCache,
        Category::Logs,
        Category::Temp,
        Category::Trash,
        Category::Downloads,
        Category::Browser,
        Category::Xcode,
        Category::Mail,
        Category::Leftovers,
        Category::LargeOld,
        Category::Duplicates,
        Category::Language,
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
            Category::Leftovers => "App Leftovers",
            Category::LargeOld => "Large & Old Files",
            Category::Duplicates => "Duplicate Files",
            Category::Language => "Unused Language Files",
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
            Category::Leftovers => {
                "Application Support / saved state for apps that no longer look installed. Verify first."
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
        }
    }
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
}

impl FileItem {
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
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
