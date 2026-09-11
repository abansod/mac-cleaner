use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use walkdir::WalkDir;

/// Truncate in this many bytes so a multi-GB unlink can report progress.
const PROGRESS_CHUNK: u64 = 32 * 1024 * 1024;

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

/// Delete `path`, calling `on_progress(message, bytes_just_freed)` as space is released.
pub fn delete_path_with_progress(
    path: &Path,
    on_progress: &mut dyn FnMut(&str, u64),
) -> Result<()> {
    let meta = match path.symlink_metadata() {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if meta.file_type().is_symlink() {
        fs::remove_file(path)?;
        return Ok(());
    }
    if meta.is_file() {
        return delete_regular_file(path, &meta, on_progress);
    }
    if meta.is_dir() {
        return delete_dir(path, on_progress);
    }
    bail!("unknown type");
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn delete_regular_file(
    path: &Path,
    meta: &fs::Metadata,
    on_progress: &mut dyn FnMut(&str, u64),
) -> Result<()> {
    let size = meta.len();
    let label = format!("Deleting {} ({})…", display_name(path), format_bytes(size));
    on_progress(&label, 0);

    if size > PROGRESS_CHUNK && meta.nlink() == 1 {
        let mut remaining = size;
        let mut truncated = 0u64;
        while remaining > PROGRESS_CHUNK {
            remaining -= PROGRESS_CHUNK;
            let Ok(file) = fs::OpenOptions::new().write(true).open(path) else {
                break;
            };
            if file.set_len(remaining).is_err() {
                break;
            }
            drop(file);
            truncated += PROGRESS_CHUNK;
            on_progress(&label, PROGRESS_CHUNK);
        }
        fs::remove_file(path)?;
        if size > truncated {
            on_progress(&label, size - truncated);
        }
        return Ok(());
    }

    fs::remove_file(path)?;
    on_progress(&label, size);
    Ok(())
}

fn delete_dir(path: &Path, on_progress: &mut dyn FnMut(&str, u64)) -> Result<()> {
    let mut first_error: Option<anyhow::Error> = None;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .contents_first(true)
        .into_iter()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err.into());
                }
                continue;
            }
        };
        let child = entry.path();
        let ft = entry.file_type();
        let result = if ft.is_symlink() {
            fs::remove_file(child).map_err(Into::into)
        } else if ft.is_file() {
            match child.symlink_metadata() {
                Ok(meta) => delete_regular_file(child, &meta, on_progress),
                Err(_) => fs::remove_file(child).map_err(Into::into),
            }
        } else if ft.is_dir() {
            fs::remove_dir(child)
                .or_else(|_| fs::remove_dir_all(child))
                .map_err(Into::into)
        } else {
            fs::remove_file(child).map_err(Into::into)
        };
        if let Err(err) = result {
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }
    if path.exists() {
        if let Err(err) = fs::remove_dir_all(path) {
            if first_error.is_none() {
                first_error = Some(err.into());
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
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

    #[test]
    fn delete_reports_bytes_for_file() {
        let dir = std::env::temp_dir().join(format!(
            "mac-cleaner-del-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("chunk.txt");
        fs::write(&file, vec![0u8; 4096]).unwrap();
        let mut freed = 0u64;
        delete_path_with_progress(&file, &mut |_, n| freed += n).unwrap();
        assert!(!file.exists());
        assert_eq!(freed, 4096);

        let extra = dir.join("plain.txt");
        fs::write(&extra, b"hi").unwrap();
        delete_path_with_progress(&extra, &mut |_, _| {}).unwrap();
        assert!(!extra.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_file_reports_chunked_progress() {
        let dir = std::env::temp_dir().join(format!(
            "mac-cleaner-big-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("big.bin");
        let size = PROGRESS_CHUNK + 2048;
        let handle = fs::File::create(&file).unwrap();
        handle.set_len(size).unwrap();
        drop(handle);

        let mut chunks = Vec::new();
        delete_path_with_progress(&file, &mut |_, n| {
            if n > 0 {
                chunks.push(n);
            }
        })
        .unwrap();
        assert!(!file.exists());
        assert!(
            chunks.len() >= 2,
            "expected chunked progress, got {chunks:?}"
        );
        assert_eq!(chunks.iter().sum::<u64>(), size);
        let _ = fs::remove_dir_all(&dir);
    }
}
