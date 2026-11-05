//! Worker events: refreshed snapshots and finished actions.

use super::{
    App, Entry, LogEntry, Page, View,
    dialog::{Dialog, Message},
};
use crate::tui::{
    action::Action,
    state::{Snapshot, Workspace},
    text,
    worker::{Event, Outcome},
};

impl App {
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => self.apply_snapshot(*snapshot),
            Event::Progress(done, total) => {
                if let Some(busy) = &mut self.busy {
                    busy.progress = Some((done, total));
                }
            }
            Event::Finished => {
                self.busy = None;
                self.ctrl_c = None;
            }
            Event::Done(done) => {
                let (action, result) = *done;
                self.done(action, result);
            }
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let state = snapshot.classify();
        // Design §2: an unfinished migration opens the recovery page once.
        if state == Workspace::Unfinished && !self.shown_recovery {
            self.page = Page::Recovery;
        }
        self.shown_recovery = state == Workspace::Unfinished;
        self.structure
            .scope
            .retain(|id| snapshot.baseline.is_dir(id) && !snapshot.excluded.contains(id));
        let rounds = &snapshot.rounds.rounds;
        self.work.unresolved.retain(|path, plan| {
            rounds.iter().any(|r| {
                r.path == *path
                    && r.draft
                        .as_ref()
                        .is_ok_and(|d| d.digest == plan.target_tree_digest)
            })
        });
        self.snapshot = Some(snapshot);
        self.state = state;
        if let Some(path) = self.work.select.take() {
            let snap = self.snap().unwrap();
            let found = self
                .entries()
                .iter()
                .position(|e| matches!(e, Entry::Round(i) if snap.rounds.rounds[*i].path == path));
            if let Some(at) = found {
                self.work.cursor = at;
            }
        }
        let len = self.entries().len();
        self.work.cursor = self.work.cursor.min(len.saturating_sub(1));
        self.work.right &= len > 0;
        let runs = self.snap().map_or(0, |s| s.recoveries.len());
        self.recovery.cursor = self.recovery.cursor.min(runs.saturating_sub(1));
    }

    fn done(&mut self, action: Action, result: Result<Outcome, String>) {
        let command = action.command();
        let at = self.started.elapsed().as_secs();
        match result {
            Ok(outcome) => {
                let (message, attention) = self.outcome(&action, outcome);
                self.log.push(LogEntry {
                    at,
                    command,
                    result: message.clone(),
                    failed: false,
                });
                self.flash(message, attention);
            }
            Err(error) => {
                if matches!(action, Action::DryRun { .. } | Action::Apply { .. }) {
                    self.pending_apply = None;
                }
                self.log.push(LogEntry {
                    at,
                    command,
                    result: error.clone(),
                    failed: true,
                });
                let mut lines: Vec<String> = error.lines().map(text::clean).collect();
                if let Some(hint) = text::hint(&error) {
                    lines.push(String::new());
                    lines.push(format!("建议：{hint}"));
                }
                if matches!(action, Action::Apply { .. }) {
                    lines.push("如果迁移已经开始，刷新后会进入恢复页。".into());
                }
                let title = format!("{}失败", action.label());
                self.flash(title.clone(), true);
                self.dialog = Some(Dialog::Message(Message {
                    title,
                    lines,
                    error: true,
                    scroll: 0,
                }));
            }
        }
    }

    /// Message and whether it needs attention.
    fn outcome(&mut self, action: &Action, outcome: Outcome) -> (String, bool) {
        match outcome {
            Outcome::Init(report) => {
                self.page = Page::Overview;
                (
                    format!("已初始化：记录了 {} 个节点", report.report.added.len()),
                    false,
                )
            }
            Outcome::Sync(report) => {
                let r = &report.report;
                let summary = format!(
                    "新增 {}，缺失 {}，移动 {}",
                    r.added.len(),
                    r.missing.len(),
                    r.moved.len()
                );
                let committed = if report.structure_changed {
                    "已提交结构"
                } else {
                    "结构无变化"
                };
                (format!("已同步：{summary}；{committed}"), false)
            }
            Outcome::Chat(report) => {
                let pending = report.unresolved.len();
                self.work.focus_node = report.unresolved.iter().find_map(|i| i.node_id.clone());
                self.select_draft(report.draft.clone(), View::Review);
                if pending == 0 {
                    ("草案已生成：检查目标结构后按 p 生成计划".into(), false)
                } else {
                    (format!("草案已生成，有 {pending} 个待审核项"), true)
                }
            }
            Outcome::Plan { plan, written } => {
                let Action::Plan { to, out, .. } = action else {
                    return (String::new(), false);
                };
                self.select_draft(to.clone(), View::Plan);
                if written {
                    let name = text::file_name(out);
                    (
                        format!(
                            "计划已写入 plans/{name}：{} 个操作。按 d 预览",
                            plan.operations.len()
                        ),
                        false,
                    )
                } else {
                    let count = plan.unresolved.len();
                    self.work.unresolved.insert(to.clone(), *plan);
                    (
                        format!("计划有 {count} 个未解决项，未写入文件：回到草案修改"),
                        true,
                    )
                }
            }
            Outcome::DryRun(report) => {
                self.previewed.insert(report.plan_id.clone());
                if let Some(plan) = self
                    .pending_apply
                    .take_if(|p| p.plan.plan_id == report.plan_id)
                {
                    self.dialog = Some(self.apply_dialog(&plan));
                }
                (
                    format!(
                        "预览通过：{} 个操作仍可执行（执行前会重新检查）",
                        report.operation_count
                    ),
                    false,
                )
            }
            Outcome::Apply(report) => {
                let head = report
                    .git_head
                    .as_deref()
                    .map(|h| format!("，Git {}", &h[..h.len().min(8)]))
                    .unwrap_or_default();
                (
                    format!(
                        "迁移完成：{} 个操作{head}。按 z 归档本轮（L 日志可见 {}）",
                        report.completed_operations,
                        &report.run_id[..report.run_id.len().min(12)]
                    ),
                    false,
                )
            }
            Outcome::Script(path) => (
                format!(
                    "已导出 plans/{}（仅预览，不带 --yes）",
                    text::file_name(&path)
                ),
                false,
            ),
            Outcome::Archive(report) => (
                format!(
                    "已归档 {} 个文件到 {}",
                    report.moved.len(),
                    text::file_name(&report.archive)
                ),
                false,
            ),
            Outcome::Recover(report) => match report.status.as_str() {
                "complete" => ("Git 记录已补齐，运行已完成".into(), false),
                "resolved" => ("运行已标记为人工处理：先归档这一轮，再同步".into(), true),
                status => (format!("运行状态：{status}"), true),
            },
            Outcome::Draft(path) => {
                self.select_draft(path.clone(), View::Review);
                (
                    format!("已创建 drafts/{}：按 e 编辑", text::file_name(&path)),
                    false,
                )
            }
        }
    }
}
