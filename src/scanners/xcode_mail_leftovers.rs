use std::path::{Path, PathBuf};

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{home_dir, library_dir, list_children, safe_size};

use super::Scanner;

pub struct XcodeScanner;

impl Scanner for XcodeScanner {
    fn name(&self) -> &'static str {
        "Xcode Junk"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Xcode junk…");
        let home = home_dir();
        let developer = home.join("Library").join("Developer");
        let caches = home.join("Library").join("Caches");
        let targets: [(&str, PathBuf, &str); 10] = [
            (
                "DerivedData",
                developer.join("Xcode").join("DerivedData"),
                "Build artifacts — safe to delete",
            ),
            (
                "Archives",
                developer.join("Xcode").join("Archives"),
                "Old Xcode archives",
            ),
            (
                "iOS DeviceSupport",
                developer.join("Xcode").join("iOS DeviceSupport"),
                "Device symbols",
            ),
            (
                "watchOS DeviceSupport",
                developer.join("Xcode").join("watchOS DeviceSupport"),
                "watchOS symbols",
            ),
            (
                "CoreSimulator Caches",
                developer.join("CoreSimulator").join("Caches"),
                "Simulator caches",
            ),
            (
                "CoreSimulator Devices",
                developer.join("CoreSimulator").join("Devices"),
                "Simulator devices (large)",
            ),
            (
                "Xcode Caches",
                caches.join("com.apple.dt.Xcode"),
                "Xcode app cache",
            ),
            (
                "SwiftPM",
                caches.join("org.swift.swiftpm"),
                "Swift package cache",
            ),
            ("CocoaPods", caches.join("CocoaPods"), "CocoaPods cache"),
            (
                "Carthage",
                caches.join("org.carthage.CarthageKit"),
                "Carthage cache",
            ),
        ];

        let mut groups = Vec::new();
        for (title, path, desc) in targets {
            if !path.exists() {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("Devices") && path.is_dir() {
                for child in list_children(&path) {
                    let size = safe_size(&child);
                    if size < 1024 * 1024 {
                        continue;
                    }
                    let name = file_name(&child);
                    let short = if name.len() > 8 {
                        format!("{}…", &name[..8])
                    } else {
                        name.clone()
                    };
                    groups.push(leaf_group(
                        format!("xcode-sim:{name}"),
                        Category::Xcode,
                        format!("Simulator — {short}"),
                        desc,
                        child,
                        size,
                        name,
                    ));
                }
                continue;
            }
            if title.contains("DerivedData") && path.is_dir() {
                for child in list_children(&path) {
                    let size = safe_size(&child);
                    if size < 1024 * 100 {
                        continue;
                    }
                    let name = file_name(&child);
                    groups.push(leaf_group(
                        format!("xcode-dd:{name}"),
                        Category::Xcode,
                        format!("DerivedData — {name}"),
                        desc,
                        child,
                        size,
                        name,
                    ));
                }
                continue;
            }
            let size = safe_size(&path);
            if size < 1024 * 50 {
                continue;
            }
            groups.push(leaf_group(
                format!("xcode:{title}"),
                Category::Xcode,
                title.to_string(),
                desc,
                path,
                size,
                title.to_string(),
            ));
        }
        groups
    }
}

pub struct MailScanner;

impl Scanner for MailScanner {
    fn name(&self) -> &'static str {
        "Mail Downloads"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Mail downloads…");
        let paths = [
            library_dir()
                .join("Containers")
                .join("com.apple.mail")
                .join("Data")
                .join("Library")
                .join("Mail Downloads"),
            home_dir().join("Library").join("Mail Downloads"),
        ];

        let mut groups = Vec::new();
        for path in paths {
            if !path.exists() {
                continue;
            }
            for child in list_children(&path) {
                let size = safe_size(&child);
                if size < 1024 {
                    continue;
                }
                let name = file_name(&child);
                groups.push(leaf_group(
                    format!("mail:{}:{name}", path.display()),
                    Category::Mail,
                    name.clone(),
                    "Downloaded Mail attachment",
                    child,
                    size,
                    name,
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

fn leaf_group(
    key: String,
    category: Category,
    title: String,
    description: &str,
    path: PathBuf,
    size: u64,
    group_key: String,
) -> FileGroup {
    FileGroup {
        key,
        category,
        title,
        description: description.to_string(),
        items: vec![FileItem::file(path, size, category, description, group_key)],
    }
}
