use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{Category, FileGroup, FileItem};
use crate::safety::{home_dir, is_safe_to_delete, library_dir, list_children, safe_size};

use super::Scanner;

/// Folders that belong to macOS itself, not leftover third-party apps.
const SKIP_NAMES: &[&str] = &[
    ".DS_Store",
    ".GlobalPreferences",
    "AddressBook",
    "Apple",
    "Application Scripts",
    "Application Support",
    "Caches",
    "CallHistoryDB",
    "CallHistoryTransactions",
    "CloudDocs",
    "com.apple.sharedfilelist",
    "Containers",
    "Cookies",
    "CrashReporter",
    "DiagnosticReports",
    "DifferentialPrivacy",
    "DiskImages",
    "FaceTime",
    "FileProvider",
    "Group Containers",
    "HomeKit",
    "iCloud",
    "IdentityServices",
    "IntelligencePlatform",
    "Knowledge",
    "Logs",
    "Mobile Documents",
    "MobileSync",
    "Network",
    "PassKit",
    "Preferences",
    "Saved Application State",
    "Suggestions",
    "SyncServices",
    "TCC",
    "Translation",
    "Weather",
];

const SKIP_PREFIXES: &[&str] = &["com.apple.", "apple.", "group.com.apple.", "group.apple."];

const GENERIC_BUNDLE_TAILS: &[&str] = &[
    "agent", "app", "client", "daemon", "desktop", "electron", "helper", "launcher", "mac",
    "macos", "updater", "webapp",
];

/// Library locations that commonly keep data after an app is uninstalled.
fn orphan_roots(lib: &Path) -> Vec<(&'static str, PathBuf, u64)> {
    vec![
        ("Preferences", lib.join("Preferences"), 0),
        (
            "Preferences/ByHost",
            lib.join("Preferences").join("ByHost"),
            0,
        ),
        ("LaunchAgents", lib.join("LaunchAgents"), 0),
        ("LaunchDaemons", lib.join("LaunchDaemons"), 0),
        ("Application Scripts", lib.join("Application Scripts"), 0),
        (
            "Application Support",
            lib.join("Application Support"),
            50 * 1024,
        ),
        ("Containers", lib.join("Containers"), 50 * 1024),
        ("Group Containers", lib.join("Group Containers"), 50 * 1024),
        ("Caches", lib.join("Caches"), 50 * 1024),
        (
            "Saved Application State",
            lib.join("Saved Application State"),
            1024,
        ),
        ("Logs", lib.join("Logs"), 10 * 1024),
        ("HTTPStorages", lib.join("HTTPStorages"), 10 * 1024),
        ("WebKit", lib.join("WebKit"), 50 * 1024),
        ("Cookies", lib.join("Cookies"), 1024),
        ("Internet Plug-Ins", lib.join("Internet Plug-Ins"), 0),
        ("QuickLook", lib.join("QuickLook"), 0),
        ("Services", lib.join("Services"), 0),
        ("PreferencePanes", lib.join("PreferencePanes"), 0),
        ("Input Methods", lib.join("Input Methods"), 0),
        ("Screen Savers", lib.join("Screen Savers"), 0),
        ("Widgets", lib.join("Widgets"), 0),
        ("Autosave Information", lib.join("Autosave Information"), 0),
        ("ColorPickers", lib.join("ColorPickers"), 0),
        (
            "Contextual Menu Items",
            lib.join("Contextual Menu Items"),
            0,
        ),
        ("Sounds", lib.join("Sounds"), 0),
        ("Receipts", lib.join("Receipts"), 0),
    ]
}

pub struct OrphanedFilesScanner;

impl Scanner for OrphanedFilesScanner {
    fn name(&self) -> &'static str {
        "Orphaned Files"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning orphaned files…");
        let installed = installed_apps();
        let lib = library_dir();
        let mut buckets: BTreeMap<String, FileGroup> = BTreeMap::new();

        for (location, root, min_size) in orphan_roots(&lib) {
            progress(&format!("Scanning leftover {location}…"));
            if !root.exists() {
                continue;
            }
            for child in list_children(&root) {
                consider_child(&mut buckets, &installed, location, child, min_size);
            }
        }

        buckets.into_values().collect()
    }
}

fn consider_child(
    buckets: &mut BTreeMap<String, FileGroup>,
    installed: &InstalledApps,
    location: &str,
    child: PathBuf,
    min_size: u64,
) {
    let raw = file_name(&child);
    if raw.starts_with('.') || SKIP_NAMES.contains(&raw.as_str()) {
        return;
    }
    let ident = normalize_ident(&raw);
    if ident.is_empty() || should_skip_ident(&ident) {
        return;
    }
    if likely_installed(&ident, installed) {
        return;
    }
    if !is_safe_to_delete(&child) {
        return;
    }
    let size = safe_size(&child);
    if size < min_size {
        return;
    }

    let key = family_key(&ident);
    let reason = format!("Leftover in ~/Library/{location}");
    let item = FileItem::file(child, size, Category::OrphanedFiles, reason, key.clone());
    let group = buckets.entry(key.clone()).or_insert_with(|| FileGroup {
        key: format!("orphan:{key}"),
        category: Category::OrphanedFiles,
        title: display_title(&ident, &raw),
        description: "Files left behind after an app was uninstalled. Verify before deleting."
            .into(),
        items: Vec::new(),
    });
    if group.title.contains('.') && !raw.contains('.') {
        group.title = display_title(&ident, &raw);
    }
    group.items.push(item);
}

#[derive(Default)]
struct InstalledApps {
    tokens: HashSet<String>,
}

fn installed_apps() -> InstalledApps {
    let mut tokens = HashSet::new();
    let roots = [
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
        home_dir().join("Applications"),
    ];
    for apps_dir in roots {
        collect_apps(&apps_dir, &mut tokens, 0);
    }
    InstalledApps { tokens }
}

fn collect_apps(dir: &Path, tokens: &mut HashSet<String>, depth: u8) {
    if depth > 2 || !dir.exists() {
        return;
    }
    for child in list_children(dir) {
        if child.extension().and_then(|e| e.to_str()) == Some("app") {
            remember_app(&child, tokens);
            continue;
        }
        if child.is_dir() {
            collect_apps(&child, tokens, depth + 1);
        }
    }
}

fn remember_app(app: &Path, tokens: &mut HashSet<String>) {
    if let Some(stem) = app.file_stem().and_then(|s| s.to_str()) {
        insert_token(tokens, stem);
    }
    if let Some(name) = app.file_name().and_then(|s| s.to_str()) {
        insert_token(tokens, name);
    }
    if let Some(bundle_id) = bundle_id(app) {
        insert_token(tokens, &bundle_id);
        for part in bundle_id.split('.') {
            if part.len() >= 4 && !GENERIC_BUNDLE_TAILS.contains(&part) {
                insert_token(tokens, part);
            }
        }
    }
}

fn insert_token(tokens: &mut HashSet<String>, raw: &str) {
    let n = raw.trim().to_lowercase();
    if n.is_empty() {
        return;
    }
    tokens.insert(n);
}

fn bundle_id(app: &Path) -> Option<String> {
    let info = app.join("Contents").join("Info.plist");
    if !info.exists() {
        return None;
    }
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(&info)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    (!id.is_empty()).then_some(id)
}

fn normalize_ident(name: &str) -> String {
    let mut n = name.trim().to_lowercase();
    for suffix in [
        ".plist",
        ".savedstate",
        ".binarycookies",
        ".plugin",
        ".qlgenerator",
        ".service",
        ".prefpane",
        ".saver",
        ".wdgt",
        ".bundle",
        ".app",
    ] {
        if let Some(stripped) = n.strip_suffix(suffix) {
            n = stripped.to_string();
        }
    }
    // ByHost: com.vendor.app.<hardware-uuid>
    if let Some((head, tail)) = n.rsplit_once('.') {
        let uuidish = tail.len() >= 8 && tail.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if uuidish {
            n = head.to_string();
        }
    }
    n
}

fn should_skip_ident(ident: &str) -> bool {
    if ident.starts_with('.') {
        return true;
    }
    SKIP_PREFIXES.iter().any(|p| ident.starts_with(p))
        || ident == "apple"
        || ident.starts_with("apple") && ident.contains('.')
}

fn family_key(ident: &str) -> String {
    let n = normalize_ident(ident);
    if !n.contains('.') {
        return n;
    }
    let parts: Vec<&str> = n.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return n;
    }
    let mut i = parts.len() - 1;
    while i > 0 && (GENERIC_BUNDLE_TAILS.contains(&parts[i]) || parts[i].len() < 3) {
        i -= 1;
    }
    if parts[i].len() >= 3 {
        parts[i].to_string()
    } else {
        n
    }
}

fn likely_installed(folder_name: &str, installed: &InstalledApps) -> bool {
    likely_installed_tokens(folder_name, &installed.tokens)
}

fn likely_installed_tokens(folder_name: &str, tokens: &HashSet<String>) -> bool {
    let n = normalize_ident(folder_name);
    if should_skip_ident(&n) {
        return true;
    }
    if tokens.contains(&n) {
        return true;
    }
    let key = family_key(&n);
    if key.len() >= 3 && tokens.contains(&key) {
        return true;
    }
    if n.contains('.') {
        for part in n.split('.') {
            if part.len() >= 4 && !GENERIC_BUNDLE_TAILS.contains(&part) && tokens.contains(part) {
                return true;
            }
        }
    } else if n.len() >= 4 {
        for token in tokens {
            if token == &n {
                return true;
            }
            if token.len() >= 4 && (token.contains(&n) || n.contains(token.as_str())) {
                return true;
            }
        }
    }
    false
}

fn display_title(ident: &str, raw: &str) -> String {
    let key = family_key(ident);
    if key.is_empty() {
        return raw.to_string();
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
        None => raw.to_string(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_plist_and_byhost_uuid() {
        assert_eq!(
            normalize_ident("com.spotify.client.plist"),
            "com.spotify.client"
        );
        assert_eq!(
            normalize_ident("com.foo.bar.A1B2C3D4-E5F6-7890-ABCD-EF1234567890.plist"),
            "com.foo.bar"
        );
        assert_eq!(normalize_ident("Slack.savedState"), "slack");
    }

    #[test]
    fn family_key_uses_product_component() {
        assert_eq!(family_key("com.spotify.client"), "spotify");
        assert_eq!(family_key("com.tinyspeck.slackmacgap"), "slackmacgap");
        assert_eq!(family_key("Docker"), "docker");
    }

    #[test]
    fn treats_matching_bundle_as_installed() {
        let mut tokens = HashSet::new();
        insert_token(&mut tokens, "spotify");
        insert_token(&mut tokens, "com.spotify.client");
        let installed = InstalledApps { tokens };
        assert!(likely_installed("com.spotify.client.plist", &installed));
        assert!(likely_installed("Spotify", &installed));
        assert!(!likely_installed("com.uninstalled.ghost.app", &installed));
        assert!(!likely_installed("GhostAppSupport", &installed));
    }

    #[test]
    fn skips_apple_idents() {
        let installed = InstalledApps::default();
        assert!(likely_installed("com.apple.Safari", &installed));
        assert!(should_skip_ident("group.com.apple.notes"));
    }
}
