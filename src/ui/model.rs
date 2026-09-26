use std::collections::HashSet;
use std::time::Instant;

use crate::engine::ScanKind;
use crate::models::{Category, FileItem, ScanResult};

#[derive(Debug, PartialEq, Eq)]
pub enum Screen {
    Scanning,
    Deleting,
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

/// Overlays drawn above the screen. They take keys before the screen does.
pub enum Modal {
    Help,
    Confirm(Confirm),
}

pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub yes: bool,
    pub action: PendingAction,
}

pub enum PendingAction {
    Items { items: Vec<FileItem>, bytes: u64 },
    Group(String),
    Category(Category),
}

pub struct Model {
    pub kind: ScanKind,
    pub result: ScanResult,
    pub screen: Screen,
    pub selection: Option<usize>,
    pub marked: HashSet<usize>,
    pub modal: Option<Modal>,
    pub status: String,
    pub scan_message: String,
    pub scan_index: usize,
    pub scan_total: usize,
    pub delete_message: String,
    pub delete_done: u64,
    pub delete_total: u64,
    pub delete_started: Instant,
    pub pre_delete_screen: Option<Screen>,
}

impl Model {
    pub fn new(kind: ScanKind) -> Self {
        Self {
            kind,
            result: ScanResult::default(),
            screen: Screen::Scanning,
            selection: Some(0),
            marked: HashSet::new(),
            modal: None,
            status: String::new(),
            scan_message: "Starting…".into(),
            scan_index: 0,
            scan_total: 1,
            delete_message: String::new(),
            delete_done: 0,
            delete_total: 1,
            delete_started: Instant::now(),
            pre_delete_screen: None,
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self.kind {
            ScanKind::Full => "Full scan",
            ScanKind::Smart => "Smart scan",
            ScanKind::Duplicates { .. } => "Duplicates",
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.screen, Screen::Scanning | Screen::Deleting)
    }

    pub fn current_len(&self) -> usize {
        match self.screen {
            Screen::Scanning | Screen::Deleting | Screen::Empty => 0,
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
        self.selection.unwrap_or(0)
    }
}
