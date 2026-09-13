use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::macos_space::{
    ios_backup_root, is_ios_backup_bundle, is_messages_attachment_file,
    list_tm_local_snapshot_dates, messages_attachments_root, plutil_extract_raw, tm_backup_running,
};
use crate::models::{Category, FileGroup, FileItem, ReclaimOp};
use crate::safety::{format_bytes, home_dir, list_children, safe_size};

use super::Scanner;

const MESSAGES_MIN_AGE_DAYS: u64 = 90;
const MESSAGES_MIN_SIZE: u64 = 512 * 1024;

pub struct LocalSnapshotScanner;

impl Scanner for LocalSnapshotScanner {
    fn name(&self) -> &'static str {
        "Time Machine Snapshots"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Listing Time Machine local snapshots…");
        let dates = list_tm_local_snapshot_dates();
        if dates.is_empty() {
            return Vec::new();
        }
        let running = tm_backup_running();
        dates
            .into_iter()
            .map(|date| {
                let display = pretty_snapshot_date(&date);
                let mut description = format!(
                    "Time Machine local restore point {display}. Reclaimed space is APFS unique blocks only — not a file delete. The sealed macOS snapshot is never listed."
                );
                if running {
                    description.push_str(
                        " Time Machine is running; delete will be refused until it finishes.",
                    );
                }
                FileGroup {
                    key: format!("tm-snap:{date}"),
                    category: Category::LocalSnapshots,
                    title: format!("Local snapshot {display}"),
                    description,
                    items: vec![FileItem {
                        path: PathBuf::from(format!("tmutil://local-snapshot/{date}")),
                        size: 0,
                        category: Category::LocalSnapshots,
                        reason: "tmutil deletelocalsnapshots (date only)".into(),
                        group_key: date.clone(),
                        op: ReclaimOp::TmLocalSnapshot { date },
                    }],
                }
            })
            .collect()
    }
}

pub struct IosBackupScanner;

impl Scanner for IosBackupScanner {
    fn name(&self) -> &'static str {
        "iOS Device Backups"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning iOS device backups…");
        let root = ios_backup_root(&home_dir());
        if !root.is_dir() {
            return Vec::new();
        }
        list_children(&root)
            .into_iter()
            .filter(|child| is_ios_backup_bundle(child))
            .filter_map(|child| {
                let size = safe_size(&child);
                if size < 1024 * 1024 {
                    return None;
                }
                let info = child.join("Info.plist");
                let device = plutil_extract_raw(&info, "Device Name")
                    .or_else(|| plutil_extract_raw(&info, "Display Name"))
                    .unwrap_or_else(|| {
                        child
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "iOS backup".into())
                    });
                let product = plutil_extract_raw(&info, "Product Name").unwrap_or_default();
                let version = plutil_extract_raw(&info, "Product Version").unwrap_or_default();
                let when = plutil_extract_raw(&info, "Last Backup Date").unwrap_or_default();
                let mut meta = Vec::new();
                if !product.is_empty() {
                    meta.push(product);
                }
                if !version.is_empty() {
                    meta.push(format!("iOS {version}"));
                }
                if !when.is_empty() {
                    meta.push(when);
                }
                let detail = if meta.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", meta.join(" · "))
                };
                Some(FileGroup {
                    key: format!("ios-bak:{}", child.display()),
                    category: Category::IosBackups,
                    title: device.clone(),
                    description: format!(
                        "Finder backup of this device ({}){detail}. Deleting removes the backup on this Mac only — not the device and not macOS.",
                        format_bytes(size)
                    ),
                    items: vec![FileItem::file(
                        child,
                        size,
                        Category::IosBackups,
                        "Complete iOS backup folder",
                        device,
                    )],
                })
            })
            .collect()
    }
}

pub struct MessagesAttachmentScanner;

impl Scanner for MessagesAttachmentScanner {
    fn name(&self) -> &'static str {
        "Messages Attachments"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Scanning Messages attachments…");
        let root = messages_attachments_root(&home_dir());
        let Ok(_rd) = fs::read_dir(&root) else {
            return Vec::new();
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now.saturating_sub(MESSAGES_MIN_AGE_DAYS * 86400);

        let mut by_month: BTreeMap<String, Vec<FileItem>> = BTreeMap::new();

        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !is_messages_attachment_file(path) {
                continue;
            }
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.len() < MESSAGES_MIN_SIZE {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(now);
            if mtime > cutoff {
                continue;
            }
            let age_days = now.saturating_sub(mtime) / 86400;
            let month = chrono_yyyymm(mtime);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            by_month.entry(month).or_default().push(FileItem::file(
                path.to_path_buf(),
                meta.len(),
                Category::MessagesAttachments,
                format!("Messages attachment unused ~{age_days}d"),
                name,
            ));
        }

        by_month
            .into_iter()
            .filter_map(|(month, items)| {
                if items.is_empty() {
                    return None;
                }
                let size: u64 = items.iter().map(|i| i.size).sum();
                Some(FileGroup {
                    key: format!("imsg:{month}"),
                    category: Category::MessagesAttachments,
                    title: format!("Attachments {month}"),
                    description: format!(
                        "{} · {} files, at least {MESSAGES_MIN_AGE_DAYS} days old. chat.db is never deleted.",
                        format_bytes(size),
                        items.len()
                    ),
                    items,
                })
            })
            .collect()
    }
}

fn pretty_snapshot_date(date: &str) -> String {
    if date.len() == 17 {
        format!("{} {}", &date[..10], &date[11..])
    } else {
        date.to_string()
    }
}

fn chrono_yyyymm(unix: u64) -> String {
    let days = unix / 86400;
    let (year, month, _) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
