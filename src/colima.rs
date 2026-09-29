//! Colima download cache and orphaned Lima data disks.
//!
//! Colima keeps downloaded VM images in `~/Library/Caches/colima`; they are
//! re-downloaded on the next `colima start` that needs them.
//!
//! Container data (images, volumes) lives on a separate Lima data disk at
//! `<colima home>/_lima/_disks/<instance>/datadisk`. `colima delete` keeps
//! this disk unless `--data` is passed, so deleted profiles can leave many GB
//! behind. A disk is only offered when nothing can still use it:
//! - its `in_use_by` link is absent or points at a missing instance, and
//! - no instance folder with the same name exists, and
//! - no instance `lima.yaml` lists it under `additionalDisks`.
//!
//! Disk files are sparse, so sizes are allocated blocks, not the logical size.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::safety::home_dir;

pub const DATADISK_FILE: &str = "datadisk";
const IN_USE_LINK: &str = "in_use_by";

/// Candidate Colima config homes, in Colima's lookup order, deduplicated.
pub fn colima_homes() -> Vec<PathBuf> {
    colima_homes_in(
        &home_dir(),
        std::env::var_os("COLIMA_HOME").map(PathBuf::from),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    )
}

fn colima_homes_in(
    home: &Path,
    colima_home: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = colima_home.filter(|d| !d.as_os_str().is_empty()) {
        candidates.push(dir);
    }
    candidates.push(home.join(".colima"));
    if let Some(dir) = xdg_config.filter(|d| !d.as_os_str().is_empty()) {
        candidates.push(dir.join("colima"));
    }
    candidates.push(home.join(".config").join("colima"));

    let mut seen = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter(|dir| dir.is_dir())
        .filter(|dir| seen.insert(dir.canonicalize().unwrap_or_else(|_| dir.clone())))
        .collect()
}

pub fn cache_root() -> PathBuf {
    cache_root_in(&home_dir())
}

fn cache_root_in(home: &Path) -> PathBuf {
    home.join("Library").join("Caches").join("colima")
}

pub fn disks_root(colima_home: &Path) -> PathBuf {
    colima_home.join("_lima").join("_disks")
}

/// Human profile name for a Lima instance name (`colima` → `default`).
pub fn profile_name(instance: &str) -> String {
    match instance.strip_prefix("colima-") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ if instance == "colima" => "default".to_string(),
        _ => instance.to_string(),
    }
}

/// Lima identifier rule: `^[A-Za-z0-9]+(?:[._-][A-Za-z0-9]+)*$`.
pub fn is_valid_disk_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut prev_sep = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            prev_sep = false;
        } else if matches!(c, '.' | '_' | '-') {
            if prev_sep {
                return false;
            }
            prev_sep = true;
        } else {
            return false;
        }
    }
    !prev_sep
}

/// All unused data disk folders under `colima_home`.
pub fn unused_disks(colima_home: &Path) -> Vec<PathBuf> {
    let root = disks_root(colima_home);
    crate::safety::list_children(&root)
        .into_iter()
        .filter(|dir| is_unused_disk_in(dir, colima_home))
        .collect()
}

/// True when `disk_dir` is `<colima_home>/_lima/_disks/<name>` and nothing uses it.
pub fn is_unused_disk_in(disk_dir: &Path, colima_home: &Path) -> bool {
    let lima = colima_home.join("_lima");
    let Some(name) = disk_dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !is_valid_disk_name(name) {
        return false;
    }
    let Some(parent) = disk_dir.parent() else {
        return false;
    };
    if parent != lima.join("_disks") {
        return false;
    }
    let Ok(meta) = disk_dir.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    let Ok(disk_meta) = disk_dir.join(DATADISK_FILE).symlink_metadata() else {
        return false;
    };
    if !disk_meta.is_file() {
        return false;
    }
    if in_use_link_is_live(&disk_dir.join(IN_USE_LINK)) {
        return false;
    }
    if lima.join(name).symlink_metadata().is_ok() {
        return false;
    }
    !any_instance_references(&lima, name)
}

/// Conservative: any `in_use_by` entry that is not a dangling symlink counts as live.
fn in_use_link_is_live(link: &Path) -> bool {
    let Ok(meta) = link.symlink_metadata() else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return true;
    }
    fs::metadata(link).is_ok()
}

fn any_instance_references(lima: &Path, disk: &str) -> bool {
    let Ok(entries) = fs::read_dir(lima) else {
        // Cannot prove the disk is unused.
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('_') || name.starts_with('.') {
            continue;
        }
        let yaml = entry.path().join("lima.yaml");
        match fs::read_to_string(&yaml) {
            Ok(text) => {
                if additional_disk_names(&text).iter().any(|d| d == disk) {
                    return true;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
    }
    false
}

/// Disk names from a Lima `additionalDisks:` block (list of names or `name:` maps).
pub fn additional_disk_names(yaml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_block = false;
    for raw in yaml.lines() {
        let line = raw.split(" #").next().unwrap_or(raw).trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let top_level = !line.starts_with(' ') && !line.starts_with('\t');
        if top_level && !trimmed.starts_with('-') {
            in_block = false;
            if let Some(rest) = trimmed.strip_prefix("additionalDisks:") {
                let rest = rest.trim();
                if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                    names.extend(inner.split(',').map(unquote).filter(|n| !n.is_empty()));
                } else {
                    in_block = rest.is_empty();
                }
            }
            continue;
        }
        if !in_block {
            continue;
        }
        let item = trimmed.strip_prefix('-').map(str::trim).unwrap_or(trimmed);
        if let Some(value) = item.strip_prefix("name:") {
            names.push(unquote(value));
        } else if trimmed.starts_with('-') && !item.contains(':') && !item.is_empty() {
            names.push(unquote(item));
        }
    }
    names.retain(|n| !n.is_empty());
    names
}

fn unquote(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_string()
}

/// True when `path` is strictly inside `~/Library/Caches/colima`.
pub fn is_cache_item(path: &Path) -> bool {
    is_cache_item_in(path, &home_dir())
}

fn is_cache_item_in(path: &Path, home: &Path) -> bool {
    let root = cache_root_in(home);
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return false;
    }
    let root_c = root.canonicalize().unwrap_or(root);
    let path_c = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path_c
        .strip_prefix(&root_c)
        .is_ok_and(|rel| !rel.as_os_str().is_empty())
}

/// Re-check right before deletion that `path` is still an unused Colima data disk.
pub fn check_disk_reclaimable(path: &Path) -> Result<()> {
    let home = colima_homes()
        .into_iter()
        .find(|home| path.parent() == Some(disks_root(home).as_path()));
    let Some(home) = home else {
        bail!("Refused (not a Colima data disk folder)");
    };
    if !is_unused_disk_in(path, &home) {
        bail!("Refused (Colima data disk is in use by an instance)");
    }
    Ok(())
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
            std::env::temp_dir().join(format!("mc-colima-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_disk(colima: &Path, name: &str) -> PathBuf {
        let dir = disks_root(colima).join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(DATADISK_FILE), b"disk").unwrap();
        dir
    }

    #[test]
    fn disk_names_follow_lima_rules() {
        assert!(is_valid_disk_name("colima"));
        assert!(is_valid_disk_name("colima-dev"));
        assert!(is_valid_disk_name("a.b_c-d"));
        assert!(!is_valid_disk_name(".DS_Store"));
        assert!(!is_valid_disk_name("colima-"));
        assert!(!is_valid_disk_name("a--b"));
        assert!(!is_valid_disk_name(".."));
        assert!(!is_valid_disk_name("a/b"));
    }

    #[test]
    fn profile_names() {
        assert_eq!(profile_name("colima"), "default");
        assert_eq!(profile_name("colima-work"), "work");
        assert_eq!(profile_name("other"), "other");
    }

    #[test]
    fn parses_additional_disks() {
        let yaml = "images: []\nadditionalDisks:\n    - name: colima\n      format: false\n    - name: \"extra\"\n    - plain\nmounts:\n  - location: ~\n    name: notadisk\n";
        assert_eq!(
            additional_disk_names(yaml),
            vec!["colima", "extra", "plain"]
        );
        assert_eq!(
            additional_disk_names("additionalDisks: [a, 'b']\n"),
            vec!["a", "b"]
        );
        assert!(additional_disk_names("additionalDisks: []\n").is_empty());
        assert!(additional_disk_names("cpus: 2\n").is_empty());
    }

    #[test]
    fn orphaned_disk_is_unused() {
        let colima = temp_home("orphan");
        fs::create_dir_all(colima.join("_lima")).unwrap();
        let disk = make_disk(&colima, "colima-old");
        assert!(is_unused_disk_in(&disk, &colima));
        assert_eq!(unused_disks(&colima), vec![disk.clone()]);

        std::os::unix::fs::symlink(
            colima.join("_lima").join("colima-old"),
            disk.join(IN_USE_LINK),
        )
        .unwrap();
        assert!(
            is_unused_disk_in(&disk, &colima),
            "dangling in_use_by is unused"
        );
        let _ = fs::remove_dir_all(&colima);
    }

    #[test]
    fn disk_with_live_instance_is_in_use() {
        let colima = temp_home("live");
        let inst = colima.join("_lima").join("colima");
        fs::create_dir_all(&inst).unwrap();
        let disk = make_disk(&colima, "colima");
        assert!(
            !is_unused_disk_in(&disk, &colima),
            "same-name instance exists"
        );

        std::os::unix::fs::symlink(&inst, disk.join(IN_USE_LINK)).unwrap();
        assert!(!is_unused_disk_in(&disk, &colima));
        let _ = fs::remove_dir_all(&colima);
    }

    #[test]
    fn disk_referenced_by_stopped_instance_is_in_use() {
        let colima = temp_home("ref");
        let inst = colima.join("_lima").join("colima-a");
        fs::create_dir_all(&inst).unwrap();
        fs::write(
            inst.join("lima.yaml"),
            "additionalDisks:\n  - name: shared\n",
        )
        .unwrap();
        let disk = make_disk(&colima, "shared");
        assert!(!is_unused_disk_in(&disk, &colima));
        let _ = fs::remove_dir_all(&colima);
    }

    #[test]
    fn rejects_non_disk_paths() {
        let colima = temp_home("reject");
        fs::create_dir_all(colima.join("_lima")).unwrap();
        let empty = disks_root(&colima).join("nodisk");
        fs::create_dir_all(&empty).unwrap();
        assert!(!is_unused_disk_in(&empty, &colima), "no datadisk file");
        let disk = make_disk(&colima, "x");
        assert!(!is_unused_disk_in(&disk.join(DATADISK_FILE), &colima));
        assert!(!is_unused_disk_in(&disks_root(&colima), &colima));
        assert!(!is_unused_disk_in(&colima, &colima));
        let _ = fs::remove_dir_all(&colima);
    }

    #[test]
    fn cache_items_must_be_inside_cache_root() {
        let home = temp_home("cache");
        let root = cache_root_in(&home);
        let file = root.join("caches").join("abc");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"img").unwrap();
        assert!(is_cache_item_in(&file, &home));
        assert!(!is_cache_item_in(&root, &home));
        assert!(!is_cache_item_in(&home.join("Library"), &home));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn allocated_size_is_sparse_aware() {
        let home = temp_home("sparse");
        let file = home.join("sparse.img");
        let handle = fs::File::create(&file).unwrap();
        handle.set_len(1024 * 1024 * 1024).unwrap();
        drop(handle);
        assert!(crate::safety::allocated_size(&file) < 1024 * 1024);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn homes_are_deduplicated() {
        let home = temp_home("homes");
        fs::create_dir_all(home.join(".colima")).unwrap();
        let homes = colima_homes_in(&home, Some(home.join(".colima")), None);
        assert_eq!(homes, vec![home.join(".colima")]);
        let _ = fs::remove_dir_all(&home);
    }
}
