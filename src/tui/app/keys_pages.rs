//! Keys for 概览, 结构, 历史 and 恢复, plus tree navigation shared with 整理.

use super::{
    App, Page, View,
    dialog::{Confirm, Dialog, Input, Next, Purpose},
};
use crate::tui::{
    action::Action,
    state::Workspace,
    text,
    tree_index::{TreeIndex, TreeView},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashSet;

/// Moves/collapses within a tree pane. Returns whether the key was used.
pub(super) fn tree_key(
    view: &mut TreeView,
    index: &TreeIndex,
    rows: &[(String, usize)],
    key: KeyEvent,
) -> bool {
    let current = rows
        .get(view.cursor)
        .map(|(id, depth)| (id.clone(), *depth));
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            view.cursor = (view.cursor + 1).min(rows.len().saturating_sub(1))
        }
        KeyCode::Up | KeyCode::Char('k') => view.cursor = view.cursor.saturating_sub(1),
        KeyCode::PageDown => view.cursor = (view.cursor + 10).min(rows.len().saturating_sub(1)),
        KeyCode::PageUp => view.cursor = view.cursor.saturating_sub(10),
        KeyCode::Home | KeyCode::Char('g') => view.cursor = 0,
        KeyCode::End | KeyCode::Char('G') => view.cursor = rows.len().saturating_sub(1),
        KeyCode::Left | KeyCode::Char('h') => match current {
            Some((id, _)) if index.is_dir(&id) && !view.collapsed.contains(&id) => {
                view.collapsed.insert(id);
            }
            Some((id, depth)) if depth > 0 => {
                let parent = index.nodes.get(&id).and_then(|n| n.parent_id.clone());
                if let Some(at) = rows.iter().position(|(r, _)| Some(r) == parent.as_ref()) {
                    view.cursor = at;
                }
            }
            _ => {}
        },
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some((id, _)) = current {
                view.collapsed.remove(&id);
            }
        }
        KeyCode::Enter => {
            if let Some((id, _)) = current.filter(|(id, _)| index.is_dir(id))
                && !view.collapsed.remove(&id)
            {
                view.collapsed.insert(id);
            }
        }
        _ => return false,
    }
    true
}

impl App {
    pub(super) fn page_key(&mut self, key: KeyEvent) {
        let searchable = self.page == Page::Structure
            || (self.page == Page::Work && self.work.right && self.work.view == View::Review);
        if key.code == KeyCode::Char('/') && searchable {
            self.dialog = Some(Dialog::Input(Input {
                title: "搜索".into(),
                prompt: "按名称查找（不区分大小写），Enter 跳到下一个匹配：".into(),
                value: String::new(),
                purpose: Purpose::Search,
                error: None,
            }));
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            match self.page {
                Page::Overview => return self.overview.right = !self.overview.right,
                Page::Structure => return self.structure.right = !self.structure.right,
                Page::History => return self.history.right = !self.history.right,
                _ => {}
            }
        }
        match self.page {
            Page::Overview => self.overview_key(key),
            Page::Structure => self.structure_key(key),
            Page::Work => self.work_key(key),
            Page::History => {
                let len = self
                    .snap()
                    .and_then(|s| s.history.as_ref().ok())
                    .map_or(0, Vec::len);
                list_key(&mut self.history.cursor, len, key);
            }
            Page::Recovery => self.recovery_key(key),
        }
    }

    fn overview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('i') => self.decide(Self::init_dialog),
            KeyCode::Char('s') => self.decide(Self::sync_dialog),
            KeyCode::Enter => {
                let at = self.overview.cursor;
                self.overview.expanded = (self.overview.expanded != Some(at)).then_some(at);
            }
            _ => {
                list_key(&mut self.overview.cursor, 5, key);
            }
        }
    }

    fn init_dialog(&self) -> Result<Dialog, String> {
        if self.state != Workspace::Uninitialized {
            return Err("已经初始化".into());
        }
        let root = self.root.to_string_lossy();
        let mut lines = vec![
            format!("根目录  {}", text::clean(&root)),
            String::new(),
            "扫描目录并创建 .organizer/（结构文本、SQLite 索引、只记录结构的 Git 仓库）。".into(),
            "不移动任何文件。".into(),
        ];
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if self.root.components().count() <= 2 || home.as_deref() == Some(self.root.as_path()) {
            lines.push("注意：这是一个很大的目录，扫描可能很慢。".into());
        }
        let action = Action::Init {
            root: self.root.clone(),
        };
        Ok(Dialog::Confirm(Confirm::new(
            "初始化",
            lines,
            Some(action.command()),
            Next::Run(action),
        )))
    }

    fn sync_dialog(&self) -> Result<Dialog, String> {
        let pending = self.snap().is_some_and(|s| s.has_pending());
        if !self.state.can_sync(pending) {
            return Err(if pending {
                "有进行中的整理轮次：先在整理页按 z/Z 归档，再同步".into()
            } else {
                format!("当前状态不能同步：{}", self.state.label())
            });
        }
        let report = &self
            .snap()
            .and_then(|s| s.detailed())
            .ok_or("没有状态")?
            .report;
        let mut lines = vec![format!(
            "新增 {}   缺失 {}   移动 {}   仅内容变化 {}   未知身份 {}",
            report.added.len(),
            report.missing.len(),
            report.moved.len(),
            report.changed.len(),
            report.unknown.len()
        )];
        if report.pending_git {
            lines.push("先重试上次未完成的 Git 记录。".into());
        }
        lines.push("缺失只从结构快照移除，不删除磁盘文件，也无法恢复其内容。".into());
        lines.push("仅内容变化只更新扫描缓存，不产生结构提交。".into());
        let accept_unknown = !report.unknown.is_empty();
        if accept_unknown {
            lines.push(String::new());
            lines.push("未知身份（不会被推断为移动）：".into());
            for item in report.unknown.iter().take(20) {
                lines.push(format!(
                    "  {}  候选 {}  {}",
                    text::clean(&item.path),
                    item.candidate_ids.join(","),
                    text::clean(&item.reason)
                ));
            }
        }
        let action = Action::Sync {
            root: self.root.clone(),
            accept_unknown,
        };
        let confirm = Confirm::new("同步", lines, Some(action.command()), Next::Run(action));
        Ok(Dialog::Confirm(if accept_unknown {
            confirm.checked("将这些记录为旧节点消失 + 新节点出现")
        } else {
            confirm
        }))
    }

    fn structure_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') => return self.open_chat(),
            KeyCode::Char('i') => return self.structure.show_ids = !self.structure.show_ids,
            _ => {}
        }
        let Some(snap) = self.snapshot.as_ref() else {
            return;
        };
        let rows = snap.baseline.rows(&self.structure.view.collapsed, None);
        if key.code != KeyCode::Char(' ') {
            tree_key(&mut self.structure.view, &snap.baseline, &rows, key);
            return;
        }
        let Some((id, _)) = rows.get(self.structure.view.cursor) else {
            return;
        };
        let problem = if !snap.baseline.is_dir(id) {
            Some("只能选择目录作为范围")
        } else if snap.excluded.contains(id) {
            Some("敏感或受保护的目录不能作为范围")
        } else {
            None
        };
        let id = id.clone();
        if let Some(problem) = problem {
            return self.flash(problem, true);
        }
        let scope = &mut self.structure.scope;
        match scope.iter().position(|s| *s == id) {
            Some(at) => {
                scope.remove(at);
            }
            None => scope.push(id),
        }
    }

    fn recovery_key(&mut self, key: KeyEvent) {
        let len = self.snap().map_or(0, |s| s.recoveries.len());
        match key.code {
            KeyCode::Char('F') => self.decide(Self::finish_git_dialog),
            KeyCode::Char('M') => self.decide(Self::resolve_dialog),
            KeyCode::Enter => self.recovery.detail = !self.recovery.detail,
            _ => {
                list_key(&mut self.recovery.cursor, len, key);
            }
        }
    }

    fn selected_run(&self) -> Result<&organizer::migration::journal::RecoveryReport, String> {
        let entry = self
            .snap()
            .and_then(|s| s.recoveries.get(self.recovery.cursor))
            .ok_or("没有未完成的运行")?;
        entry
            .report
            .as_ref()
            .map_err(|e| format!("无法检查该运行：{}", text::clean(e)))
    }

    fn finish_git_dialog(&self) -> Result<Dialog, String> {
        let report = self.selected_run()?;
        if report.metadata_update_pending || report.completed_steps != report.steps.len() {
            return Err("只有全部步骤已确认、且没有发布标记时才能补记 Git".into());
        }
        let action = Action::FinishGit {
            root: self.root.clone(),
            run_id: report.run_id.clone(),
        };
        let lines = vec![
            format!("运行 {}  状态 {}", report.run_id, report.status),
            "只补齐结构提交与确认；绝不重放文件移动。库会先核对目标、身份和 Git 前置条件。".into(),
        ];
        Ok(Dialog::Confirm(Confirm::new(
            "补记 Git",
            lines,
            Some(action.command()),
            Next::Run(action),
        )))
    }

    fn resolve_dialog(&self) -> Result<Dialog, String> {
        let report = self.selected_run()?;
        if report.status == "git_pending" {
            return Err("该运行只差 Git 记录：按 F 补记".into());
        }
        if report.metadata_update_pending {
            return Err("元数据发布曾中断：先备份 .organizer 再人工检查，不能标记为已处理".into());
        }
        let action = Action::MarkResolved {
            root: self.root.clone(),
            run_id: report.run_id.clone(),
        };
        let lines = vec![
            format!(
                "运行 {}  状态 {}  已确认 {}/{}",
                report.run_id,
                report.status,
                report.completed_steps,
                report.steps.len()
            ),
            "只关闭这次运行，不移动文件，不修改基线、索引或 Git；该计划此后永不再执行。".into(),
            "之后先归档这一轮的草案和计划，再同步，记录磁盘的实际状态。".into(),
        ];
        let confirm = Confirm::new(
            "标记已人工处理",
            lines,
            Some(action.command()),
            Next::Run(action),
        );
        Ok(Dialog::Confirm(confirm.checked(
            "我已人工核对每个步骤的源和目标，磁盘处于我接受的状态",
        )))
    }

    /// Jumps to the next node whose name contains `query`, expanding ancestors.
    pub(super) fn search(&mut self, query: &str) {
        let query = query.to_lowercase();
        let Some(snap) = self.snapshot.as_ref() else {
            return;
        };
        let (index, view) = if self.page == Page::Structure {
            (&snap.baseline, &mut self.structure.view)
        } else {
            let Some(super::Entry::Round(i)) = self.entries().get(self.work.cursor).copied() else {
                return;
            };
            let Ok(draft) = &snap.rounds.rounds[i].draft else {
                return;
            };
            self.work.changed_only = false;
            (&draft.index, &mut self.work.tree)
        };
        let all = index.rows(&HashSet::new(), None);
        let visible = index.rows(&view.collapsed, None);
        let current = visible.get(view.cursor).map(|(id, _)| id.clone());
        let start = current
            .and_then(|c| all.iter().position(|(id, _)| *id == c))
            .map_or(0, |i| i + 1);
        let found = (0..all.len())
            .map(|k| &all[(start + k) % all.len()].0)
            .find(|id| index.nodes[*id].name.to_lowercase().contains(&query))
            .cloned();
        let Some(id) = found else {
            return self.flash(format!("没有匹配“{}”的节点", text::clean(&query)), true);
        };
        for ancestor in index.with_ancestors([&id]) {
            if ancestor != id {
                view.collapsed.remove(&ancestor);
            }
        }
        let rows = index.rows(&view.collapsed, None);
        view.cursor = rows.iter().position(|(r, _)| *r == id).unwrap_or(0);
    }
}

pub(super) fn list_key(cursor: &mut usize, len: usize, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(len.saturating_sub(1)),
        KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
        KeyCode::Home | KeyCode::Char('g') => *cursor = 0,
        KeyCode::End | KeyCode::Char('G') => *cursor = len.saturating_sub(1),
        _ => return false,
    }
    true
}
