use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::engine::{self, DeleteOutcome, ScanKind};
use crate::models::{Category, ScanResult};
use crate::safety::format_bytes;

pub enum Screen {
    Scanning,
    Empty,
    Categories,
    Groups {
        category: Category,
    },
    Files {
        category: Category,
        group_key: String,
    },
}

pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub yes: bool,
    pub action: PendingAction,
}

pub enum PendingAction {
    Paths(Vec<PathBuf>),
    Group(String),
    Category(Category),
}

pub enum ScanEvent {
    Progress {
        message: String,
        index: usize,
        total: usize,
    },
    Done(ScanResult),
}

pub struct App {
    pub kind: ScanKind,
    pub result: ScanResult,
    pub screen: Screen,
    pub list_state: ListState,
    pub marked: HashSet<usize>,
    pub confirm: Option<Confirm>,
    pub help: bool,
    pub status: String,
    pub scan_message: String,
    pub scan_index: usize,
    pub scan_total: usize,
    pub list_area: Rect,
    pub should_quit: bool,
    rx: Option<Receiver<ScanEvent>>,
}

impl App {
    pub fn start(kind: ScanKind) -> Self {
        let mut app = Self {
            kind,
            result: ScanResult::default(),
            screen: Screen::Scanning,
            list_state: ListState::default().with_selected(Some(0)),
            marked: HashSet::new(),
            confirm: None,
            help: false,
            status: String::new(),
            scan_message: "Starting…".into(),
            scan_index: 0,
            scan_total: 1,
            list_area: Rect::default(),
            should_quit: false,
            rx: None,
        };
        app.begin_scan();
        app
    }

    fn begin_scan(&mut self) {
        self.screen = Screen::Scanning;
        self.help = false;
        self.confirm = None;
        self.marked.clear();
        self.scan_message = "Starting…".into();
        self.scan_index = 0;
        self.scan_total = 1;
        let kind = self.kind.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let tx_progress = tx.clone();
            let result = engine::scan(kind, &mut |message, index, total| {
                let _ = tx_progress.send(ScanEvent::Progress {
                    message: message.to_string(),
                    index,
                    total,
                });
            });
            let _ = tx.send(ScanEvent::Done(result));
        });
        self.rx = Some(rx);
    }

    pub fn poll_scan(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(ScanEvent::Progress {
                    message,
                    index,
                    total,
                }) => {
                    self.scan_message = message;
                    self.scan_index = index;
                    self.scan_total = total.max(1);
                }
                Ok(ScanEvent::Done(result)) => {
                    self.result = result;
                    self.finish_scan();
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.finish_scan();
                    return;
                }
            }
        }
    }

    fn finish_scan(&mut self) {
        self.result.prune_empty();
        if self.result.groups.is_empty() {
            self.screen = Screen::Empty;
            self.status = "Nothing to clean — your Mac looks tidy.".into();
        } else {
            self.screen = Screen::Categories;
            self.select(0);
            self.status = engine::summarize(&self.result);
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self.kind {
            ScanKind::Full => "Full scan",
            ScanKind::Smart => "Smart scan",
            ScanKind::Duplicates { .. } => "Duplicates",
        }
    }

    pub fn current_len(&self) -> usize {
        match self.screen {
            Screen::Scanning | Screen::Empty => 0,
            Screen::Categories => self.result.categories_sorted().len(),
            Screen::Groups { category } => self.result.groups_in(category).len(),
            Screen::Files { ref group_key, .. } => self
                .result
                .group(group_key)
                .map(|g| g.items.len())
                .unwrap_or(0),
        }
    }

    pub fn selected(&self) -> usize {
        self.list_state.selected().unwrap_or(0)
    }

    fn select(&mut self, index: usize) {
        let len = self.current_len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        self.list_state.select(Some(index.min(len - 1)));
    }

    fn move_sel(&mut self, delta: isize) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let cur = self.selected() as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.select(next);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Char('h') => {
                    self.help = false;
                }
                _ => {}
            }
            return;
        }

        if let Some(confirm) = self.confirm.as_mut() {
            match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                    confirm.yes = !confirm.yes;
                }
                KeyCode::Char('y') => {
                    confirm.yes = true;
                    self.apply_confirm();
                }
                KeyCode::Char('n') | KeyCode::Esc => self.confirm = None,
                KeyCode::Enter => self.apply_confirm(),
                _ => {}
            }
            return;
        }

        if matches!(self.screen, Screen::Scanning) {
            if matches!(key.code, KeyCode::Char('q'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.should_quit = true;
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') | KeyCode::Char('h') => self.help = true,
            KeyCode::Char('r') => self.begin_scan(),
            KeyCode::Up => self.move_sel(-1),
            KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.keep_highlighted();
            }
            KeyCode::Char('K') => self.keep_highlighted(),
            KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Char('j') => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::Home | KeyCode::Char('g') => self.select(0),
            KeyCode::End | KeyCode::Char('G') => {
                let len = self.current_len();
                if len > 0 {
                    self.select(len - 1);
                }
            }
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('b') => self.go_back(),
            KeyCode::Enter => self.activate(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('c') => self.marked.clear(),
            KeyCode::Char('d') => self.delete_shortcut(),
            KeyCode::Char('a') => self.delete_all_in_category(),
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.help || self.confirm.is_some() || matches!(self.screen, Screen::Scanning) {
            return;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if let Some(index) = index_from_mouse(self.list_area, &self.list_state, mouse.row) {
            if index < self.current_len() {
                self.select(index);
            }
        }
    }

    fn go_back(&mut self) {
        match self.screen {
            Screen::Files { category, .. } => {
                self.marked.clear();
                self.screen = Screen::Groups { category };
                self.select(0);
            }
            Screen::Groups { .. } | Screen::Empty => {
                if self.result.categories_sorted().is_empty() {
                    self.screen = Screen::Empty;
                } else {
                    self.screen = Screen::Categories;
                    self.select(0);
                }
            }
            Screen::Categories => self.should_quit = true,
            Screen::Scanning => {}
        }
    }

    fn activate(&mut self) {
        match self.screen {
            Screen::Categories => {
                let Some(category) = self
                    .result
                    .categories_sorted()
                    .get(self.selected())
                    .copied()
                else {
                    return;
                };
                self.screen = Screen::Groups { category };
                self.select(0);
            }
            Screen::Groups { category } => {
                let groups = self.result.groups_in(category);
                let Some(group) = groups.get(self.selected()) else {
                    return;
                };
                let key = group.key.clone();
                self.marked.clear();
                self.screen = Screen::Files {
                    category,
                    group_key: key,
                };
                self.select(0);
            }
            Screen::Files { .. } => self.delete_marked_or_current(),
            Screen::Empty | Screen::Scanning => {}
        }
    }

    fn toggle_mark(&mut self) {
        let Screen::Files { .. } = self.screen else {
            return;
        };
        let idx = self.selected();
        if idx >= self.current_len() {
            return;
        }
        if !self.marked.remove(&idx) {
            self.marked.insert(idx);
        }
    }

    fn delete_shortcut(&mut self) {
        match self.screen {
            Screen::Files { .. } => {
                if self.marked.is_empty() {
                    self.delete_current_group();
                } else {
                    self.delete_marked_or_current();
                }
            }
            Screen::Groups { .. } => self.delete_current_group(),
            _ => {}
        }
    }

    fn delete_marked_or_current(&mut self) {
        let Screen::Files { ref group_key, .. } = self.screen else {
            return;
        };
        let Some(group) = self.result.group(group_key) else {
            return;
        };
        let indices: Vec<usize> = if self.marked.is_empty() {
            vec![self.selected()]
        } else {
            let mut v: Vec<usize> = self.marked.iter().copied().collect();
            v.sort_unstable();
            v
        };
        let items: Vec<_> = indices.iter().filter_map(|&i| group.items.get(i)).collect();
        if items.is_empty() {
            return;
        }
        let size: u64 = items.iter().map(|i| i.size).sum();
        let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        let mut body: Vec<String> = items
            .iter()
            .take(8)
            .map(|i| i.path.display().to_string())
            .collect();
        if items.len() > 8 {
            body.push(format!("… and {} more", items.len() - 8));
        }
        self.confirm = Some(Confirm {
            title: format!("Delete {} item(s) ({})?", items.len(), format_bytes(size)),
            body,
            yes: false,
            action: PendingAction::Paths(paths),
        });
    }

    fn delete_current_group(&mut self) {
        let (key, title, size, count, is_dup) = match &self.screen {
            Screen::Groups { category } => {
                let groups = self.result.groups_in(*category);
                let Some(group) = groups.get(self.selected()) else {
                    return;
                };
                (
                    group.key.clone(),
                    group.title.clone(),
                    group.size(),
                    group.count(),
                    group.category == Category::Duplicates,
                )
            }
            Screen::Files { group_key, .. } => {
                let Some(group) = self.result.group(group_key) else {
                    return;
                };
                (
                    group.key.clone(),
                    group.title.clone(),
                    group.size(),
                    group.count(),
                    group.category == Category::Duplicates,
                )
            }
            _ => return,
        };
        let extra = if is_dup && count > 1 {
            " This removes EVERY copy, including the original."
        } else {
            ""
        };
        self.confirm = Some(Confirm {
            title: format!(
                "Delete group «{title}» ({}, {count} items)?{extra}",
                format_bytes(size)
            ),
            body: Vec::new(),
            yes: false,
            action: PendingAction::Group(key),
        });
    }

    fn delete_all_in_category(&mut self) {
        let Screen::Groups { category } = self.screen else {
            return;
        };
        let groups = self.result.groups_in(category);
        if groups.is_empty() {
            return;
        }
        let size: u64 = groups.iter().map(|g| g.size()).sum();
        self.confirm = Some(Confirm {
            title: format!(
                "Delete ALL {} groups in {} ({})?",
                groups.len(),
                category.label(),
                format_bytes(size)
            ),
            body: Vec::new(),
            yes: false,
            action: PendingAction::Category(category),
        });
    }

    fn keep_highlighted(&mut self) {
        let Screen::Files { ref group_key, .. } = self.screen else {
            return;
        };
        let Some(group) = self.result.group(group_key) else {
            return;
        };
        if group.items.len() < 2 {
            self.status = "Nothing else to delete — you already have a single file.".into();
            return;
        }
        let keep = self.selected().min(group.items.len() - 1);
        let keep_path = group.items[keep].path.clone();
        let paths: Vec<PathBuf> = group
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != keep)
            .map(|(_, item)| item.path.clone())
            .collect();
        let size: u64 = group
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != keep)
            .map(|(_, item)| item.size)
            .sum();
        self.confirm = Some(Confirm {
            title: format!(
                "Keep {} and delete {} other copy(ies) ({})?",
                keep_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| keep_path.display().to_string()),
                paths.len(),
                format_bytes(size)
            ),
            body: paths
                .iter()
                .map(|p| p.display().to_string())
                .take(8)
                .collect(),
            yes: false,
            action: PendingAction::Paths(paths),
        });
    }

    fn apply_confirm(&mut self) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        if !confirm.yes {
            return;
        }
        let outcome = match confirm.action {
            PendingAction::Paths(paths) => engine::delete_items(&paths),
            PendingAction::Group(key) => {
                let paths = self
                    .result
                    .group(&key)
                    .map(|g| g.items.iter().map(|i| i.path.clone()).collect::<Vec<_>>())
                    .unwrap_or_default();
                engine::delete_items(&paths)
            }
            PendingAction::Category(category) => {
                let paths: Vec<PathBuf> = self
                    .result
                    .groups_in(category)
                    .into_iter()
                    .flat_map(|g| g.items.iter().map(|i| i.path.clone()))
                    .collect();
                engine::delete_items(&paths)
            }
        };
        self.after_delete(outcome);
    }

    fn after_delete(&mut self, outcome: DeleteOutcome) {
        self.marked.clear();
        self.result.prune_empty();
        if outcome.removed > 0 {
            self.status = format!(
                "Deleted {} item(s), freed {}.",
                outcome.removed,
                format_bytes(outcome.freed)
            );
        } else if outcome.errors.is_empty() {
            self.status = "Nothing deleted.".into();
        }
        if !outcome.errors.is_empty() {
            let extra = outcome.errors.join(" · ");
            if self.status.is_empty() {
                self.status = extra;
            } else {
                self.status = format!("{}  {}", self.status, extra);
            }
        }

        let files_nav = match &self.screen {
            Screen::Files {
                category,
                group_key,
            } => Some((*category, group_key.clone())),
            _ => None,
        };
        let groups_cat = match &self.screen {
            Screen::Groups { category } => Some(*category),
            _ => None,
        };

        if let Some((category, group_key)) = files_nav {
            if self.result.group(&group_key).is_none() {
                if self.result.groups_in(category).is_empty() {
                    self.screen = Screen::Categories;
                } else {
                    self.screen = Screen::Groups { category };
                }
                self.select(0);
            } else {
                self.select(self.selected());
            }
        } else if let Some(category) = groups_cat {
            if self.result.groups_in(category).is_empty() {
                self.screen = Screen::Categories;
                self.select(0);
            } else {
                self.select(self.selected());
            }
        } else if self.result.groups.is_empty() {
            self.screen = Screen::Empty;
        }
        if self.result.groups.is_empty() {
            self.screen = Screen::Empty;
        }
    }

    pub fn selected_detail(&self) -> (String, String) {
        match &self.screen {
            Screen::Categories => {
                if let Some(cat) = self.result.categories_sorted().get(self.selected()) {
                    return (cat.label().to_string(), cat.hint().to_string());
                }
            }
            Screen::Groups { category } => {
                if let Some(group) = self.result.groups_in(*category).get(self.selected()) {
                    let path = group
                        .items
                        .first()
                        .map(|i| i.path.display().to_string())
                        .unwrap_or_default();
                    return (
                        group.title.clone(),
                        format!("{}\n{}", group.description, path),
                    );
                }
            }
            Screen::Files { group_key, .. } => {
                if let Some(group) = self.result.group(group_key) {
                    if let Some(item) = group.items.get(self.selected()) {
                        return (
                            item.path.display().to_string(),
                            format!("{}\n{}", group.description, item.reason),
                        );
                    }
                }
            }
            Screen::Empty => {
                return (
                    "All clean".into(),
                    "Press r to scan again, or q to quit.".into(),
                )
            }
            Screen::Scanning => {
                return (
                    self.scan_message.clone(),
                    "Scanning your home folder…".into(),
                )
            }
        }
        (String::new(), String::new())
    }
}

fn index_from_mouse(area: Rect, state: &ListState, row: u16) -> Option<usize> {
    if row <= area.y || row + 1 >= area.y + area.height {
        return None;
    }
    let inner = row.saturating_sub(area.y + 1);
    Some(state.offset().saturating_add(inner as usize))
}
