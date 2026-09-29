use crate::login_items::{
    is_stale_open_at_login, list_open_at_login, push_warning, stale_launch_jobs,
};
use crate::models::{Category, FileGroup, FileItem, ReclaimOp};
use crate::safety::safe_size;

use super::orphaned::installed_apps;
use super::Scanner;

pub struct LoginItemsScanner {
    /// Reading "Open at Login" goes through System Events and can trigger a
    /// one-time macOS Automation prompt, so the fast scan skips it.
    pub open_at_login: bool,
}

impl Scanner for LoginItemsScanner {
    fn name(&self) -> &'static str {
        "Login Items"
    }

    fn scan(&self, progress: &mut dyn FnMut(&str)) -> Vec<FileGroup> {
        progress("Checking launch agents and daemons…");
        let installed = installed_apps();
        let mut groups: Vec<FileGroup> = stale_launch_jobs(&installed)
            .into_iter()
            .map(|stale| {
                let job = &stale.job;
                let mut size = safe_size(&job.plist);
                let mut reason = stale.reason.clone();
                if let Some(helper) = &stale.helper {
                    size += safe_size(helper);
                    reason.push_str(&format!(". Also removes helper {}", helper.display()));
                }
                let admin = if job.dir.needs_admin() {
                    " macOS will ask for your administrator password."
                } else {
                    ""
                };
                FileGroup {
                    key: format!("login:job:{}", job.plist.display()),
                    category: Category::LoginItems,
                    title: job.label.clone(),
                    description: format!(
                        "Background item in {}. {}. Removing stops it and deletes its launchd plist.{admin}",
                        job.dir.describe(),
                        stale.reason
                    ),
                    items: vec![FileItem {
                        path: job.plist.clone(),
                        size,
                        category: Category::LoginItems,
                        reason,
                        group_key: job.label.clone(),
                        op: ReclaimOp::LaunchJob {
                            label: job.label.clone(),
                            helper: stale.helper.clone(),
                        },
                    }],
                }
            })
            .collect();

        if self.open_at_login {
            progress("Checking Open at Login items…");
            match list_open_at_login() {
                Ok(items) => groups.extend(items.into_iter().filter(is_stale_open_at_login).map(
                    |item| FileGroup {
                        key: format!("login:open:{}", item.name),
                        category: Category::LoginItems,
                        title: format!("{} (Open at Login)", item.name),
                        description: format!(
                            "Opens at login, but {} no longer exists. Removing only drops the entry from the Open at Login list.",
                            item.path.display()
                        ),
                        items: vec![FileItem {
                            path: item.path.clone(),
                            size: 0,
                            category: Category::LoginItems,
                            reason: "Open at Login entry for a missing app".into(),
                            group_key: item.name.clone(),
                            op: ReclaimOp::OpenAtLogin { name: item.name },
                        }],
                    },
                )),
                Err(message) => push_warning(message),
            }
        }
        groups
    }
}
