//! Stale login items: launchd jobs and "Open at Login" entries whose app is gone.
//!
//! Only `.plist` files directly inside the three launchd folders below are ever
//! removed, plus the job's own helper in `/Library/PrivilegedHelperTools`.
//! Apple jobs are never listed. Every item is re-validated right before
//! removal. System-wide removals run in a single
//! `do shell script … with administrator privileges` call, so macOS shows its
//! own password dialog once per cleanup; mac-cleaner never sees the password.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::macos_space::plutil_extract_raw;
use crate::models::{FileItem, ReclaimOp};
use crate::safety::home_dir;
use crate::scanners::orphaned::{installed_apps, likely_installed, InstalledApps};

const LAUNCHCTL: &str = "/bin/launchctl";
const OSASCRIPT: &str = "/usr/bin/osascript";
const MDFIND: &str = "/usr/bin/mdfind";
const HELPER_DIR: &str = "/Library/PrivilegedHelperTools";
const OSASCRIPT_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_BUNDLE_IDS: usize = 16;

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchDir {
    UserAgents,
    SystemAgents,
    SystemDaemons,
}

impl LaunchDir {
    pub const ALL: [LaunchDir; 3] = [
        LaunchDir::UserAgents,
        LaunchDir::SystemAgents,
        LaunchDir::SystemDaemons,
    ];

    pub fn path(self) -> PathBuf {
        match self {
            LaunchDir::UserAgents => home_dir().join("Library").join("LaunchAgents"),
            LaunchDir::SystemAgents => PathBuf::from("/Library/LaunchAgents"),
            LaunchDir::SystemDaemons => PathBuf::from("/Library/LaunchDaemons"),
        }
    }

    pub fn needs_admin(self) -> bool {
        !matches!(self, LaunchDir::UserAgents)
    }

    pub fn describe(self) -> &'static str {
        match self {
            LaunchDir::UserAgents => "your launch agents",
            LaunchDir::SystemAgents => "system-wide launch agents",
            LaunchDir::SystemDaemons => "system launch daemons",
        }
    }

    /// Which launchd folder directly contains `plist`, if any.
    pub fn of(plist: &Path) -> Option<LaunchDir> {
        let parent = plist.parent()?;
        let parent_c = parent.canonicalize().ok()?;
        LaunchDir::ALL.into_iter().find(|dir| {
            let p = dir.path();
            parent == p || p.canonicalize().is_ok_and(|c| c == parent_c)
        })
    }
}

#[derive(Debug, Clone)]
pub struct LaunchJob {
    pub plist: PathBuf,
    pub dir: LaunchDir,
    pub label: String,
    pub program: Option<PathBuf>,
    pub bundle_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StaleJob {
    pub job: LaunchJob,
    pub reason: String,
    pub helper: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAtLoginItem {
    pub name: String,
    pub path: PathBuf,
}

pub fn push_warning(message: impl Into<String>) {
    if let Ok(mut w) = WARNINGS.lock() {
        w.push(message.into());
    }
}

pub fn take_warnings() -> Vec<String> {
    WARNINGS
        .lock()
        .map(|mut w| std::mem::take(&mut *w))
        .unwrap_or_default()
}

pub fn is_login_item_op(op: &ReclaimOp) -> bool {
    matches!(
        op,
        ReclaimOp::LaunchJob { .. } | ReclaimOp::OpenAtLogin { .. }
    )
}

pub fn needs_admin(item: &FileItem) -> bool {
    matches!(item.op, ReclaimOp::LaunchJob { .. })
        && LaunchDir::of(&item.path).is_some_and(LaunchDir::needs_admin)
}

// ---------------------------------------------------------------- launchd jobs

pub fn stale_launch_jobs(installed: &InstalledApps) -> Vec<StaleJob> {
    let mut out = Vec::new();
    for dir in LaunchDir::ALL {
        let Ok(entries) = fs::read_dir(dir.path()) else {
            continue;
        };
        let mut plists: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        plists.sort();
        for plist in plists {
            let Some(job) = read_job(&plist, dir) else {
                continue;
            };
            if let Some(reason) = stale_reason(&job, installed) {
                let helper = helper_for(&job);
                out.push(StaleJob {
                    job,
                    reason,
                    helper,
                });
            }
        }
    }
    out
}

pub fn read_job(plist: &Path, dir: LaunchDir) -> Option<LaunchJob> {
    if plist.extension().and_then(|e| e.to_str()) != Some("plist") {
        return None;
    }
    let meta = plist.symlink_metadata().ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let label = plutil_extract_raw(plist, "Label")?;
    if !valid_label(&label) {
        return None;
    }
    let program = plutil_extract_raw(plist, "Program")
        .or_else(|| plutil_extract_raw(plist, "ProgramArguments.0"))
        .map(PathBuf::from);
    let bundle_ids = match plutil_extract_raw(plist, "AssociatedBundleIdentifiers") {
        Some(single) => vec![single],
        None => (0..MAX_BUNDLE_IDS)
            .map_while(|i| plutil_extract_raw(plist, &format!("AssociatedBundleIdentifiers.{i}")))
            .collect(),
    };
    Some(LaunchJob {
        plist: plist.to_path_buf(),
        dir,
        label,
        program,
        bundle_ids,
    })
}

pub fn stale_reason(job: &LaunchJob, installed: &InstalledApps) -> Option<String> {
    let program_exists = job
        .program
        .as_deref()
        .map_or(true, |p| p.symlink_metadata().is_ok());
    let app_installed = |id: &str| installed.has_bundle_id(id) || spotlight_has_app(id);
    let vendor_installed = |label: &str| {
        likely_installed(label, installed) || vendor_prefix(label).is_some_and(spotlight_has_vendor)
    };
    classify(job, program_exists, &app_installed, &vendor_installed)
}

fn classify(
    job: &LaunchJob,
    program_exists: bool,
    app_installed: &dyn Fn(&str) -> bool,
    vendor_installed: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if is_apple_job(job) {
        return None;
    }
    if let Some(program) = &job.program {
        if program.is_absolute() && !program.starts_with("/Volumes") && !program_exists {
            return Some(format!(
                "Its program no longer exists: {}",
                program.display()
            ));
        }
    }
    if !job.bundle_ids.is_empty() {
        if job.bundle_ids.iter().any(|id| app_installed(id)) {
            return None;
        }
        return Some(format!(
            "Belongs to {}, which is not installed",
            job.bundle_ids.join(", ")
        ));
    }
    let is_helper = job
        .program
        .as_deref()
        .is_some_and(|p| p.starts_with(HELPER_DIR));
    if is_helper && !vendor_installed(&job.label) {
        return Some("Privileged helper for an app that is no longer installed".into());
    }
    None
}

fn is_apple_job(job: &LaunchJob) -> bool {
    let label = job.label.to_ascii_lowercase();
    if label.starts_with("com.apple.") {
        return true;
    }
    job.program.as_deref().is_some_and(|p| {
        p.starts_with("/System") || (p.starts_with("/usr") && !p.starts_with("/usr/local"))
    })
}

/// The job's helper binary, only when it sits directly in PrivilegedHelperTools.
fn helper_for(job: &LaunchJob) -> Option<PathBuf> {
    if !job.dir.needs_admin() {
        return None;
    }
    let program = job.program.as_deref()?;
    if program.parent() != Some(Path::new(HELPER_DIR)) {
        return None;
    }
    let meta = program.symlink_metadata().ok()?;
    (meta.is_file() && !meta.file_type().is_symlink()).then(|| program.to_path_buf())
}

pub fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 255
        && !label.starts_with(['-', '.'])
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// `com.docker.vmnetd` → `com.docker`.
fn vendor_prefix(label: &str) -> Option<String> {
    let parts: Vec<&str> = label.split('.').collect();
    (parts.len() >= 3 && parts[..2].iter().all(|p| !p.is_empty()))
        .then(|| format!("{}.{}", parts[0], parts[1]))
}

fn spotlight_has_app(bundle_id: &str) -> bool {
    valid_label(bundle_id) && spotlight_query_hits(bundle_id)
}

fn spotlight_has_vendor(prefix: String) -> bool {
    valid_label(&prefix) && spotlight_query_hits(&format!("{prefix}.*"))
}

fn spotlight_query_hits(bundle_glob: &str) -> bool {
    let query = format!(
        "kMDItemContentType == 'com.apple.application-bundle' && kMDItemCFBundleIdentifier == '{bundle_glob}'c"
    );
    Command::new(MDFIND)
        .arg(query)
        .output()
        .is_ok_and(|o| o.status.success() && o.stdout.iter().any(|b| !b.is_ascii_whitespace()))
}

// ------------------------------------------------------------- Open at Login

const LIST_OPEN_AT_LOGIN: &[&str] = &[
    "set out to \"\"",
    "tell application \"System Events\"",
    "repeat with li in every login item",
    "set p to \"\"",
    "try",
    "set p to path of li",
    "end try",
    "set out to out & (name of li) & tab & p & linefeed",
    "end repeat",
    "end tell",
    "return out",
];

const AUTOMATION_HINT: &str = "Open at Login items skipped — allow your terminal to control System Events in System Settings › Privacy & Security › Automation.";

pub fn list_open_at_login() -> std::result::Result<Vec<OpenAtLoginItem>, String> {
    let mut cmd = Command::new(OSASCRIPT);
    for line in LIST_OPEN_AT_LOGIN {
        cmd.arg("-e").arg(line);
    }
    let Some(output) = output_with_timeout(&mut cmd, OSASCRIPT_TIMEOUT) else {
        return Err("Open at Login items skipped — System Events did not answer in time.".into());
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("-1743") || err.contains("-1744") {
            return Err(AUTOMATION_HINT.into());
        }
        return Err(format!("Open at Login items skipped — {}", err.trim()));
    }
    Ok(parse_open_at_login(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_open_at_login(out: &str) -> Vec<OpenAtLoginItem> {
    out.lines()
        .filter_map(|line| {
            let (name, path) = line.split_once('\t')?;
            let name = name.trim();
            (!name.is_empty()).then(|| OpenAtLoginItem {
                name: name.to_string(),
                path: PathBuf::from(path.trim()),
            })
        })
        .collect()
}

pub fn is_stale_open_at_login(item: &OpenAtLoginItem) -> bool {
    item.path.is_absolute()
        && !item.path.starts_with("/Volumes")
        && item.path.symlink_metadata().is_err()
}

// ------------------------------------------------------------------- removal

/// Remove login items after re-validating each one. Returns `(path, bytes freed)`
/// per item. All admin-only jobs share one macOS password dialog.
pub fn remove(items: &[FileItem]) -> Vec<(PathBuf, Result<u64>)> {
    let mut results = Vec::new();
    let mut admin: Vec<(StaleJob, u64)> = Vec::new();
    let mut installed: Option<InstalledApps> = None;
    let mut open_at_login: Option<std::result::Result<Vec<OpenAtLoginItem>, String>> = None;

    for item in items {
        match &item.op {
            ReclaimOp::LaunchJob { label, helper } => {
                if item.path.symlink_metadata().is_err() {
                    results.push((item.path.clone(), Ok(0)));
                    continue;
                }
                let installed = installed.get_or_insert_with(installed_apps);
                match revalidate_job(&item.path, label, helper.as_deref(), installed) {
                    Ok(stale) if stale.job.dir.needs_admin() => admin.push((stale, item.size)),
                    Ok(stale) => {
                        results.push((item.path.clone(), remove_user_job(&stale, item.size)))
                    }
                    Err(err) => results.push((item.path.clone(), Err(err))),
                }
            }
            ReclaimOp::OpenAtLogin { name } => {
                let listed = open_at_login.get_or_insert_with(list_open_at_login);
                results.push((item.path.clone(), remove_open_at_login(name, listed)));
            }
            _ => results.push((
                item.path.clone(),
                Err(anyhow::anyhow!("Refused (not a login item)")),
            )),
        }
    }

    if !admin.is_empty() {
        results.extend(remove_admin_jobs(&admin));
    }
    results
}

fn revalidate_job(
    plist: &Path,
    label: &str,
    helper: Option<&Path>,
    installed: &InstalledApps,
) -> Result<StaleJob> {
    let Some(dir) = LaunchDir::of(plist) else {
        bail!("Refused (not directly inside a launchd folder)");
    };
    let Some(job) = read_job(plist, dir) else {
        bail!("Refused (not a readable launchd plist)");
    };
    if job.label != label {
        bail!("Refused (job label changed since the scan)");
    }
    let Some(reason) = stale_reason(&job, installed) else {
        bail!("Refused (its app looks installed now)");
    };
    let current_helper = helper_for(&job);
    if current_helper.as_deref() != helper {
        bail!("Refused (helper changed since the scan)");
    }
    Ok(StaleJob {
        job,
        reason,
        helper: current_helper,
    })
}

fn remove_user_job(stale: &StaleJob, size: u64) -> Result<u64> {
    let uid = unsafe { libc::getuid() };
    let _ = Command::new(LAUNCHCTL)
        .arg("bootout")
        .arg(format!("gui/{uid}/{}", stale.job.label))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    fs::remove_file(&stale.job.plist)?;
    Ok(size)
}

fn remove_admin_jobs(jobs: &[(StaleJob, u64)]) -> Vec<(PathBuf, Result<u64>)> {
    let uid = unsafe { libc::getuid() };
    let script = admin_script(jobs.iter().map(|(s, _)| s), uid);
    let run = run_admin_script(&script);
    jobs.iter()
        .map(|(stale, size)| {
            let path = stale.job.plist.clone();
            let result = match &run {
                Err(err) => Err(anyhow::anyhow!("{err}")),
                Ok(()) if path.symlink_metadata().is_ok() => {
                    Err(anyhow::anyhow!("still present after removal"))
                }
                Ok(()) => Ok(*size),
            };
            (path, result)
        })
        .collect()
}

fn admin_script<'a>(jobs: impl Iterator<Item = &'a StaleJob>, uid: u32) -> String {
    let mut lines = Vec::new();
    for stale in jobs {
        let target = match stale.job.dir {
            LaunchDir::SystemDaemons => format!("system/{}", stale.job.label),
            _ => format!("gui/{uid}/{}", stale.job.label),
        };
        lines.push(format!(
            "{LAUNCHCTL} bootout {} >/dev/null 2>&1",
            sh_quote(&target)
        ));
        lines.push(format!(
            "/bin/rm -f {}",
            sh_quote(&stale.job.plist.to_string_lossy())
        ));
        if let Some(helper) = &stale.helper {
            lines.push(format!(
                "/bin/rm -f {}",
                sh_quote(&helper.to_string_lossy())
            ));
        }
    }
    lines.push("exit 0".into());
    lines.join("; ")
}

fn run_admin_script(script: &str) -> Result<()> {
    let mut cmd = Command::new(OSASCRIPT);
    cmd.args([
        "-e",
        "on run argv",
        "-e",
        "do shell script (item 1 of argv) with administrator privileges",
        "-e",
        "end run",
        script,
    ]);
    let Some(output) = output_with_timeout(&mut cmd, Duration::from_secs(300)) else {
        bail!("Timed out waiting for the administrator password");
    };
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr);
    if err.contains("-128") {
        bail!("Administrator password was cancelled; nothing removed");
    }
    bail!("Admin removal failed: {}", err.trim());
}

fn remove_open_at_login(
    name: &str,
    listed: &std::result::Result<Vec<OpenAtLoginItem>, String>,
) -> Result<u64> {
    let listed = match listed {
        Ok(items) => items,
        Err(err) => bail!("{err}"),
    };
    if name.starts_with('-') {
        bail!("Refused (unexpected login item name)");
    }
    let Some(item) = listed.iter().find(|i| i.name == name) else {
        return Ok(0);
    };
    if !is_stale_open_at_login(item) {
        bail!("Refused (its app exists again at {})", item.path.display());
    }
    let mut cmd = Command::new(OSASCRIPT);
    cmd.args([
        "-e",
        "on run argv",
        "-e",
        "tell application \"System Events\" to delete login item (item 1 of argv)",
        "-e",
        "end run",
        name,
    ]);
    let Some(output) = output_with_timeout(&mut cmd, OSASCRIPT_TIMEOUT) else {
        bail!("System Events did not answer in time");
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("-1743") || err.contains("-1744") {
            bail!("{AUTOMATION_HINT}");
        }
        bail!("System Events refused: {}", err.trim());
    }
    Ok(0)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(label: &str, program: Option<&str>, ids: &[&str], dir: LaunchDir) -> LaunchJob {
        LaunchJob {
            plist: PathBuf::from(format!("/Library/LaunchDaemons/{label}.plist")),
            dir,
            label: label.into(),
            program: program.map(PathBuf::from),
            bundle_ids: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    const YES: &dyn Fn(&str) -> bool = &|_| true;
    const NO: &dyn Fn(&str) -> bool = &|_| false;

    #[test]
    fn missing_program_is_stale() {
        let j = job(
            "com.ghost.agent",
            Some("/Applications/Ghost.app/Contents/MacOS/agent"),
            &[],
            LaunchDir::UserAgents,
        );
        assert!(classify(&j, false, YES, YES).is_some());
        assert!(classify(&j, true, YES, YES).is_none());
    }

    #[test]
    fn uninstalled_associated_app_is_stale() {
        let j = job(
            "com.docker.socket",
            Some("/Library/PrivilegedHelperTools/com.docker.socket"),
            &["com.docker.docker"],
            LaunchDir::SystemDaemons,
        );
        let reason = classify(&j, true, NO, YES).unwrap();
        assert!(reason.contains("com.docker.docker"));
        assert!(classify(&j, true, YES, NO).is_none());
    }

    #[test]
    fn orphan_privileged_helper_is_stale() {
        let j = job(
            "com.docker.vmnetd",
            Some("/Library/PrivilegedHelperTools/com.docker.vmnetd"),
            &[],
            LaunchDir::SystemDaemons,
        );
        assert!(classify(&j, true, YES, NO).is_some());
        assert!(classify(&j, true, YES, YES).is_none());
    }

    #[test]
    fn apple_and_cli_tool_jobs_are_left_alone() {
        let apple = job(
            "com.apple.something",
            Some("/opt/missing"),
            &[],
            LaunchDir::SystemDaemons,
        );
        assert!(classify(&apple, false, NO, NO).is_none());
        let libexec = job(
            "org.example.x",
            Some("/usr/libexec/x"),
            &[],
            LaunchDir::SystemDaemons,
        );
        assert!(classify(&libexec, false, NO, NO).is_none());
        let brew = job(
            "homebrew.mxcl.postgresql",
            Some("/opt/homebrew/opt/postgresql/bin/postgres"),
            &[],
            LaunchDir::UserAgents,
        );
        assert!(classify(&brew, true, NO, NO).is_none());
        let offline = job(
            "com.x.y",
            Some("/Volumes/External/tool"),
            &[],
            LaunchDir::UserAgents,
        );
        assert!(classify(&offline, false, YES, YES).is_none());
    }

    #[test]
    fn labels_and_quoting() {
        assert!(valid_label("com.docker.vmnetd"));
        assert!(!valid_label("com.x; rm -rf /"));
        assert!(!valid_label("-flag"));
        assert!(!valid_label(""));
        assert_eq!(sh_quote("/a b/it's"), "'/a b/it'\\''s'");
        assert_eq!(
            vendor_prefix("com.docker.vmnetd").as_deref(),
            Some("com.docker")
        );
        assert_eq!(vendor_prefix("vmnetd"), None);
    }

    #[test]
    fn admin_script_only_touches_listed_paths() {
        let stale = StaleJob {
            job: job(
                "com.docker.vmnetd",
                Some("/Library/PrivilegedHelperTools/com.docker.vmnetd"),
                &[],
                LaunchDir::SystemDaemons,
            ),
            reason: String::new(),
            helper: Some(PathBuf::from(
                "/Library/PrivilegedHelperTools/com.docker.vmnetd",
            )),
        };
        let script = admin_script(std::iter::once(&stale), 501);
        assert_eq!(
            script,
            "/bin/launchctl bootout 'system/com.docker.vmnetd' >/dev/null 2>&1; \
             /bin/rm -f '/Library/LaunchDaemons/com.docker.vmnetd.plist'; \
             /bin/rm -f '/Library/PrivilegedHelperTools/com.docker.vmnetd'; exit 0"
        );
    }

    #[test]
    fn parses_open_at_login_listing() {
        let items = parse_open_at_login("Ghost\t/Applications/Ghost.app\nNoPath\t\n\n");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "Ghost");
        assert!(is_stale_open_at_login(&OpenAtLoginItem {
            name: "Ghost".into(),
            path: PathBuf::from("/Applications/Definitely-Not-Here-mc.app"),
        }));
        assert!(!is_stale_open_at_login(&items[1]));
    }

    #[test]
    fn remove_refuses_plist_outside_launchd_folders() {
        let dir = std::env::temp_dir().join(format!("mc-login-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let plist = dir.join("com.ghost.agent.plist");
        fs::write(&plist, "<plist></plist>").unwrap();
        let item = FileItem {
            path: plist.clone(),
            size: 1,
            category: crate::models::Category::LoginItems,
            reason: String::new(),
            group_key: String::new(),
            op: ReclaimOp::LaunchJob {
                label: "com.ghost.agent".into(),
                helper: None,
            },
        };
        let results = remove(std::slice::from_ref(&item));
        assert!(results[0].1.is_err());
        assert!(plist.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
