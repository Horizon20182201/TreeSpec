//! Application state. Key handlers never perform I/O: they queue jobs in
//! `outbox` (sent to the worker by the run loop, or executed inline by tests).

mod events;
mod keys;
mod keys_pages;
mod keys_work;

pub mod dialog;

use super::{
    action::Action,
    llm_info::LlmInfo,
    rounds::{PlanFile, Round},
    state::{Snapshot, Workspace},
    tree_index::TreeView,
    worker::Job,
};
use dialog::Dialog;
use organizer::migration::MigrationPlan;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    Structure,
    Work,
    History,
    Recovery,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Self::Overview,
        Self::Structure,
        Self::Work,
        Self::History,
        Self::Recovery,
    ];
    pub fn title(self) -> &'static str {
        ["概览", "结构", "整理", "历史", "恢复"][self as usize]
    }
}

pub struct Busy {
    pub label: &'static str,
    pub started: Instant,
    pub progress: Option<(usize, usize)>,
    /// `apply --yes` is moving files: quitting needs a second Ctrl-C.
    pub applying: bool,
}

pub struct LogEntry {
    pub at: u64,
    pub command: String,
    pub result: String,
    pub failed: bool,
}

#[derive(Default)]
pub struct OverviewState {
    pub cursor: usize,
    pub expanded: Option<usize>,
    /// Narrow terminals show one pane; Tab switches (design §9.4).
    pub right: bool,
}

#[derive(Default)]
pub struct StructureState {
    pub view: TreeView,
    /// Selected `--scope` directory IDs, in selection order.
    pub scope: Vec<String>,
    pub show_ids: bool,
    pub right: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Review,
    Plan,
    Report,
}

pub struct WorkState {
    pub cursor: usize,
    pub right: bool,
    pub view: View,
    pub changed_only: bool,
    pub tree: TreeView,
    pub scroll: usize,
    /// Plans with unresolved items are shown but not written (design §4.5).
    pub unresolved: HashMap<PathBuf, MigrationPlan>,
    pub focus_node: Option<String>,
    /// Select this draft's round after the next refresh.
    pub select: Option<PathBuf>,
}

impl Default for WorkState {
    fn default() -> Self {
        Self {
            cursor: 0,
            right: false,
            view: View::Review,
            changed_only: true,
            tree: TreeView::default(),
            scroll: 0,
            unresolved: HashMap::new(),
            focus_node: None,
            select: None,
        }
    }
}

#[derive(Default)]
pub struct ListState {
    pub cursor: usize,
    pub detail: bool,
    pub right: bool,
}

/// Rows of the 整理 list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Round(usize),
    Unpaired(usize),
    Orphan(usize),
    Other(usize),
}

pub struct App {
    pub root: PathBuf,
    pub llm: LlmInfo,
    pub snapshot: Option<Snapshot>,
    pub state: Workspace,
    pub page: Page,
    pub busy: Option<Busy>,
    pub dialog: Option<Dialog>,
    pub log: Vec<LogEntry>,
    pub flash: Option<(String, bool)>,
    pub previewed: HashSet<String>,
    pub pending_apply: Option<PlanFile>,
    pub pending_editor: Option<PathBuf>,
    pub overview: OverviewState,
    pub structure: StructureState,
    pub work: WorkState,
    pub history: ListState,
    pub recovery: ListState,
    pub quit: bool,
    pub ctrl_c: Option<Instant>,
    pub outbox: Vec<Job>,
    pub started: Instant,
    shown_recovery: bool,
}

impl App {
    pub fn new(root: PathBuf, llm: LlmInfo) -> Self {
        let mut app = Self {
            root,
            llm,
            snapshot: None,
            state: Workspace::Failed("正在扫描".into()),
            page: Page::Overview,
            busy: None,
            dialog: None,
            log: vec![],
            flash: None,
            previewed: HashSet::new(),
            pending_apply: None,
            pending_editor: None,
            overview: OverviewState::default(),
            structure: StructureState::default(),
            work: WorkState::default(),
            history: ListState::default(),
            recovery: ListState::default(),
            quit: false,
            ctrl_c: None,
            outbox: vec![],
            started: Instant::now(),
            shown_recovery: false,
        };
        app.refresh();
        app
    }

    pub fn snap(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn refresh(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy {
            label: "扫描中",
            started: Instant::now(),
            progress: None,
            applying: false,
        });
        self.outbox.push(Job::Refresh);
    }

    /// Queues a write/read action. Only one job runs at a time.
    pub fn start(&mut self, action: Action) {
        if self.busy.is_some() {
            self.flash("正在执行其他任务，请稍候", true);
            return;
        }
        self.busy = Some(Busy {
            label: action.label(),
            started: Instant::now(),
            progress: None,
            applying: matches!(action, Action::Apply { .. }),
        });
        self.outbox.push(Job::Run(action));
    }

    pub fn flash(&mut self, text: impl Into<String>, error: bool) {
        self.flash = Some((text.into(), error));
    }

    pub fn entries(&self) -> Vec<Entry> {
        let Some(snap) = self.snap() else {
            return vec![];
        };
        let r = &snap.rounds;
        (0..r.rounds.len())
            .map(Entry::Round)
            .chain((0..r.unpaired.len()).map(Entry::Unpaired))
            .chain((0..r.orphans.len()).map(Entry::Orphan))
            .chain((0..r.others.len()).map(Entry::Other))
            .collect()
    }

    pub fn current_entry(&self) -> Option<Entry> {
        self.entries().get(self.work.cursor).copied()
    }

    pub fn current_round(&self) -> Option<&Round> {
        match self.current_entry()? {
            Entry::Round(i) => self.snap()?.rounds.rounds.get(i),
            _ => None,
        }
    }

    fn select_draft(&mut self, path: PathBuf, view: View) {
        self.work.select = Some(path);
        self.work.view = view;
        self.work.tree = TreeView::default();
        self.work.scroll = 0;
        self.page = Page::Work;
        self.work.right = true;
    }
}
