use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::engine::{self, DeleteOutcome, ScanKind};
use crate::models::{Category, FileGroup, FileItem, ReclaimOp};
use crate::safety::format_bytes;

use super::effect::Command;
use super::message::Message;
use super::model::{Confirm, Modal, Model, PendingAction, Screen};

pub fn init(kind: ScanKind) -> (Model, Command) {
    let mut model = Model::new(kind);
    let command = begin_scan(&mut model);
    (model, command)
}

pub fn update(model: &mut Model, message: Message) -> Command {
    match message {
        Message::Key(key) => return on_key(model, key),
        Message::Select(index) => {
            if model.modal.is_none() && !model.is_busy() && index < model.current_len() {
                select(model, index);
            }
        }
        Message::ScanProgress {
            message,
            index,
            total,
        } => {
            model.scan_message = message;
            model.scan_index = index;
            model.scan_total = total.max(1);
        }
        Message::ScanFinished(result) => {
            model.result = result;
            finish_scan(model);
        }
        Message::ScanStopped => finish_scan(model),
        Message::DeleteProgress {
            message,
            done,
            total,
        } => {
            model.delete_message = message;
            model.delete_done = done;
            model.delete_total = total.max(1);
        }
        Message::DeleteFinished(outcome) => {
            restore_screen(model);
            after_delete(model, outcome);
        }
        Message::DeleteStopped => {
            restore_screen(model);
            model.status = "Deletion stopped unexpectedly.".into();
        }
    }
    Command::None
}

fn on_key(model: &mut Model, key: KeyEvent) -> Command {
    match model.modal.take() {
        Some(Modal::Help) => {
            if !matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | '?' | 'h')) {
                model.modal = Some(Modal::Help);
            }
            Command::None
        }
        Some(Modal::Confirm(confirm)) => on_confirm_key(model, confirm, key),
        None if model.is_busy() => {
            let abort = model.screen == Screen::Scanning
                && key.code == KeyCode::Char('q')
                && key.modifiers.contains(KeyModifiers::CONTROL);
            if abort {
                Command::Quit
            } else {
                Command::None
            }
        }
        None => on_screen_key(model, key),
    }
}

fn on_confirm_key(model: &mut Model, mut confirm: Confirm, key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') => confirm.yes = !confirm.yes,
        KeyCode::Char('y') => {
            confirm.yes = true;
            return apply_confirm(model, confirm);
        }
        KeyCode::Enter => return apply_confirm(model, confirm),
        KeyCode::Char('n') | KeyCode::Esc => return Command::None,
        _ => {}
    }
    model.modal = Some(Modal::Confirm(confirm));
    Command::None
}

fn on_screen_key(model: &mut Model, key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Char('q') => return Command::Quit,
        KeyCode::Char('r') => return begin_scan(model),
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('b') => return go_back(model),
        KeyCode::Char('?' | 'h') => model.modal = Some(Modal::Help),
        KeyCode::Up => move_selection(model, -1),
        KeyCode::Down | KeyCode::Char('j') => move_selection(model, 1),
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            keep_highlighted(model)
        }
        KeyCode::Char('K') => keep_highlighted(model),
        KeyCode::Char('k') => move_selection(model, -1),
        KeyCode::PageUp => move_selection(model, -10),
        KeyCode::PageDown => move_selection(model, 10),
        KeyCode::Home | KeyCode::Char('g') => select(model, 0),
        KeyCode::End | KeyCode::Char('G') => {
            let len = model.current_len();
            if len > 0 {
                select(model, len - 1);
            }
        }
        KeyCode::Enter => activate(model),
        KeyCode::Char(' ') => toggle_mark(model),
        KeyCode::Char('c') => model.marked.clear(),
        KeyCode::Char('d') => delete_shortcut(model),
        KeyCode::Char('a') => delete_all_in_category(model),
        _ => {}
    }
    Command::None
}

fn select(model: &mut Model, index: usize) {
    let len = model.current_len();
    model.selection = if len == 0 {
        None
    } else {
        Some(index.min(len - 1))
    };
}

fn move_selection(model: &mut Model, delta: isize) {
    let len = model.current_len();
    if len == 0 {
        return;
    }
    let cur = model.selected() as isize;
    let next = (cur + delta).rem_euclid(len as isize) as usize;
    select(model, next);
}

fn begin_scan(model: &mut Model) -> Command {
    model.screen = Screen::Scanning;
    model.modal = None;
    model.marked.clear();
    model.scan_message = "Starting…".into();
    model.scan_index = 0;
    model.scan_total = 1;
    Command::Scan(model.kind.clone())
}

fn finish_scan(model: &mut Model) {
    model.result.prune_empty();
    let summary = if model.result.groups.is_empty() {
        model.screen = Screen::Empty;
        "Nothing to clean — your Mac looks tidy.".to_string()
    } else {
        model.screen = Screen::Categories;
        select(model, 0);
        engine::summarize(&model.result)
    };
    model.status = if model.result.warnings.is_empty() {
        summary
    } else {
        format!("{summary}  {}", model.result.warnings.join("  "))
    };
}

fn go_back(model: &mut Model) -> Command {
    match model.screen {
        Screen::Files { category, .. } => {
            model.marked.clear();
            model.screen = Screen::Groups { category };
            select(model, 0);
        }
        Screen::Groups { .. } | Screen::Empty => {
            if model.result.categories_sorted().is_empty() {
                model.screen = Screen::Empty;
            } else {
                model.screen = Screen::Categories;
                select(model, 0);
            }
        }
        Screen::Categories => return Command::Quit,
        Screen::Scanning | Screen::Deleting => {}
    }
    Command::None
}

fn activate(model: &mut Model) {
    match model.screen {
        Screen::Categories => {
            let Some(category) = model
                .result
                .categories_sorted()
                .get(model.selected())
                .copied()
            else {
                return;
            };
            model.screen = Screen::Groups { category };
            select(model, 0);
        }
        Screen::Groups { category } => {
            let Some(group) = current_group(model) else {
                return;
            };
            let group_key = group.key.clone();
            model.marked.clear();
            model.screen = Screen::Files {
                category,
                group_key,
            };
            select(model, 0);
        }
        Screen::Files { .. } => delete_marked_or_current(model),
        Screen::Empty | Screen::Scanning | Screen::Deleting => {}
    }
}

/// The highlighted group on the groups screen, or the open group on the files screen.
fn current_group(model: &Model) -> Option<&FileGroup> {
    match &model.screen {
        Screen::Groups { category } => model
            .result
            .groups_in(*category)
            .get(model.selected())
            .copied(),
        Screen::Files { group_key, .. } => model.result.group(group_key),
        _ => None,
    }
}

fn toggle_mark(model: &mut Model) {
    let Screen::Files { .. } = model.screen else {
        return;
    };
    let idx = model.selected();
    if idx >= model.current_len() {
        return;
    }
    if !model.marked.remove(&idx) {
        model.marked.insert(idx);
    }
}

fn delete_shortcut(model: &mut Model) {
    match model.screen {
        Screen::Files { .. } if !model.marked.is_empty() => delete_marked_or_current(model),
        Screen::Files { .. } | Screen::Groups { .. } => delete_current_group(model),
        _ => {}
    }
}

fn ask(model: &mut Model, title: String, body: Vec<String>, action: PendingAction) {
    model.modal = Some(Modal::Confirm(Confirm {
        title,
        body,
        yes: false,
        action,
    }));
}

fn delete_marked_or_current(model: &mut Model) {
    let Screen::Files { .. } = model.screen else {
        return;
    };
    let Some(group) = current_group(model) else {
        return;
    };
    let mut indices: Vec<usize> = if model.marked.is_empty() {
        vec![model.selected()]
    } else {
        model.marked.iter().copied().collect()
    };
    indices.sort_unstable();
    let items: Vec<FileItem> = indices
        .iter()
        .filter_map(|&i| group.items.get(i).cloned())
        .collect();
    if items.is_empty() {
        return;
    }
    let size: u64 = items.iter().map(|i| i.size).sum();
    let mut body: Vec<String> = items
        .iter()
        .take(8)
        .map(|i| i.path.display().to_string())
        .collect();
    if items.len() > 8 {
        body.push(format!("… and {} more", items.len() - 8));
    }
    body.extend(reclaim_warnings(&items));
    let title = format!("Delete {} item(s) ({})?", items.len(), format_bytes(size));
    ask(
        model,
        title,
        body,
        PendingAction::Items { items, bytes: size },
    );
}

fn delete_current_group(model: &mut Model) {
    let Some(group) = current_group(model) else {
        return;
    };
    let count = group.count();
    let extra = if group.category == Category::Duplicates && count > 1 {
        " This removes EVERY copy, including the original."
    } else {
        ""
    };
    let title = format!(
        "Delete group «{}» ({}, {count} items)?{extra}",
        group.title,
        format_bytes(group.size())
    );
    let body = reclaim_warnings(&group.items);
    let action = PendingAction::Group(group.key.clone());
    ask(model, title, body, action);
}

fn delete_all_in_category(model: &mut Model) {
    let Screen::Groups { category } = model.screen else {
        return;
    };
    let groups = model.result.groups_in(category);
    if groups.is_empty() {
        return;
    }
    let size: u64 = groups.iter().map(|g| g.size()).sum();
    let items: Vec<FileItem> = groups
        .iter()
        .flat_map(|g| g.items.iter().cloned())
        .collect();
    let mut body = reclaim_warnings(&items);
    if matches!(
        category,
        Category::LocalSnapshots | Category::IosBackups | Category::LoginItems
    ) {
        body.insert(
            0,
            "Each item is re-checked against an allowlist before anything is removed.".into(),
        );
    }
    let title = format!(
        "Delete ALL {} groups in {} ({})?",
        groups.len(),
        category.label(),
        format_bytes(size)
    );
    ask(model, title, body, PendingAction::Category(category));
}

fn keep_highlighted(model: &mut Model) {
    let Screen::Files { .. } = model.screen else {
        return;
    };
    let Some(group) = current_group(model) else {
        return;
    };
    if group.items.len() < 2 {
        model.status = "Nothing else to delete — you already have a single file.".into();
        return;
    }
    let keep = model.selected().min(group.items.len() - 1);
    let keep_path = &group.items[keep].path;
    let keep_name = keep_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| keep_path.display().to_string());
    let items: Vec<FileItem> = group
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != keep)
        .map(|(_, item)| item.clone())
        .collect();
    let size: u64 = items.iter().map(|item| item.size).sum();
    let title = format!(
        "Keep {keep_name} and delete {} other copy(ies) ({})?",
        items.len(),
        format_bytes(size)
    );
    let body = items
        .iter()
        .map(|item| item.path.display().to_string())
        .take(8)
        .collect();
    ask(
        model,
        title,
        body,
        PendingAction::Items { items, bytes: size },
    );
}

fn apply_confirm(model: &mut Model, confirm: Confirm) -> Command {
    if !confirm.yes {
        return Command::None;
    }
    let (items, bytes) = match confirm.action {
        PendingAction::Items { items, bytes } => (items, bytes),
        PendingAction::Group(key) => {
            let group = model.result.group(&key);
            let bytes = group.map(|g| g.size()).unwrap_or(0);
            let items = group.map(|g| g.items.clone()).unwrap_or_default();
            (items, bytes)
        }
        PendingAction::Category(category) => {
            let groups = model.result.groups_in(category);
            let bytes: u64 = groups.iter().map(|g| g.size()).sum();
            let items: Vec<FileItem> = groups
                .into_iter()
                .flat_map(|g| g.items.iter().cloned())
                .collect();
            (items, bytes)
        }
    };
    begin_delete(model, items, bytes)
}

fn begin_delete(model: &mut Model, items: Vec<FileItem>, bytes: u64) -> Command {
    model.delete_message = "Starting…".into();
    model.delete_done = 0;
    model.delete_total = bytes.max(1);
    model.delete_started = Instant::now();
    model.pre_delete_screen = Some(std::mem::replace(&mut model.screen, Screen::Deleting));
    Command::Delete { items, bytes }
}

fn restore_screen(model: &mut Model) {
    if let Some(prev) = model.pre_delete_screen.take() {
        model.screen = prev;
    }
}

fn after_delete(model: &mut Model, outcome: DeleteOutcome) {
    model.marked.clear();
    model.result.forget_paths(&outcome.removed_paths);
    model.result.prune_empty();
    if outcome.removed > 0 {
        model.status = format!(
            "Deleted {} item(s), freed {}.",
            outcome.removed,
            format_bytes(outcome.freed)
        );
    } else if outcome.errors.is_empty() {
        model.status = "Nothing deleted.".into();
    }
    if !outcome.errors.is_empty() {
        let extra = outcome.errors.join(" · ");
        if model.status.is_empty() {
            model.status = extra;
        } else {
            model.status = format!("{}  {}", model.status, extra);
        }
    }

    let fallback = match &model.screen {
        Screen::Files {
            category,
            group_key,
        } if model.result.group(group_key).is_none() => {
            if model.result.groups_in(*category).is_empty() {
                Some(Screen::Categories)
            } else {
                Some(Screen::Groups {
                    category: *category,
                })
            }
        }
        Screen::Groups { category } if model.result.groups_in(*category).is_empty() => {
            Some(Screen::Categories)
        }
        _ => None,
    };
    if let Some(screen) = fallback {
        model.screen = screen;
        select(model, 0);
    } else {
        let index = model.selected();
        select(model, index);
    }
    if model.result.groups.is_empty() {
        model.screen = Screen::Empty;
    }
}

fn reclaim_warnings(items: &[FileItem]) -> Vec<String> {
    let mut extra = Vec::new();
    if items
        .iter()
        .any(|item| matches!(item.op, ReclaimOp::TmLocalSnapshot { .. }))
    {
        extra.push(
            "Calls tmutil deletelocalsnapshots with a date only. Does not delete macOS or the sealed boot snapshot.".into(),
        );
        extra.push(
            "You lose that local restore point. Unique APFS reclaim may be less than listed."
                .into(),
        );
    }
    if items
        .iter()
        .any(|item| item.category == Category::IosBackups)
    {
        extra.push(
            "Removes the Finder backup from this Mac only. The iPhone/iPad and macOS are untouched.".into(),
        );
        extra.push("You cannot restore that device from this backup afterward.".into());
    }
    if items
        .iter()
        .any(|item| item.category == Category::MessagesAttachments)
    {
        extra.push(
            "Removes old Messages attachment files. chat.db and other Messages databases are never touched.".into(),
        );
    }
    if items
        .iter()
        .any(|item| matches!(item.op, ReclaimOp::LaunchJob { .. }))
    {
        extra.push(
            "Stops each background job with launchctl, then deletes its launchd plist (and its privileged helper, if any).".into(),
        );
    }
    if items.iter().any(crate::login_items::needs_admin) {
        extra.push(
            "System-wide items need admin rights: macOS will show its password dialog once.".into(),
        );
    }
    if items
        .iter()
        .any(|item| matches!(item.op, ReclaimOp::OpenAtLogin { .. }))
    {
        extra.push(
            "Removes the Open at Login entry via System Events; macOS may ask to allow Automation."
                .into(),
        );
    }
    extra
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::models::ScanResult;

    // Open-at-login items always count as present, so tests need no real files.
    fn group(key: &str, category: Category, sizes: &[u64]) -> FileGroup {
        let items = sizes
            .iter()
            .enumerate()
            .map(|(i, &size)| FileItem {
                path: PathBuf::from(format!("/{key}/{i}")),
                size,
                category,
                reason: String::new(),
                group_key: key.into(),
                op: ReclaimOp::OpenAtLogin {
                    name: format!("{key}{i}"),
                },
            })
            .collect();
        FileGroup {
            key: key.into(),
            category,
            title: key.into(),
            description: String::new(),
            items,
        }
    }

    /// Caches holds groups "a" (600 bytes) and "b" (100); Logs holds "c" (50).
    fn scanned() -> Model {
        let (mut model, _) = init(ScanKind::Smart);
        let result = ScanResult {
            groups: vec![
                group("a", Category::UserCache, &[300, 300]),
                group("b", Category::UserCache, &[100]),
                group("c", Category::Logs, &[50]),
            ],
            ..ScanResult::default()
        };
        update(&mut model, Message::ScanFinished(result));
        model
    }

    fn press(model: &mut Model, code: KeyCode) -> Command {
        update(model, Message::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn removed(model: &Model, key: &str) -> DeleteOutcome {
        let paths: Vec<PathBuf> = model
            .result
            .group(key)
            .unwrap()
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect();
        DeleteOutcome {
            removed: paths.len(),
            removed_paths: paths,
            ..DeleteOutcome::default()
        }
    }

    #[test]
    fn init_starts_a_scan() {
        let (model, command) = init(ScanKind::Smart);
        assert_eq!(model.screen, Screen::Scanning);
        assert!(matches!(command, Command::Scan(ScanKind::Smart)));
    }

    #[test]
    fn scan_finished_opens_categories() {
        let model = scanned();
        assert_eq!(model.screen, Screen::Categories);
        assert_eq!(model.selection, Some(0));
        assert!(model.status.starts_with("Found 4 items in 3 groups"));
    }

    #[test]
    fn empty_scan_shows_empty_screen() {
        let (mut model, _) = init(ScanKind::Smart);
        update(&mut model, Message::ScanFinished(ScanResult::default()));
        assert_eq!(model.screen, Screen::Empty);
        assert_eq!(model.status, "Nothing to clean — your Mac looks tidy.");
    }

    #[test]
    fn enter_drills_down_and_esc_backs_out() {
        let mut model = scanned();
        let caches = Category::UserCache;

        press(&mut model, KeyCode::Enter);
        assert_eq!(model.screen, Screen::Groups { category: caches });

        press(&mut model, KeyCode::Enter);
        assert_eq!(
            model.screen,
            Screen::Files {
                category: caches,
                group_key: "a".into()
            }
        );

        press(&mut model, KeyCode::Esc);
        assert_eq!(model.screen, Screen::Groups { category: caches });
        press(&mut model, KeyCode::Esc);
        assert_eq!(model.screen, Screen::Categories);
        assert!(matches!(press(&mut model, KeyCode::Esc), Command::Quit));
    }

    #[test]
    fn selection_wraps_and_clicks_are_bounded() {
        let mut model = scanned();
        press(&mut model, KeyCode::Up);
        assert_eq!(model.selection, Some(1));
        update(&mut model, Message::Select(9));
        assert_eq!(model.selection, Some(1));
        update(&mut model, Message::Select(0));
        assert_eq!(model.selection, Some(0));
    }

    #[test]
    fn help_swallows_keys_until_closed() {
        let mut model = scanned();
        press(&mut model, KeyCode::Char('?'));
        assert!(matches!(model.modal, Some(Modal::Help)));
        assert!(matches!(press(&mut model, KeyCode::Enter), Command::None));
        assert_eq!(model.screen, Screen::Categories);
        press(&mut model, KeyCode::Esc);
        assert!(model.modal.is_none());
    }

    #[test]
    fn confirm_cancel_keeps_screen() {
        let mut model = scanned();
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Char('d'));
        assert!(matches!(model.modal, Some(Modal::Confirm(_))));

        let command = press(&mut model, KeyCode::Char('n'));
        assert!(matches!(command, Command::None));
        assert!(model.modal.is_none());
        assert_eq!(
            model.screen,
            Screen::Groups {
                category: Category::UserCache
            }
        );
    }

    #[test]
    fn confirm_enter_defaults_to_no() {
        let mut model = scanned();
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Char('d'));
        assert!(matches!(press(&mut model, KeyCode::Enter), Command::None));
        assert!(model.modal.is_none());
    }

    #[test]
    fn confirm_yes_requests_delete() {
        let mut model = scanned();
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Char('d'));

        let command = press(&mut model, KeyCode::Char('y'));
        let Command::Delete { items, bytes } = command else {
            panic!("expected a delete command, got {command:?}");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(bytes, 600);
        assert_eq!(model.screen, Screen::Deleting);
        assert!(model.modal.is_none());
    }

    #[test]
    fn delete_finished_returns_to_groups_when_group_is_gone() {
        let mut model = scanned();
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Char('d'));
        press(&mut model, KeyCode::Char('y'));

        let outcome = removed(&model, "a");
        update(&mut model, Message::DeleteFinished(outcome));
        assert_eq!(
            model.screen,
            Screen::Groups {
                category: Category::UserCache
            }
        );
        assert!(model.result.group("a").is_none());
        assert!(model.status.starts_with("Deleted 2 item(s)"));
    }

    #[test]
    fn delete_finished_returns_to_categories_when_category_is_empty() {
        let mut model = scanned();
        press(&mut model, KeyCode::Down);
        press(&mut model, KeyCode::Enter);
        assert_eq!(
            model.screen,
            Screen::Groups {
                category: Category::Logs
            }
        );
        press(&mut model, KeyCode::Char('a'));
        press(&mut model, KeyCode::Char('y'));

        let outcome = removed(&model, "c");
        update(&mut model, Message::DeleteFinished(outcome));
        assert_eq!(model.screen, Screen::Categories);
    }

    #[test]
    fn delete_stopped_restores_screen() {
        let mut model = scanned();
        press(&mut model, KeyCode::Enter);
        press(&mut model, KeyCode::Char('d'));
        press(&mut model, KeyCode::Char('y'));

        update(&mut model, Message::DeleteStopped);
        assert_eq!(
            model.screen,
            Screen::Groups {
                category: Category::UserCache
            }
        );
        assert_eq!(model.status, "Deletion stopped unexpectedly.");
    }
}
