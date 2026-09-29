//! Docker Desktop caches and disk images that Docker no longer uses.
//!
//! Docker Desktop keeps every image, container, and volume inside one sparse
//! VM disk (`Docker.raw`, or `Docker.qcow2` on old installs). The active disk
//! lives in the `DataFolder` from
//! `~/Library/Group Containers/group.com.docker/settings-store.json`
//! (default `<HOME>/Library/Containers/com.docker.docker/Data/vms/0/data`).
//! The active disk is never offered while Docker Desktop is installed.
//!
//! A disk image is only offered when one of these holds:
//! - Docker Desktop is not installed and not running (leftover after the app
//!   was dragged to the Trash), or
//! - Docker Desktop is installed, the active folder has its own disk image,
//!   and this image sits in a legacy/other `vms/<n>/data` folder, or is a
//!   `Docker.qcow2` superseded by a newer `Docker.raw` in the active folder.
//!
//! `Data/Docker.raw` is never offered while Docker Desktop is installed, since
//! Docker VMM may use it.
//!
//! Caches (`~/Library/Caches/com.docker.docker` and the Electron dashboard
//! caches) are rebuilt on the next launch; they are refused while Docker
//! Desktop is running.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};

use crate::safety::home_dir;

const BUNDLE_ID: &str = "com.docker.docker";
const DISK_NAMES: [&str; 2] = ["Docker.raw", "Docker.qcow2"];
const RAW: &str = "Docker.raw";
const QCOW2: &str = "Docker.qcow2";
/// Chromium/Electron caches under `~/Library/Application Support/Docker Desktop`.
pub const ELECTRON_CACHE_DIRS: [&str; 6] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
];
const PROCESS_PATTERN: &str = "com\\.docker\\.backend|com\\.docker\\.virtualization|com\\.docker\\.krun|Docker\\.app/Contents/MacOS/Docker Desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerState {
    pub installed: bool,
    pub running: bool,
}

impl DockerState {
    pub fn detect() -> Self {
        let running = is_running();
        Self {
            installed: running || is_installed(&home_dir()),
            running,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusedDisk {
    pub path: PathBuf,
    pub reason: String,
}

pub fn container_data(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Containers")
        .join(BUNDLE_ID)
        .join("Data")
}

pub fn container_root(home: &Path) -> PathBuf {
    home.join("Library").join("Containers").join(BUNDLE_ID)
}

pub fn default_data_folder(home: &Path) -> PathBuf {
    container_data(home).join("vms").join("0").join("data")
}

pub fn cache_root(home: &Path) -> PathBuf {
    home.join("Library").join("Caches").join(BUNDLE_ID)
}

pub fn app_support(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Docker Desktop")
}

fn group_container(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Group Containers")
        .join("group.com.docker")
}

/// `DataFolder` from Docker Desktop settings, with `<HOME>` expanded.
pub fn configured_data_folder(home: &Path) -> Option<PathBuf> {
    let group = group_container(home);
    for (file, keys) in [
        ("settings-store.json", ["DataFolder", "dataFolder"]),
        ("settings.json", ["dataFolder", "DataFolder"]),
    ] {
        let Ok(text) = fs::read_to_string(group.join(file)) else {
            continue;
        };
        for key in keys {
            if let Some(value) = json_string_value(&text, key) {
                let value = value.trim();
                if value.is_empty() {
                    continue;
                }
                let expanded = value.replace("<HOME>", &home.to_string_lossy());
                return Some(PathBuf::from(expanded));
            }
        }
    }
    None
}

pub fn active_data_folder(home: &Path) -> PathBuf {
    configured_data_folder(home).unwrap_or_else(|| default_data_folder(home))
}

/// Minimal JSON lookup of a top-level-ish `"key": "string"` pair.
pub fn json_string_value(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut search = json;
    while let Some(pos) = search.find(&needle) {
        let rest = search[pos + needle.len()..].trim_start();
        if let Some(rest) = rest.strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(body) = rest.strip_prefix('"') {
                return decode_json_string(body);
            }
            return None;
        }
        search = &search[pos + needle.len()..];
    }
    None
}

fn decode_json_string(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

fn is_installed(home: &Path) -> bool {
    let app_paths = [
        PathBuf::from("/Applications/Docker.app"),
        home.join("Applications").join("Docker.app"),
    ];
    if app_paths.iter().any(|p| p.exists()) {
        return true;
    }
    let cli_links = [
        PathBuf::from("/usr/local/bin/docker"),
        PathBuf::from("/usr/local/bin/com.docker.cli"),
        home.join(".docker").join("bin").join("docker"),
    ];
    let cli_in_app = cli_links.iter().any(|link| {
        link.canonicalize()
            .is_ok_and(|target| target.to_string_lossy().contains("Docker.app/"))
    });
    if cli_in_app {
        return true;
    }
    Command::new("/usr/bin/mdfind")
        .arg(format!("kMDItemCFBundleIdentifier == \"{BUNDLE_ID}\""))
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim_end().ends_with(".app") && Path::new(line.trim()).exists())
        })
}

pub fn is_running() -> bool {
    Command::new("/usr/bin/pgrep")
        .args(["-f", PROCESS_PATTERN])
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

/// Every Docker Desktop disk image file that exists under known locations.
pub fn disk_image_candidates(home: &Path) -> Vec<PathBuf> {
    let data = container_data(home);
    let mut folders = vec![data.clone(), data.join("com.docker.driver.amd64-linux")];
    folders.extend(
        crate::safety::list_children(&data.join("vms"))
            .into_iter()
            .map(|vm| vm.join("data")),
    );
    if let Some(configured) = configured_data_folder(home) {
        folders.push(configured);
    }

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for folder in folders {
        for name in DISK_NAMES {
            let path = folder.join(name);
            if !is_regular_file(&path) {
                continue;
            }
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen.insert(key) {
                out.push(path);
            }
        }
    }
    out
}

/// Disk images nothing uses, given whether Docker Desktop is installed.
pub fn unused_disk_images_in(home: &Path, installed: bool) -> Vec<UnusedDisk> {
    let candidates = disk_image_candidates(home);
    if !installed {
        return candidates
            .into_iter()
            .map(|path| UnusedDisk {
                path,
                reason: "Docker Desktop is not installed".into(),
            })
            .collect();
    }

    let active = active_data_folder(home);
    let active_raw = active.join(RAW);
    let active_has_disk = DISK_NAMES.iter().any(|n| is_regular_file(&active.join(n)));
    if !active_has_disk {
        return Vec::new();
    }
    let data = container_data(home);

    candidates
        .into_iter()
        .filter_map(|path| {
            let parent = path.parent()?;
            let name = path.file_name()?.to_str()?;
            let reason = if same_path(parent, &active) {
                if name == QCOW2 && newer_than(&active_raw, &path) {
                    "Superseded by the newer Docker.raw in the active disk folder".to_string()
                } else {
                    return None;
                }
            } else if same_path(parent, &data) {
                return None;
            } else if is_under(parent, &data) {
                format!(
                    "Docker Desktop now uses the disk image in {}",
                    active.display()
                )
            } else {
                return None;
            };
            Some(UnusedDisk { path, reason })
        })
        .collect()
}

pub fn unused_disk_images(state: DockerState) -> Vec<UnusedDisk> {
    unused_disk_images_in(&home_dir(), state.installed || state.running)
}

/// Re-check right before deletion.
pub fn check_disk_reclaimable(path: &Path) -> Result<()> {
    let state = DockerState::detect();
    if state.running {
        bail!("Refused (quit Docker Desktop first)");
    }
    if !unused_disk_images(state).iter().any(|d| d.path == path) {
        bail!("Refused (Docker Desktop may still use this disk image)");
    }
    Ok(())
}

pub fn check_cache_reclaimable(path: &Path) -> Result<()> {
    if !is_cache_item_in(path, &home_dir()) {
        bail!("Refused (not a Docker Desktop cache)");
    }
    if is_running() {
        bail!("Refused (quit Docker Desktop first)");
    }
    Ok(())
}

/// Items under `~/Library/Caches/com.docker.docker`, or an Electron cache folder.
pub fn is_cache_item_in(path: &Path, home: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return false;
    }
    let caches = cache_root(home);
    if is_under(path, &caches) {
        return true;
    }
    let support = app_support(home);
    path.parent().is_some_and(|p| same_path(p, &support))
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| ELECTRON_CACHE_DIRS.contains(&n))
}

fn is_regular_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_file())
}

fn newer_than(a: &Path, b: &Path) -> bool {
    let mtime = |p: &Path| p.symlink_metadata().and_then(|m| m.modified()).ok();
    match (mtime(a), mtime(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.canonicalize()
            .unwrap_or_else(|_| p.components().collect())
    };
    norm(a) == norm(b)
}

fn is_under(path: &Path, parent: &Path) -> bool {
    let norm = |p: &Path| {
        p.canonicalize()
            .unwrap_or_else(|_| p.components().collect())
    };
    norm(path)
        .strip_prefix(norm(parent))
        .is_ok_and(|rel| !rel.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("mc-docker-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"disk").unwrap();
    }

    fn set_mtime(path: &Path, stamp: &str) {
        let status = Command::new("/usr/bin/touch")
            .args(["-t", stamp])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn write_settings(home: &Path, json: &str) {
        let file = group_container(home).join("settings-store.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, json).unwrap();
    }

    fn paths(disks: &[UnusedDisk]) -> Vec<PathBuf> {
        disks.iter().map(|d| d.path.clone()).collect()
    }

    #[test]
    fn json_lookup_decodes_escapes() {
        let json = r#"{"Cpus":4,"DataFolder":"\u003cHOME\u003e/Library/x \"y\"","Other":"z"}"#;
        assert_eq!(
            json_string_value(json, "DataFolder").as_deref(),
            Some("<HOME>/Library/x \"y\"")
        );
        assert_eq!(json_string_value(json, "Cpus"), None);
        assert_eq!(json_string_value(json, "Missing"), None);
        assert_eq!(
            json_string_value(r#"{"dataFolder" : "/a/b"}"#, "dataFolder").as_deref(),
            Some("/a/b")
        );
    }

    #[test]
    fn data_folder_expands_home_placeholder() {
        let home = temp_home("folder");
        assert_eq!(active_data_folder(&home), default_data_folder(&home));
        write_settings(&home, r#"{"DataFolder":"\u003cHOME\u003e/DockerData"}"#);
        assert_eq!(active_data_folder(&home), home.join("DockerData"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn active_disk_is_never_offered_while_installed() {
        let home = temp_home("active");
        let raw = default_data_folder(&home).join(RAW);
        touch(&raw);
        let vmm = container_data(&home).join(RAW);
        touch(&vmm);
        assert!(unused_disk_images_in(&home, true).is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn uninstalled_leftovers_are_all_offered() {
        let home = temp_home("uninstalled");
        let raw = default_data_folder(&home).join(RAW);
        let vmm = container_data(&home).join(RAW);
        touch(&raw);
        touch(&vmm);
        let mut found = paths(&unused_disk_images_in(&home, false));
        found.sort();
        let mut expected = vec![raw, vmm];
        expected.sort();
        assert_eq!(found, expected);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn legacy_and_other_vm_disks_are_stale_when_active_exists() {
        let home = temp_home("stale");
        let active = default_data_folder(&home).join(RAW);
        let legacy = container_data(&home)
            .join("com.docker.driver.amd64-linux")
            .join(QCOW2);
        let other_vm = container_data(&home)
            .join("vms")
            .join("1")
            .join("data")
            .join(RAW);
        touch(&active);
        touch(&legacy);
        touch(&other_vm);
        let mut found = paths(&unused_disk_images_in(&home, true));
        found.sort();
        let mut expected = vec![legacy.clone(), other_vm.clone()];
        expected.sort();
        assert_eq!(found, expected);

        fs::remove_file(&active).unwrap();
        assert!(
            unused_disk_images_in(&home, true).is_empty(),
            "without an active disk nothing is provably stale"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn default_disk_is_stale_after_move_to_custom_folder() {
        let home = temp_home("moved");
        let old = default_data_folder(&home).join(RAW);
        let new = home.join("DockerData").join(RAW);
        touch(&old);
        write_settings(&home, r#"{"DataFolder":"\u003cHOME\u003e/DockerData"}"#);
        assert!(
            unused_disk_images_in(&home, true).is_empty(),
            "custom folder has no disk yet — keep the default one"
        );
        touch(&new);
        assert_eq!(paths(&unused_disk_images_in(&home, true)), vec![old]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn qcow2_only_stale_when_older_than_raw() {
        let home = temp_home("qcow");
        let folder = default_data_folder(&home);
        let raw = folder.join(RAW);
        let qcow = folder.join(QCOW2);
        touch(&raw);
        touch(&qcow);
        set_mtime(&raw, "202401010000");
        assert!(unused_disk_images_in(&home, true).is_empty());
        set_mtime(&qcow, "202001010000");
        assert_eq!(paths(&unused_disk_images_in(&home, true)), vec![qcow]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn cache_items_are_allowlisted() {
        let home = temp_home("cache");
        let cache = cache_root(&home).join("update.zip");
        touch(&cache);
        let gpu = app_support(&home).join("GPUCache");
        fs::create_dir_all(&gpu).unwrap();
        let prefs = app_support(&home).join("Preferences");
        touch(&prefs);
        assert!(is_cache_item_in(&cache, &home));
        assert!(is_cache_item_in(&gpu, &home));
        assert!(!is_cache_item_in(&prefs, &home));
        assert!(!is_cache_item_in(&cache_root(&home), &home));
        assert!(!is_cache_item_in(&app_support(&home), &home));
        let _ = fs::remove_dir_all(&home);
    }
}
