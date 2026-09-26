mod caches;
mod downloads_browser;
mod duplicates;
mod hidden_space;
mod login_items;
mod logs_trash;
pub(crate) mod orphaned;
mod xcode_mail_leftovers;

use std::path::PathBuf;

use crate::models::FileGroup;

pub trait Scanner: Send {
    fn name(&self) -> &'static str;
    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup>;
}

pub fn all_scanners() -> Vec<Box<dyn Scanner>> {
    vec![
        Box::new(caches::UserCacheScanner),
        Box::new(caches::SystemCacheScanner),
        Box::new(logs_trash::LogScanner),
        Box::new(caches::TempScanner),
        Box::new(logs_trash::TrashScanner),
        Box::new(downloads_browser::DownloadsJunkScanner::default()),
        Box::new(downloads_browser::BrowserCacheScanner),
        Box::new(xcode_mail_leftovers::XcodeScanner),
        Box::new(xcode_mail_leftovers::MailScanner),
        Box::new(hidden_space::LocalSnapshotScanner),
        Box::new(hidden_space::IosBackupScanner),
        Box::new(hidden_space::MessagesAttachmentScanner),
        Box::new(orphaned::OrphanedFilesScanner),
        Box::new(login_items::LoginItemsScanner {
            open_at_login: true,
        }),
        Box::new(duplicates::LargeOldScanner::default()),
        Box::new(duplicates::DuplicateScanner::default()),
        Box::new(duplicates::LanguageFileScanner),
    ]
}

pub fn smart_scanners() -> Vec<Box<dyn Scanner>> {
    vec![
        Box::new(caches::UserCacheScanner),
        Box::new(logs_trash::LogScanner),
        Box::new(caches::TempScanner),
        Box::new(logs_trash::TrashScanner),
        Box::new(downloads_browser::DownloadsJunkScanner::default()),
        Box::new(downloads_browser::BrowserCacheScanner),
        Box::new(xcode_mail_leftovers::XcodeScanner),
        Box::new(xcode_mail_leftovers::MailScanner),
        Box::new(hidden_space::LocalSnapshotScanner),
        Box::new(hidden_space::IosBackupScanner),
        Box::new(login_items::LoginItemsScanner {
            open_at_login: false,
        }),
    ]
}

pub fn duplicate_scanner(extra_root: Option<PathBuf>) -> duplicates::DuplicateScanner {
    duplicates::DuplicateScanner::with_extra_root(extra_root)
}

/// A duplicate scanner that walks only `roots`, skipping the default home folders.
pub fn duplicate_scanner_in(roots: Vec<PathBuf>) -> duplicates::DuplicateScanner {
    duplicates::DuplicateScanner::with_roots(roots)
}
