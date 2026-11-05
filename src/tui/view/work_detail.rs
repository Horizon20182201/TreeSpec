//! Review, plan and report views of the 整理 page.

use super::{bold, color, dim, error, selected};
use crate::tui::{
    review,
    rounds::{Draft, PlanFile, Round},
    text,
    tree_index::TreeView,
};
use organizer::{
    llm::file_mapper::Assignment,
    migration::{MigrationPlan, Operation},
    scan::ScanIssueKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use std::collections::HashSet;

pub fn review(
    frame: &mut Frame,
    round: &Round,
    draft: &Draft,
    view: &mut TreeView,
    changed_only: bool,
    focus: &mut Option<String>,
    area: Rect,
) {
    let [tree_area, detail_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(5)]).areas(area);
    let marks = review::marks(round, draft);
    let filter = changed_only.then(|| review::filter(round, draft));
    let rows = draft.index.rows(&view.collapsed, filter.as_ref());
    if let Some(id) = focus.take()
        && let Some(at) = rows.iter().position(|(r, _)| *r == id)
    {
        view.cursor = at;
    }
    let missing = review::missing(draft);
    let reserve = if missing.is_empty() {
        0
    } else {
        missing.len().min(4) + 1
    };
    let height = (tree_area.height as usize).saturating_sub(1 + reserve);
    view.fit(rows.len(), height);
    let title = if changed_only {
        "目标结构（只看变化，f 切换）"
    } else {
        "目标结构（全部，f 切换）"
    };
    let mut lines = vec![Line::styled(title, bold())];
    if let Some(reason) = round.ack_badge() {
        lines[0].push_span(Span::styled(
            format!("   需确认审核：{reason}"),
            color(Color::Yellow),
        ));
    }
    for (at, (id, depth)) in rows.iter().enumerate().skip(view.offset).take(height) {
        let node = &draft.index.nodes[id];
        let dir = draft.index.is_dir(id);
        let marker = draft.index.marker(id, &view.collapsed);
        let name = format!(
            "{}{marker}{}{}",
            "  ".repeat(*depth),
            text::clean(&node.name),
            if dir { "/" } else { "" }
        );
        let mut spans = vec![Span::raw(text::pad(&name, 34))];
        if let Some(mark) = marks.get(id) {
            let style = match mark.symbol {
                '?' => color(Color::Yellow),
                '+' => color(Color::Green),
                _ => color(Color::Cyan),
            };
            spans.push(Span::styled(
                format!(" {}  {}", mark.symbol, mark.text),
                style,
            ));
        }
        let line = Line::from(spans);
        lines.push(if at == view.cursor {
            line.style(selected())
        } else {
            line
        });
    }
    if rows.is_empty() {
        lines.push(Line::styled("没有变化（按 f 查看全部）", dim()));
    }
    if !missing.is_empty() {
        lines.push(Line::styled("目标中缺失的旧节点（不是删除命令）", error()));
        for path in missing.iter().take(4) {
            lines.push(Line::styled(format!("  ! {path}"), error()));
        }
    }
    frame.render_widget(Paragraph::new(lines), tree_area);
    let detail: Vec<Line> = rows
        .get(view.cursor)
        .map(|(id, _)| {
            review::detail(round, draft, id)
                .into_iter()
                .map(Line::from)
                .collect()
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn op_lines(index: usize, op: &Operation) -> Vec<Line<'static>> {
    let (name, from, to) = match op {
        Operation::Mkdir { path, .. } => ("mkdir", None, path),
        Operation::MoveFile { from, to, .. } => ("move_file", Some(from), to),
        Operation::MoveDirectory { from, to, .. } => ("move_directory", Some(from), to),
    };
    match from {
        None => vec![Line::from(format!(
            "  {:>3}  {:<15} {}",
            index + 1,
            name,
            text::clean(to)
        ))],
        Some(from) => vec![
            Line::from(format!(
                "  {:>3}  {:<15} {}",
                index + 1,
                name,
                text::clean(from)
            )),
            Line::styled(format!("         → {}", text::clean(to)), dim()),
        ],
    }
}

pub fn plan(
    frame: &mut Frame,
    file: Option<&PlanFile>,
    unresolved: Option<&MigrationPlan>,
    previewed: &HashSet<String>,
    scroll: usize,
    area: Rect,
    note: &str,
) {
    let mut lines = vec![];
    let plan = match (file, unresolved) {
        (Some(file), _) => {
            let id = file.plan.plan_id.trim_start_matches("sha256:");
            lines.push(Line::styled(
                format!(
                    "plans/{}    plan sha256:{}…",
                    text::clean(&file.name),
                    &id[..id.len().min(8)]
                ),
                bold(),
            ));
            let state = if previewed.contains(&file.plan.plan_id) {
                "本次会话已通过（执行前会重新检查）"
            } else {
                "尚未预览（d）"
            };
            lines.push(Line::from(format!("阶段：{}", file.stage.label())));
            lines.push(Line::from(format!("预览：{state}")));
            &file.plan
        }
        (None, Some(plan)) => {
            lines.push(Line::styled("计划有未解决项，未写入文件", error()));
            plan
        }
        (None, None) => {
            lines.push(Line::from("还没有计划：在审核视图检查后按 p 生成。"));
            return frame.render_widget(Paragraph::new(lines), area);
        }
    };
    if !note.is_empty() {
        lines.push(Line::styled(note.to_owned(), dim()));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        format!("操作（{}）", plan.operations.len()),
        bold(),
    ));
    for (i, op) in plan.operations.iter().enumerate() {
        lines.extend(op_lines(i, op));
    }
    lines.push(Line::from(""));
    if plan.unresolved.is_empty() {
        lines.push(Line::from("未解决  无"));
    } else {
        lines.push(Line::styled(
            format!("未解决（{}）：回到草案修改", plan.unresolved.len()),
            error(),
        ));
        lines.extend(
            plan.unresolved
                .iter()
                .map(|u| Line::styled(format!("  ! {}", text::clean(u)), error())),
        );
    }
    for issue in &plan.boundaries {
        let tag = match issue.kind {
            ScanIssueKind::Protected => "[P]",
            ScanIssueKind::Skipped => "[S]",
            _ => "[E]",
        };
        lines.push(Line::styled(
            format!("扫描边界  {tag} {}", text::clean(&issue.path)),
            dim(),
        ));
    }
    let scroll = scroll.min(lines.len().saturating_sub(1)) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

pub fn report(frame: &mut Frame, round: &Round, scroll: usize, area: Rect) {
    let Some(report) = &round.report else {
        return frame.render_widget(Paragraph::new("手工草案没有审核报告。"), area);
    };
    let paths = round.draft.as_ref().ok().map(|d| &d.index.paths);
    let path = |id: &str| text::clean(paths.and_then(|p| p.get(id)).map_or(id, |p| p));
    let mut lines = vec![
        Line::styled(
            format!("审核报告  drafts/{}.review.json", text::clean(&round.name)),
            bold(),
        ),
        Line::from(format!(
            "编辑数 {}   读取的正文字节 {}   待审核 {}",
            report.edit_count,
            report.context_bytes_read,
            report.unresolved.len()
        )),
        Line::from(""),
        Line::styled(format!("分配（{}）", report.assignments.len()), bold()),
    ];
    for assignment in &report.assignments {
        let (file, decision, reason) = match assignment {
            Assignment::Place {
                file_id,
                target_dir_id,
                reason,
            } => (file_id, format!("place → {}", path(target_dir_id)), reason),
            Assignment::Keep { file_id, reason } => (file_id, "keep".into(), reason),
            Assignment::Review { file_id, reason } => (file_id, "review".into(), reason),
        };
        lines.push(Line::from(format!("  {}  {decision}", path(file))));
        lines.push(Line::styled(
            format!("      {}", text::clean(reason)),
            dim(),
        ));
    }
    if !report.unresolved.is_empty() {
        lines.extend([Line::from(""), Line::styled("待审核", color(Color::Yellow))]);
        for item in &report.unresolved {
            let at = item.node_id.as_deref().map(path).unwrap_or_default();
            lines.push(Line::from(format!(
                "  ? {at}  {}",
                text::clean(&item.reason)
            )));
        }
    }
    lines.extend([Line::from(""), Line::styled("警告", bold())]);
    lines.extend(
        report
            .warnings
            .iter()
            .map(|w| Line::styled(format!("  {}", text::clean(w)), dim())),
    );
    let scroll = scroll.min(lines.len().saturating_sub(1)) as u16;
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}
