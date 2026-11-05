//! 历史 and 恢复 (design §5.5).

use super::{bold, color, dim, error, selected};
use crate::tui::{app::App, text};
use organizer::{
    migration::{Operation, journal::RecoveryReport},
    scan::FileStamp,
};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Color,
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub fn history(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] = super::panes(
        area,
        Constraint::Percentage(50),
        Constraint::Percentage(50),
        app.history.right,
    );
    let Some(snap) = app.snap() else { return };
    let mut lines = vec![Line::styled("结构提交", bold())];
    match &snap.history {
        Ok(commits) if commits.is_empty() => lines.push(Line::styled("  无", dim())),
        Ok(commits) => {
            let height = left.height.saturating_sub(1) as usize;
            let start = app.history.cursor.saturating_sub(height.saturating_sub(1));
            for (at, (hash, subject)) in commits.iter().enumerate().skip(start).take(height) {
                let kind = subject.split(':').next().unwrap_or("");
                let line = text::tail(
                    &format!(
                        "{hash}  {:<9} {}",
                        kind,
                        text::clean(subject.split_once(": ").map_or(subject, |(_, s)| s))
                    ),
                    left.width as usize,
                );
                lines.push(if at == app.history.cursor {
                    Line::styled(line, selected())
                } else {
                    Line::from(line)
                });
            }
        }
        Err(e) => lines.push(Line::styled(
            format!("  无法读取：{}", text::clean(e)),
            error(),
        )),
    }
    frame.render_widget(Paragraph::new(lines), left);
    let mut runs = vec![Line::styled("迁移运行", bold())];
    let all = snap
        .probe
        .as_ref()
        .map(|p| p.runs.as_slice())
        .unwrap_or_default();
    if all.is_empty() {
        runs.push(Line::styled("  无", dim()));
    }
    for run in all.iter().rev().take(right.height as usize) {
        let style = if run.is_finished() {
            Default::default()
        } else {
            color(Color::Yellow)
        };
        runs.push(Line::styled(
            format!(
                "{}  {:<11} {}/{}",
                short(&run.run_id),
                run.status,
                run.completed_steps,
                run.total_steps
            ),
            style,
        ));
    }
    frame.render_widget(Paragraph::new(runs), right);
}

fn short(run_id: &str) -> String {
    format!("{}…", &run_id[..run_id.len().min(12)])
}

fn same(a: &FileStamp, b: &FileStamp) -> bool {
    a.platform_file_key.is_some() && a.platform_file_key == b.platform_file_key
}

/// Where the planned object is now. A hint for manual checking only.
fn disk(report: &RecoveryReport, op: &Operation) -> &'static str {
    let entries = &report.current_scan.entries;
    let at = |path: &str| entries.iter().find(|e| e.path == path);
    let (from, to) = match op {
        Operation::Mkdir { path, .. } => {
            return if at(path).is_some() {
                "在目标"
            } else {
                "不存在"
            };
        }
        Operation::MoveFile { from, to, .. } | Operation::MoveDirectory { from, to, .. } => {
            (from, to)
        }
    };
    let Some(before) = report.plan.preconditions.iter().find(|p| p.id == op.id()) else {
        return "?";
    };
    match (at(to), at(from)) {
        (Some(e), _) if same(&e.stamp, &before.entry.stamp) => "在目标",
        (_, Some(e)) if same(&e.stamp, &before.entry.stamp) => "在源",
        (Some(_), _) => "目标被占用",
        _ if entries.iter().any(|e| same(&e.stamp, &before.entry.stamp)) => "在别处",
        _ => "两端都不存在",
    }
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let Some(snap) = app.snap() else { return };
    if snap.recoveries.is_empty() {
        let note = "没有未完成的迁移。迁移失败或中断时，这里会列出每个步骤的状态。";
        return frame.render_widget(Paragraph::new(note), area);
    }
    let mut lines = vec![];
    if snap.recoveries.len() > 1 {
        for (at, entry) in snap.recoveries.iter().enumerate() {
            let row = format!("{}  {}", short(&entry.run.run_id), entry.run.status);
            lines.push(if at == app.recovery.cursor {
                Line::styled(row, selected())
            } else {
                Line::from(row)
            });
        }
        lines.push(Line::from(""));
    }
    let entry = &snap.recoveries[app.recovery.cursor.min(snap.recoveries.len() - 1)];
    let report = match &entry.report {
        Ok(report) => report,
        Err(e) => {
            lines.push(Line::styled(
                format!("无法检查 {}：{}", entry.run.run_id, text::clean(e)),
                error(),
            ));
            return frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        }
    };
    let id = report.plan.plan_id.trim_start_matches("sha256:");
    lines.push(Line::styled(
        format!(
            "{}   状态 {}   已确认 {}/{}   plan sha256:{}…",
            short(&report.run_id),
            report.status,
            report.completed_steps,
            report.steps.len(),
            &id[..id.len().min(8)]
        ),
        bold(),
    ));
    if let Some(e) = &report.error {
        lines.push(Line::styled(
            format!("错误：{}", text::tail(&text::clean(e), 400)),
            error(),
        ));
    }
    if report.metadata_update_pending {
        lines.push(Line::styled(
            "存在元数据发布标记 state/pending.json：先备份 .organizer 再人工检查。",
            error(),
        ));
    }
    lines.extend([
        Line::from(""),
        Line::styled(
            "步骤  状态     操作                                         当前磁盘",
            bold(),
        ),
    ]);
    for step in &report.steps {
        let (name, path) = match &step.operation {
            Operation::Mkdir { path, .. } => ("mkdir", text::clean(path)),
            Operation::MoveFile { from, to, .. } => (
                "move_file",
                format!("{} → {}", text::clean(from), text::clean(to)),
            ),
            Operation::MoveDirectory { from, to, .. } => (
                "move_directory",
                format!("{} → {}", text::clean(from), text::clean(to)),
            ),
        };
        let detail = if app.recovery.detail {
            path.clone()
        } else {
            text::middle(&path, 28)
        };
        let style = if step.status == "done" {
            Default::default()
        } else {
            color(Color::Yellow)
        };
        lines.push(Line::styled(
            format!(
                "  {:<3} {:<8} {:<15} {} {}",
                step.position + 1,
                step.status,
                name,
                text::pad(&detail, 28),
                disk(report, &step.operation)
            ),
            style,
        ));
    }
    lines.extend([
        Line::from(""),
        Line::from("只读检查。pending 可能已落盘但未确认，不要重新执行计划。"),
        Line::from("部分完成的文件步骤需人工核对；TUI 不提供续跑或回滚。核对后按 M 关闭这次运行，再归档、同步。"),
    ]);
    let finish = !report.metadata_update_pending && report.completed_steps == report.steps.len();
    let resolve = report.status != "git_pending" && !report.metadata_update_pending;
    lines.push(Line::styled(
        format!(
            "F 补记 Git（{}）   M 标记已人工处理（{}）",
            if finish {
                "可尝试"
            } else {
                "本状态不可用"
            },
            if resolve {
                "可用"
            } else {
                "本状态不可用"
            }
        ),
        dim(),
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
