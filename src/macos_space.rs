//! Allowlisted reclaim of hidden macOS space.
//!
//! Nothing in this module deletes by walking APFS snapshot contents or the
//! sealed system volume. Time Machine local snapshots are removed only through
//! `tmutil deletelocalsnapshots <YYYY-MM-DD-HHMMSS>`. iOS backups are removed
//! only as a whole finished device folder. Messages reclaim is attachment
//! files only — never `chat.db`.
//!
//! Explicitly out of scope (would risk the OS or cloud data):
//! - `com.apple.os.update-*` / sealed boot snapshots
//! - `diskutil apfs deleteSnapshot`
//! - `/private/var/vm` swap and sleepimage
//! - iCloud `Mobile Documents` / `CloudStorage` unlinks (evict is not available
//!   via `brctl` on current macOS, and unlink deletes every device)

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};

use crate::models::{Category, DiskPressure, FileItem, ReclaimOp};
use crate::safety::{home_dir, is_safe_to_delete, safe_size};

const TMUTIL: &str = "/usr/bin/tmutil";
const DISKUTIL: &str = "/usr/sbin/diskutil";
const PLUTIL: &str = "/usr/bin/plutil";

const TM_NAME_PREFIX: &str = "com.apple.TimeMachine.";
const TM_NAME_SUFFIX: &str = ".local";
const TM_DATE_LEN: usize = 17; // YYYY-MM-DD-HHMMSS

pub fn disk_pressure() -> Option<DiskPressure> {
    let mount = if Path::new("/System/Volumes/Data").is_dir() {
        "/System/Volumes/Data"
    } else {
        "/"
    };
    let container_bytes = plutil_u64_from_diskutil(mount, "APFSContainerSize")?;
    let container_free = plutil_u64_from_diskutil(mount, "APFSContainerFree")?;
    Some(DiskPressure {
        mount: mount.to_string(),
        container_bytes,
        container_free,
    })
}

pub fn scan_warnings(include_messages: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(name) = boot_snapshot_name() {
        if is_os_update_snapshot_name(&name) {
            warnings.push(
                "Booted from a sealed macOS snapshot; it is never offered for deletion.".into(),
            );
        }
    }
    if include_messages {
        if let Some(msg) = messages_permission_warning() {
            warnings.push(msg);
        }
    }
    warnings
}

pub fn boot_snapshot_name() -> Option<String> {
    plutil_raw_from_diskutil("/", "APFSSnapshotName")
}

pub fn is_os_update_snapshot_name(name: &str) -> bool {
    let n = name.trim();
    n.starts_with("com.apple.os.update")
        || n.contains("os.update")
        || n.contains("MSUPrepare")
        || n.contains("bless")
}

/// Extract `YYYY-MM-DD-HHMMSS` only from a Time Machine *local* snapshot name.
pub fn parse_tm_local_snapshot_name(line: &str) -> Option<&str> {
    let line = line.trim();
    if is_os_update_snapshot_name(line) {
        return None;
    }
    if !line.starts_with(TM_NAME_PREFIX) || !line.ends_with(TM_NAME_SUFFIX) {
        return None;
    }
    let date = &line[TM_NAME_PREFIX.len()..line.len() - TM_NAME_SUFFIX.len()];
    is_valid_tm_snapshot_date(date).then_some(date)
}

/// `tmutil deletelocalsnapshots` takes either a date or a mount point.
/// We only ever pass a date so `/` (the sealed boot snapshot) cannot be used.
pub fn is_valid_tm_snapshot_date(date: &str) -> bool {
    if date.len() != TM_DATE_LEN || date.contains('/') || date.contains('\\') {
        return false;
    }
    let b = date.as_bytes();
    let digit = |i: usize| b[i].is_ascii_digit();
    digit(0)
        && digit(1)
        && digit(2)
        && digit(3)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9)
        && b[10] == b'-'
        && (11..17).all(digit)
}

pub fn list_tm_local_snapshot_dates() -> Vec<String> {
    let mut dates = Vec::new();
    for mount in tm_list_mounts() {
        let Some(out) = run_stdout(TMUTIL, &["listlocalsnapshots", mount]) else {
            continue;
        };
        for line in out.lines() {
            if let Some(date) = parse_tm_local_snapshot_name(line) {
                dates.push(date.to_string());
            }
        }
    }
    dates.sort();
    dates.dedup();
    dates
}

pub fn tm_backup_running() -> bool {
    let Some(out) = run_stdout(TMUTIL, &["status"]) else {
        return false;
    };
    out.lines().any(|line| {
        let t = line.trim();
        t.starts_with("Running") && (t.contains("= 1;") || t.ends_with("= 1"))
    })
}

pub fn is_ios_backup_bundle(path: &Path) -> bool {
    is_ios_backup_bundle_in(path, &home_dir())
}

pub fn is_messages_attachment_file(path: &Path) -> bool {
    is_messages_attachment_file_in(path, &home_dir())
}

pub fn is_ios_backup_bundle_in(path: &Path, home: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !is_ios_backup_folder_name(name) {
        return false;
    }
    let expected_parent = ios_backup_root(home);
    let Some(parent) = path.parent() else {
        return false;
    };
    if !path_eq_relaxed(parent, &expected_parent) {
        return false;
    }
    if !backup_looks_complete(path) {
        return false;
    }
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    let expected_canon = expected_parent
        .canonicalize()
        .unwrap_or_else(|_| expected_parent.clone());
    canonical.parent() == Some(expected_canon.as_path()) && !backup_in_progress(path)
}

pub fn is_ios_backup_folder_name(name: &str) -> bool {
    if name.contains("..") || name.contains('/') {
        return false;
    }
    let hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    if name.len() == 40 && hex(name) {
        return true;
    }
    // Hardware UDID: 00008120-001A21D11E88002E
    let mut parts = name.split('-');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), None) if a.len() == 8 && b.len() == 16 && hex(a) && hex(b)
    )
}

pub fn is_messages_attachment_file_in(path: &Path, home: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') || name.starts_with("chat.db") {
        return false;
    }
    let attachments = messages_attachments_root(home);
    let Ok(canonical) = path.canonicalize() else {
        return is_strict_under(path, &attachments);
    };
    let attach_canon = attachments.canonicalize().unwrap_or(attachments);
    is_strict_under(&canonical, &attach_canon)
}

/// User-data paths that must never be unlinked, even though they sit under $HOME.
pub fn is_protected_user_data(path: &Path) -> bool {
    is_protected_user_data_in(path, &home_dir())
}

pub fn is_protected_user_data_in(path: &Path, home: &Path) -> bool {
    let Some(rel) = home_relative(path, home) else {
        return false;
    };
    let rel = rel.as_str();

    if rel == "Library/Messages" || rel.starts_with("Library/Messages/") {
        return !is_messages_attachment_file_in(path, home);
    }
    if rel == "Library/Application Support/MobileSync"
        || rel.starts_with("Library/Application Support/MobileSync/")
    {
        return !is_ios_backup_bundle_in(path, home);
    }

    matches_protected_rel(rel)
}

fn home_relative(path: &Path, home: &Path) -> Option<String> {
    if let Ok(rel) = path.strip_prefix(home) {
        return Some(rel.to_string_lossy().into_owned());
    }
    let home_c = home.canonicalize().ok()?;
    let path_c = path.canonicalize().ok()?;
    Some(
        path_c
            .strip_prefix(&home_c)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}

fn matches_protected_rel(rel: &str) -> bool {
    let prefixes = [
        "Library/Keychains",
        "Library/Application Support/com.apple.TCC",
        "Library/Mobile Documents",
        "Library/CloudStorage",
        "Library/HomeKit",
        "Library/Accounts",
        "Library/IdentityServices",
        "Library/Suggestions",
        "Library/Group Containers/group.com.apple.replayd",
        ".ssh",
        ".gnupg",
    ];
    for prefix in prefixes {
        if rel == prefix || rel.starts_with(&format!("{prefix}/")) {
            return true;
        }
    }
    if rel == "Library/Mail" || rel.starts_with("Library/Mail/") {
        return true;
    }
    rel.contains(".photoslibrary")
}

pub fn reclaim(item: &FileItem, on_progress: &mut dyn FnMut(&str, u64)) -> Result<u64> {
    match &item.op {
        ReclaimOp::TmLocalSnapshot { date } => {
            on_progress(&format!("Removing Time Machine snapshot {date}…"), 0);
            reclaim_tm_snapshot(date)
        }
        ReclaimOp::DeletePath => reclaim_hidden_or_normal_path(item, on_progress),
        ReclaimOp::LaunchJob { .. } | ReclaimOp::OpenAtLogin { .. } => {
            on_progress("Removing login item…", 0);
            crate::login_items::remove(std::slice::from_ref(item))
                .pop()
                .map_or(Ok(0), |(_, result)| result)
        }
    }
}

fn reclaim_hidden_or_normal_path(
    item: &FileItem,
    on_progress: &mut dyn FnMut(&str, u64),
) -> Result<u64> {
    match item.category {
        Category::IosBackups => {
            if !is_ios_backup_bundle(&item.path) {
                bail!("Refused (not a complete iOS backup folder)");
            }
        }
        Category::MessagesAttachments => {
            if !is_messages_attachment_file(&item.path) {
                bail!("Refused (not a Messages attachment file)");
            }
        }
        Category::LocalSnapshots => {
            bail!("Refused (snapshots cannot be deleted as files)");
        }
        _ => {}
    }
    if !is_safe_to_delete(&item.path) {
        bail!("Refused (protected)");
    }
    let size = if item.size > 0 {
        item.size
    } else {
        safe_size(&item.path)
    };
    crate::safety::delete_path_with_progress(&item.path, on_progress)?;
    Ok(size)
}

fn reclaim_tm_snapshot(date: &str) -> Result<u64> {
    if !is_valid_tm_snapshot_date(date) {
        bail!("Refused snapshot date (not YYYY-MM-DD-HHMMSS)");
    }
    if tm_backup_running() {
        bail!("Time Machine is running; snapshot delete refused");
    }
    if !list_tm_local_snapshot_dates().iter().any(|d| d == date) {
        bail!("Refused: {date} is not a listed Time Machine local snapshot");
    }
    let output = Command::new(TMUTIL)
        .arg("deletelocalsnapshots")
        .arg(date)
        .output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("tmutil deletelocalsnapshots {date} failed: {err}");
    }
    Ok(0)
}

fn tm_list_mounts() -> Vec<&'static str> {
    let mut mounts = Vec::new();
    if Path::new("/System/Volumes/Data").is_dir() {
        mounts.push("/System/Volumes/Data");
    }
    mounts.push("/");
    mounts
}

pub(crate) fn ios_backup_root(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("MobileSync")
        .join("Backup")
}

pub(crate) fn messages_attachments_root(home: &Path) -> PathBuf {
    home.join("Library").join("Messages").join("Attachments")
}

fn backup_looks_complete(dir: &Path) -> bool {
    ["Info.plist", "Manifest.plist", "Manifest.db"]
        .iter()
        .any(|name| dir.join(name).is_file())
}

fn backup_in_progress(dir: &Path) -> bool {
    let status = dir.join("Status.plist");
    if !status.is_file() {
        return false;
    }
    let Some(state) = plutil_extract_raw(&status, "SnapshotState") else {
        return false;
    };
    let state = state.trim().to_ascii_lowercase();
    !state.is_empty() && state != "finished"
}

fn messages_permission_warning() -> Option<String> {
    let messages = home_dir().join("Library").join("Messages");
    match fs::metadata(&messages) {
        Err(err) if err.kind() == ErrorKind::PermissionDenied => Some(
            "Messages attachments skipped — grant Full Disk Access to this terminal to scan them."
                .into(),
        ),
        Ok(_) => match fs::read_dir(messages.join("Attachments")) {
            Err(err) if err.kind() == ErrorKind::PermissionDenied => Some(
                "Messages attachments skipped — grant Full Disk Access to this terminal to scan them."
                    .into(),
            ),
            _ => None,
        },
        Err(_) => None,
    }
}

fn is_strict_under(path: &Path, parent: &Path) -> bool {
    path.strip_prefix(parent)
        .is_ok_and(|rel| !rel.as_os_str().is_empty())
}

fn path_eq_relaxed(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(x, y)| x == y)
}

fn run_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

pub(crate) fn plutil_extract_raw(plist: &Path, key: &str) -> Option<String> {
    let output = Command::new(PLUTIL)
        .args(["-extract", key, "raw", "-o", "-", plist.to_str()?])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn plutil_raw_from_diskutil(mount: &str, key: &str) -> Option<String> {
    let plist = Command::new(DISKUTIL)
        .args(["info", "-plist", mount])
        .output()
        .ok()?;
    if !plist.status.success() {
        return None;
    }
    let mut child = Command::new(PLUTIL)
        .args(["-extract", key, "raw", "-o", "-", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(&plist.stdout).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn plutil_u64_from_diskutil(mount: &str, key: &str) -> Option<u64> {
    plutil_raw_from_diskutil(mount, key)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tm_parser_accepts_only_time_machine_local_names() {
        assert_eq!(
            parse_tm_local_snapshot_name("com.apple.TimeMachine.2024-01-15-093011.local"),
            Some("2024-01-15-093011")
        );
        assert_eq!(
            parse_tm_local_snapshot_name(
                "com.apple.os.update-60587424F5399FC05D957DF05B4D2F65543462495C77AEF520F790FBB57CB212"
            ),
            None
        );
        assert_eq!(parse_tm_local_snapshot_name("/"), None);
        assert_eq!(parse_tm_local_snapshot_name("/System/Volumes/Data"), None);
        assert_eq!(
            parse_tm_local_snapshot_name("com.apple.TimeMachine.2024-01-15-093011"),
            None
        );
        assert!(!is_valid_tm_snapshot_date("/"));
        assert!(!is_valid_tm_snapshot_date("/System/Volumes/Data"));
        assert!(!is_valid_tm_snapshot_date("2024-01-15-093011.local"));
        assert!(is_valid_tm_snapshot_date("2024-01-15-093011"));
    }

    #[test]
    fn os_update_names_are_detected() {
        assert!(is_os_update_snapshot_name(
            "com.apple.os.update-60587424F5399FC05D957DF05B4D2F65543462495C77AEF520F790FBB57CB212"
        ));
        assert!(!is_os_update_snapshot_name(
            "com.apple.TimeMachine.2024-01-15-093011.local"
        ));
    }

    #[test]
    fn ios_udid_names() {
        assert!(is_ios_backup_folder_name(
            "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b"
        ));
        assert!(is_ios_backup_folder_name("00008120-001A21D11E88002E"));
        assert!(!is_ios_backup_folder_name("Backup"));
        assert!(!is_ios_backup_folder_name(".."));
        assert!(!is_ios_backup_folder_name("foo/bar"));
        assert!(!is_ios_backup_folder_name("not-a-udid"));
    }

    #[test]
    fn ios_backup_requires_complete_folder_under_backup() {
        let home = std::env::temp_dir().join(format!(
            "mc-ios-home-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let udid = "00008120-001A21D11E88002E";
        let bak = ios_backup_root(&home).join(udid);
        fs::create_dir_all(&bak).unwrap();
        assert!(!is_ios_backup_bundle_in(&bak, &home));
        fs::write(bak.join("Info.plist"), "<plist></plist>").unwrap();
        assert!(is_ios_backup_bundle_in(&bak, &home));
        assert!(!is_ios_backup_bundle_in(bak.parent().unwrap(), &home));
        assert!(!is_ios_backup_bundle_in(&bak.join("Info.plist"), &home));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn in_progress_ios_backup_is_skipped() {
        let home = std::env::temp_dir().join(format!(
            "mc-ios-prog-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let udid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let bak = ios_backup_root(&home).join(udid);
        fs::create_dir_all(&bak).unwrap();
        fs::write(bak.join("Manifest.db"), b"x").unwrap();
        fs::write(
            bak.join("Status.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>SnapshotState</key><string>new</string></dict></plist>"#,
        )
        .unwrap();
        assert!(!is_ios_backup_bundle_in(&bak, &home));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn messages_attachments_allowlist() {
        let home = std::env::temp_dir().join(format!(
            "mc-msg-home-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let attach = messages_attachments_root(&home).join("aa").join("bb");
        fs::create_dir_all(&attach).unwrap();
        let file = attach.join("photo.jpg");
        fs::write(&file, b"hello-image").unwrap();
        let db = home.join("Library").join("Messages").join("chat.db");
        fs::write(&db, b"sqlite").unwrap();
        assert!(is_messages_attachment_file_in(&file, &home));
        assert!(!is_messages_attachment_file_in(&db, &home));
        assert!(is_protected_user_data_in(&db, &home));
        assert!(!is_protected_user_data_in(&file, &home));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn protected_rel_does_not_block_mail_downloads() {
        assert!(!matches_protected_rel("Library/Mail Downloads/x"));
        assert!(matches_protected_rel("Library/Mail/V10/MailData"));
        assert!(matches_protected_rel("Library/Keychains/login.keychain-db"));
        assert!(matches_protected_rel(
            "Pictures/Photos Library.photoslibrary/database/Photos.sqlite"
        ));
        assert!(matches_protected_rel(
            "Library/Mobile Documents/com~apple~CloudDocs/doc"
        ));
    }

    #[test]
    fn snapshot_reclaim_refuses_mount_points() {
        assert!(reclaim_tm_snapshot("/").is_err());
        assert!(reclaim_tm_snapshot("/System/Volumes/Data").is_err());
        assert!(reclaim_tm_snapshot("com.apple.os.update-abc").is_err());
    }

    fn unix_nanos() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
