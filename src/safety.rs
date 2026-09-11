use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use walkdir::WalkDir;

const PROTECTED_PREFIXES: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/private/var/db",
    "/Library/Apple",
];

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn library_dir() -> PathBuf {
    home_dir().join("Library")
}

pub fn format_bytes(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    for (i, unit) in units.iter().enumerate() {
        if size < 1024.0 || i == units.len() - 1 {
            if *unit == "B" {
                return format!("{n} B");
            }
            return format!("{size:.1} {unit}");
        }
        size /= 1024.0;
    }
    format!("{n} B")
}

pub fn list_children(path: &Path) -> Vec<PathBuf> {
    let mut kids: Vec<PathBuf> = match fs::read_dir(path) {
        Ok(rd) => rd
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .collect(),
        Err(_) => return Vec::new(),
    };
    kids.sort_by_key(|a| {
        a.file_name()
            .map(|n| n.to_ascii_lowercase())
            .unwrap_or_default()
    });
    kids
}

pub fn safe_size(path: &Path) -> u64 {
    let Ok(meta) = path.symlink_metadata() else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }

    let mut total = 0u64;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let Ok(entry_meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if entry_meta.file_type().is_symlink() {
            continue;
        }
        if entry_meta.is_file() {
            total = total.saturating_add(entry_meta.len());
        }
    }
    total
}

pub fn is_writable(path: &Path) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

pub fn is_safe_to_delete(path: &Path) -> bool {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = resolved.to_string_lossy();
    let home = home_dir().canonicalize().unwrap_or_else(|_| home_dir());
    let home_s = home.to_string_lossy();

    if s == home_s {
        return false;
    }
    if s.starts_with(&format!("{home_s}/")) {
        return true;
    }

    for prefix in PROTECTED_PREFIXES {
        if s == *prefix || s.starts_with(&format!("{prefix}/")) {
            return false;
        }
    }

    if s.starts_with("/tmp/") || s.starts_with("/private/tmp/") {
        return true;
    }
    if s.starts_with("/Library/Caches/") {
        return is_writable(&resolved);
    }
    false
}

pub fn delete_path(path: &Path) -> Result<()> {
    let meta = match path.symlink_metadata() {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path)?;
        return Ok(());
    }
    if meta.is_dir() {
        fs::remove_dir_all(path)?;
        return Ok(());
    }
    bail!("unknown type");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn refuses_home_and_system() {
        assert!(!is_safe_to_delete(&home_dir()));
        assert!(!is_safe_to_delete(Path::new("/System/Library")));
        assert!(!is_safe_to_delete(Path::new("/usr/bin/ls")));
    }

    #[test]
    fn allows_paths_under_home() {
        let nested = home_dir().join("Library").join("Caches").join("example");
        assert!(is_safe_to_delete(&nested));
    }
}
