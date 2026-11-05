//! Worker results → [`ActionResult`] for the activity log and sheets.

use super::{ActionResult, OutcomeView};
use organizer::frontend::{
    action::Action,
    text::{clean, hint},
    worker::Outcome,
};

fn name(path: &std::path::Path) -> String {
    clean(
        &path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy()),
    )
}

fn n(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The command shown for `action`; `chat` gets the LLM environment prefix.
pub fn display_command(action: &Action, env_prefix: &str) -> String {
    match action {
        Action::Chat { .. } if !env_prefix.is_empty() => {
            format!("{env_prefix} {}", action.command())
        }
        _ => action.command(),
    }
}

/// `token` is the apply token a successful preview issued.
pub fn action_result(
    action: &Action,
    result: &Result<Outcome, String>,
    env_prefix: &str,
    token: Option<String>,
) -> ActionResult {
    let mut out = ActionResult {
        label: action.label().into(),
        command: clean(&display_command(action, env_prefix)),
        kind: "ok".into(),
        message: String::new(),
        error: None,
        hint: None,
        go_recovery: false,
        outcome: None,
    };
    let outcome = match result {
        Err(error) => {
            out.kind = "failed".into();
            out.message = format!("{}失败", action.label());
            out.hint = hint(error).map(Into::into);
            out.go_recovery = matches!(action, Action::Apply { .. }) && error.contains("run_");
            out.error = Some(clean(error));
            return out;
        }
        Ok(outcome) => outcome,
    };
    let (message, view) = match outcome {
        Outcome::Init(r) | Outcome::Sync(r) => (
            if matches!(action, Action::Init { .. }) {
                "初始化完成，没有移动任何文件".to_owned()
            } else if r.structure_changed {
                "已同步，结构变化已记录".to_owned()
            } else {
                "已同步".to_owned()
            },
            OutcomeView::Synced {
                structure_changed: r.structure_changed,
                git_head: r.git_head.as_deref().map(clean),
            },
        ),
        Outcome::Chat(report) => {
            if !report.unresolved.is_empty() {
                out.kind = "review".into();
            }
            (
                format!("草案已生成，有 {} 个待审核项", report.unresolved.len()),
                OutcomeView::Chatted {
                    draft: name(&report.draft),
                    review_items: n(report.unresolved.len()),
                    first_review: report.unresolved.iter().find_map(|u| u.node_id.clone()),
                },
            )
        }
        Outcome::Plan { plan, written } => {
            let file = match action {
                Action::Plan { out, .. } => name(out),
                _ => String::new(),
            };
            if !plan.unresolved.is_empty() {
                out.kind = "review".into();
            }
            (
                if *written {
                    format!("已生成计划 plans/{file}，{} 个操作", plan.operations.len())
                } else {
                    format!("计划有 {} 个未解决项，没有写入文件", plan.unresolved.len())
                },
                OutcomeView::Planned {
                    file,
                    written: *written,
                    ops: n(plan.operations.len()),
                    unresolved: plan.unresolved.iter().map(|u| clean(u)).collect(),
                },
            )
        }
        Outcome::DryRun(report) => (
            "预览通过。执行前会重新检查。".to_owned(),
            OutcomeView::Previewed {
                plan: match action {
                    Action::DryRun { plan, .. } => name(plan),
                    _ => String::new(),
                },
                ops: n(report.operation_count),
                token: token.unwrap_or_default(),
            },
        ),
        Outcome::Apply(report) => (
            format!("迁移完成：{} 个操作", report.completed_operations),
            OutcomeView::Applied {
                run_id: clean(&report.run_id),
                ops: n(report.completed_operations),
                git_head: report
                    .git_head
                    .as_deref()
                    .map(|h| clean(&h[..h.len().min(12)])),
            },
        ),
        Outcome::Script(path) => (
            "已导出脚本。脚本只调用不带 --yes 的 apply。".to_owned(),
            OutcomeView::Script { file: name(path) },
        ),
        Outcome::Archive(report) => (
            format!("已归档 {} 个文件，没有删除任何文件", report.moved.len()),
            OutcomeView::Archived {
                into: name(&report.archive),
                moved: report.moved.iter().map(|m| clean(m)).collect(),
            },
        ),
        Outcome::Recover(report) => (
            format!(
                "运行 {} 状态：{}",
                clean(&report.run_id),
                clean(&report.status)
            ),
            OutcomeView::Recovered {
                run_id: clean(&report.run_id),
                status: clean(&report.status),
            },
        ),
        Outcome::Draft(path) => (
            format!("已创建 drafts/{}", name(path)),
            OutcomeView::Draft { name: name(path) },
        ),
    };
    out.message = message;
    out.outcome = Some(view);
    out
}
