//! Keys for the 整理 page (design §4.3–4.6).

use super::{
    App, Entry, View,
    dialog::{ApplyConfirm, Confirm, Dialog, Input, Next, Purpose},
    keys::unique,
    keys_pages::{list_key, tree_key},
};
use crate::tui::{
    action::Action,
    review,
    rounds::{PlanFile, Stage},
    text,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub(super) fn work_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                return self.work.right = !self.work.right && self.current_entry().is_some();
            }
            KeyCode::Esc => return self.work.right = false,
            KeyCode::Char('v') => return self.cycle_view(),
            KeyCode::Char('f') => return self.work.changed_only = !self.work.changed_only,
            KeyCode::Char('n') => return self.new_draft(),
            KeyCode::Char('e') => return self.decide(Self::edit_dialog),
            KeyCode::Char('p') => return self.decide(Self::plan_dialog),
            KeyCode::Char('d') => return self.dry_run(false),
            KeyCode::Char('A') => return self.dry_run(true),
            KeyCode::Char('x') => return self.decide(Self::export_dialog),
            KeyCode::Char('z') => return self.decide(Self::archive_dialog),
            KeyCode::Char('Z') => return self.decide(Self::archive_all_dialog),
            _ => {}
        }
        if !self.work.right {
            let len = self.entries().len();
            if list_key(&mut self.work.cursor, len, key) {
                self.reset_view();
            } else if key.code == KeyCode::Enter && len > 0 {
                self.work.right = true;
            }
            return;
        }
        if self.work.view != View::Review {
            if !list_key(&mut self.work.scroll, usize::MAX, key) && key.code == KeyCode::PageDown {
                self.work.scroll += 10;
            }
            return;
        }
        let Some(snap) = self.snapshot.as_ref() else {
            return;
        };
        let Some(Entry::Round(i)) = self.entries().get(self.work.cursor).copied() else {
            return;
        };
        let round = &snap.rounds.rounds[i];
        let Ok(draft) = &round.draft else { return };
        let filter = self.work.changed_only.then(|| review::filter(round, draft));
        let rows = draft.index.rows(&self.work.tree.collapsed, filter.as_ref());
        tree_key(&mut self.work.tree, &draft.index, &rows, key);
    }

    /// New selection: default to the plan view once a plan exists.
    pub(super) fn reset_view(&mut self) {
        let has_plan = self.current_round().is_some_and(|r| r.active.is_some());
        let unpaired = matches!(self.current_entry(), Some(Entry::Unpaired(_)));
        self.work.view = if has_plan || unpaired {
            View::Plan
        } else {
            View::Review
        };
        self.work.tree = Default::default();
        self.work.scroll = 0;
    }

    fn cycle_view(&mut self) {
        let Some(round) = self.current_round() else {
            return;
        };
        let plan = round.active.is_some() || self.work.unresolved.contains_key(&round.path);
        let report = round.report.is_some();
        let order = [View::Review, View::Plan, View::Report];
        let available = |v: &View| match v {
            View::Review => true,
            View::Plan => plan,
            View::Report => report,
        };
        let at = order.iter().position(|v| *v == self.work.view).unwrap_or(0);
        let next = (1..=3)
            .map(|k| order[(at + k) % 3])
            .find(available)
            .unwrap_or(View::Review);
        self.work.view = next;
        self.work.scroll = 0;
        self.work.right = true;
    }

    fn plan_dialog(&self) -> Result<Dialog, String> {
        if !self.state.can_plan() {
            return Err(self.state.blocked_reason().unwrap_or("受阻").into());
        }
        let round = self.current_round().ok_or("先在左栏选择一个草案")?;
        match &round.stage {
            Stage::Invalid(_) => return Err("草案无效：按 e 修正".into()),
            Stage::Stale => return Err("草案基于旧基线：按 z 归档后重新生成".into()),
            Stage::NoChanges => return Err("草案与基线相同，没有可规划的变化".into()),
            Stage::Applied | Stage::NothingToApply => return Err("这一轮已完成：按 z 归档".into()),
            _ => {}
        }
        let plans = self.root.join(".organizer/plans");
        let stem = round.name.strip_suffix(".tree").unwrap_or(&round.name);
        let ack = round.needs_ack.is_some();
        let stem = if ack {
            format!("{stem}.reviewed")
        } else {
            stem.to_owned()
        };
        let input = Dialog::Input(Input {
            title: "生成计划".into(),
            prompt: "计划只写入 plans/ 下的新文件；有未解决项时不写盘。文件名：".into(),
            value: unique(&plans, &stem, ".json"),
            purpose: Purpose::PlanName {
                to: round.path.clone(),
                ack,
            },
            error: None,
        });
        let Some(reason) = &round.needs_ack else {
            return Ok(input);
        };
        let lines = vec![
            format!("原因：{reason}"),
            "未分类和待审核的文件保持原有归属。".into(),
            "这一步不绕过路径、身份、源文件和冲突检查。".into(),
        ];
        let confirm = Confirm::new(
            "确认审核（--acknowledge-review）",
            lines,
            None,
            Next::Open(Box::new(input)),
        );
        Ok(Dialog::Confirm(
            confirm.checked("我已检查完整草案和报告；未分类文件保持原有归属"),
        ))
    }

    /// The current round's plan, if the TUI may preview/execute it. Unpaired
    /// plans are view/archive only (design §3.2).
    fn executable_plan(&self) -> Result<PlanFile, String> {
        if !self.state.can_plan() {
            return Err(self.state.blocked_reason().unwrap_or("受阻").into());
        }
        if matches!(self.current_entry(), Some(Entry::Unpaired(_))) {
            return Err("未关联计划只能查看和归档；对当前草案重新生成计划".into());
        }
        let round = self.current_round().ok_or("先选择一个整理轮次")?;
        let plan = round.active_plan().ok_or("还没有计划：按 p 生成")?;
        if plan.stage != Stage::Executable {
            return Err(format!("计划{}，不能执行", plan.stage.label()));
        }
        Ok(plan.clone())
    }

    /// `d` previews; `A` previews again and then asks for the strong confirmation.
    fn dry_run(&mut self, then_apply: bool) {
        match self.executable_plan() {
            Err(message) => self.flash(message, true),
            Ok(_) if self.busy.is_some() => self.flash("正在执行其他任务，请稍候", true),
            Ok(plan) => {
                let action = Action::DryRun {
                    root: self.root.clone(),
                    plan: plan.path.clone(),
                };
                self.pending_apply = then_apply.then_some(plan);
                self.start(action);
            }
        }
    }

    pub(super) fn apply_dialog(&self, plan: &PlanFile) -> Dialog {
        let ops = &plan.plan.operations;
        use organizer::migration::Operation as Op;
        let count = |f: fn(&Op) -> bool| ops.iter().filter(|o| f(o)).count();
        let action = Action::Apply {
            root: self.root.clone(),
            plan: plan.path.clone(),
        };
        let lines = vec![
            format!("根目录   {}", text::clean(&self.root.to_string_lossy())),
            format!("计划     plans/{}", text::clean(&plan.name)),
            format!(
                "操作     {} 个：{} mkdir，{} move_file，{} move_directory",
                ops.len(),
                count(|o| matches!(o, Op::Mkdir { .. })),
                count(|o| matches!(o, Op::MoveFile { .. })),
                count(|o| matches!(o, Op::MoveDirectory { .. }))
            ),
            "预览     刚刚通过".into(),
            String::new(),
            "只创建目录和移动，不删除、不覆盖。".into(),
            "任一步失败立即停止，已完成的移动不会自动回滚。".into(),
            "执行期间请不要改动该目录。".into(),
        ];
        Dialog::Apply(ApplyConfirm {
            lines,
            command: action.command(),
            typed: String::new(),
            action: Some(action),
            note: None,
        })
    }

    fn export_dialog(&self) -> Result<Dialog, String> {
        let plan = match self.current_entry() {
            Some(Entry::Unpaired(i)) => self.snap().map(|s| s.rounds.unpaired[i].clone()),
            _ => self.current_round().and_then(|r| r.active_plan().cloned()),
        }
        .ok_or("没有可导出的计划")?;
        let plans = self.root.join(".organizer/plans");
        let stem = plan.name.strip_suffix(".json").unwrap_or(&plan.name);
        Ok(Dialog::Input(Input {
            title: "导出预览脚本".into(),
            prompt: "脚本只调用不带 --yes 的 organizer apply。文件名：".into(),
            value: unique(&plans, stem, ".sh"),
            purpose: Purpose::ScriptName { plan: plan.path },
            error: None,
        }))
    }

    fn archive_dialog(&self) -> Result<Dialog, String> {
        if !self.state.can_archive() {
            return Err(format!("当前状态不能归档：{}", self.state.label()));
        }
        let snap = self.snap().ok_or("没有状态")?;
        let r = &snap.rounds;
        let (title, entries) = match self.current_entry().ok_or("没有可归档的内容")? {
            Entry::Round(i) => (
                format!("归档本轮 {}", r.rounds[i].name),
                r.rounds[i].selectors.clone(),
            ),
            Entry::Unpaired(i) => {
                let plan = &r.unpaired[i];
                let mut list = vec![format!("plans/{}", plan.name)];
                list.extend(plan.script.iter().map(|s| format!("plans/{s}")));
                ("归档未关联计划".into(), list)
            }
            Entry::Orphan(i) => (
                "归档孤立报告".into(),
                vec![format!("drafts/{}", r.orphans[i])],
            ),
            Entry::Other(i) => ("归档".into(), vec![r.others[i].0.clone()]),
        };
        Ok(archive_confirm(&self.root, title, entries))
    }

    fn archive_all_dialog(&self) -> Result<Dialog, String> {
        if !self.state.can_archive() {
            return Err(format!("当前状态不能归档：{}", self.state.label()));
        }
        let pending = self
            .snap()
            .and_then(|s| s.detailed())
            .map(|d| d.pending.clone())
            .unwrap_or_default();
        if pending.is_empty() {
            return Err("drafts/ 和 plans/ 都是空的".into());
        }
        let mut dialog = archive_confirm(&self.root, "全部归档".into(), vec![]);
        if let Dialog::Confirm(confirm) = &mut dialog {
            confirm.lines.splice(
                1..1,
                pending
                    .iter()
                    .map(|a| format!("  {}", text::clean(&a.selector()))),
            );
        }
        Ok(dialog)
    }

    fn edit_dialog(&self) -> Result<Dialog, String> {
        let round = self.current_round().ok_or("先在左栏选择一个草案")?;
        let generated = round.draft.as_ref().is_ok_and(|d| d.generated) || round.report.is_some();
        if !generated {
            return Ok(Dialog::Confirm(Confirm::new(
                "外部编辑",
                vec![
                    format!(
                        "用 $VISUAL / $EDITOR 打开 drafts/{}。",
                        text::clean(&round.name)
                    ),
                    "保留所有已有节点和 @ID。".into(),
                ],
                None,
                Next::Edit(round.path.clone()),
            )));
        }
        Ok(Dialog::Confirm(Confirm::new(
            "编辑 LLM 草案",
            vec![
                "保留首行 # organizer-review-v1: … 标记，以及所有已有节点和 @ID。".into(),
                "编辑后生成计划需要确认审核。".into(),
            ],
            None,
            Next::Edit(round.path.clone()),
        )))
    }
}

fn archive_confirm(root: &std::path::Path, title: String, entries: Vec<String>) -> Dialog {
    let action = Action::Archive {
        root: root.to_owned(),
        entries: entries.clone(),
    };
    let mut lines = vec!["移到 .organizer/archive/<时间>-<名称>/，不覆盖、不删除：".to_owned()];
    lines.extend(entries.iter().map(|e| format!("  {}", text::clean(e))));
    lines.push("LLM 草案总是与其审核报告一起移动。归档后它们不再阻止同步。".into());
    Dialog::Confirm(Confirm::new(
        title,
        lines,
        Some(action.command()),
        Next::Run(action),
    ))
}
