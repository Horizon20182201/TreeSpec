//! Global keys, quitting, and what happens after a dialog.

use super::{
    App, Page,
    dialog::{ChatForm, Choice, Dialog, Input, Next, Purpose, Reply},
};
use crate::tui::{action::Action, text};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    path::Path,
    time::{Duration, Instant},
};

impl App {
    pub fn key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return self.ctrl_c();
        }
        if let Some(mut dialog) = self.dialog.take() {
            match dialog.key(key) {
                Reply::Stay => self.dialog = Some(dialog),
                Reply::Close => {}
                Reply::Then(next) => self.follow(next, dialog),
            }
            return;
        }
        self.flash = None;
        match key.code {
            KeyCode::Char(c @ '1'..='5') => self.page = Page::ALL[(c as u8 - b'1') as usize],
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('R') if self.busy.is_some() => {
                self.flash("正在执行其他任务，请稍候", true)
            }
            KeyCode::Char('R') => self.refresh(),
            KeyCode::Char('?') => self.dialog = Some(Dialog::Help(0)),
            KeyCode::Char('L') => self.dialog = Some(Dialog::Log(0)),
            _ => self.page_key(key),
        }
    }

    fn request_quit(&mut self) {
        if self.busy.as_ref().is_some_and(|b| b.applying) {
            self.flash("迁移执行中不能退出；如必须中止，连按两次 Ctrl-C", true);
        } else {
            self.quit = true;
        }
    }

    /// Design §6: during `apply` the first Ctrl-C only warns.
    fn ctrl_c(&mut self) {
        let applying = self.busy.as_ref().is_some_and(|b| b.applying);
        if !applying
            || self
                .ctrl_c
                .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
        {
            self.quit = true;
        } else {
            self.ctrl_c = Some(Instant::now());
            self.flash(
                "中止会留下部分完成的迁移，需要人工恢复。3 秒内再按一次 Ctrl-C 才会退出",
                true,
            );
        }
    }

    /// Opens the dialog built by `f`, or explains why it is unavailable.
    pub(super) fn decide(&mut self, f: fn(&Self) -> Result<Dialog, String>) {
        if self.busy.is_some() {
            return self.flash("正在执行其他任务，请稍候", true);
        }
        match f(self) {
            Ok(dialog) => self.dialog = Some(dialog),
            Err(message) => self.flash(message, true),
        }
    }

    fn follow(&mut self, next: Next, dialog: Dialog) {
        match next {
            Next::Run(action) => self.start(action),
            Next::Open(dialog) => self.dialog = Some(*dialog),
            Next::Edit(path) => self.pending_editor = Some(path),
            Next::Chat => self.open_chat(),
            Next::AskDraftName => self.ask_draft_name(),
            Next::Submit(purpose, value) => {
                if let Err(error) = self.submit(purpose, &value)
                    && let Dialog::Input(mut input) = dialog
                {
                    input.error = Some(error);
                    self.dialog = Some(Dialog::Input(input));
                }
            }
        }
    }

    fn submit(&mut self, purpose: Purpose, value: &str) -> Result<(), String> {
        let meta = self.root.join(".organizer");
        let action = match purpose {
            Purpose::PlanName { to, ack } => {
                check_name(&meta.join("plans"), value, ".json")?;
                Action::Plan {
                    root: self.root.clone(),
                    to,
                    acknowledge_review: ack,
                    out: meta.join("plans").join(value),
                }
            }
            Purpose::ScriptName { plan } => {
                check_name(&meta.join("plans"), value, ".sh")?;
                Action::ExportScript {
                    plan,
                    out: meta.join("plans").join(value),
                }
            }
            Purpose::DraftName => {
                check_name(&meta.join("drafts"), value, ".tree")?;
                Action::CopyBaseline {
                    root: self.root.clone(),
                    name: value.to_owned(),
                }
            }
            Purpose::Search => {
                self.search(value);
                return Ok(());
            }
        };
        self.start(action);
        Ok(())
    }

    pub(super) fn open_chat(&mut self) {
        if self.busy.is_some() {
            return self.flash("正在执行其他任务，请稍候", true);
        }
        if !self.llm.available {
            let reason = self.llm.reason.clone().unwrap_or_default();
            return self.flash(format!("LLM 不可用：{}", text::clean(&reason)), true);
        }
        if !self.state.can_draft() {
            return self.flash(self.state.blocked_reason().unwrap_or("受阻"), true);
        }
        let Some(detailed) = self.snap().and_then(|s| s.detailed()) else {
            return;
        };
        let scope_ids = self.structure.scope.clone();
        let preview =
            organizer::llm::preview_scope(&detailed.saved, &scope_ids).map_err(|e| e.to_string());
        let paths = &self.snap().unwrap().baseline.paths;
        let scope_label = if scope_ids.is_empty() {
            "整个根目录（结构页 Space 选择范围）".to_owned()
        } else {
            scope_ids
                .iter()
                .map(|id| text::clean(paths.get(id).map_or(id, |p| p)))
                .collect::<Vec<_>>()
                .join("、")
        };
        let drafts = self.root.join(".organizer/drafts");
        self.dialog = Some(Dialog::Chat(Box::new(ChatForm {
            root: self.root.clone(),
            out: unique(
                &drafts,
                &format!("chat_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                ".tree",
            ),
            drafts,
            prompt: String::new(),
            scope_ids,
            scope_label,
            classify: true,
            read_content: false,
            focus: 0,
            preview,
            llm: self.llm.clone(),
            error: None,
        })));
    }

    pub(super) fn ask_draft_name(&mut self) {
        if !self.state.can_copy_baseline() {
            return self.flash(self.state.blocked_reason().unwrap_or("受阻"), true);
        }
        let drafts = self.root.join(".organizer/drafts");
        self.dialog = Some(Dialog::Input(Input {
            title: "复制当前结构为草案".into(),
            prompt: "在 drafts/ 下只新建、不覆盖，等价于 cp -n。文件名：".into(),
            value: unique(&drafts, "new", ".tree"),
            purpose: Purpose::DraftName,
            error: None,
        }));
    }

    pub(super) fn new_draft(&mut self) {
        if self.busy.is_some() {
            return self.flash("正在执行其他任务，请稍候", true);
        }
        if !self.state.can_copy_baseline() {
            return self.flash(self.state.blocked_reason().unwrap_or("受阻"), true);
        }
        let llm_blocked = if !self.state.can_draft() {
            Some("基线冲突时只能另存当前 structure.tree".to_owned())
        } else {
            (!self.llm.available).then(|| self.llm.label.clone())
        };
        let focus = usize::from(llm_blocked.is_some());
        self.dialog = Some(Dialog::Choice(Choice { focus, llm_blocked }));
    }
}

/// A valid, not yet existing file name inside `directory`.
pub(super) fn check_name(directory: &Path, name: &str, suffix: &str) -> Result<(), String> {
    organizer::workspace::check_new_name(name, suffix).map_err(|e| e.to_string())?;
    let path = directory.join(name);
    if path.exists() || directory.join(format!("{name}.review.json")).exists() {
        return Err(format!("{name} 已存在，换一个文件名"));
    }
    Ok(())
}

/// `stem.suffix`, or `stem-2.suffix`, `stem-3.suffix`, … if taken.
pub(super) fn unique(directory: &Path, stem: &str, suffix: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{stem}{suffix}")
            } else {
                format!("{stem}-{n}{suffix}")
            }
        })
        .find(|name| check_name(directory, name, suffix).is_ok())
        .unwrap()
}
