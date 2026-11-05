//! 概览 (design §5.1).

use super::{bold, color, dim, error, selected};
use crate::tui::{app::App, state::Workspace, text};
use organizer::scan::ScanIssueKind;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Color,
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = match &app.state {
        Workspace::Uninitialized => vec![
            Line::styled("当前目录未初始化", bold()),
            Line::from(""),
            Line::from(format!(
                "将要管理的根目录  {}",
                text::clean(&app.root.to_string_lossy())
            )),
            Line::from(""),
            Line::from(
                "初始化会扫描目录，创建 .organizer/（结构文本、SQLite 索引、只记录结构的 Git 仓库），不移动任何文件。",
            ),
            Line::from("要管理其他目录，退出后用 organizer tui --root PATH 启动。"),
            Line::from(""),
            Line::from("按 i 初始化"),
        ],
        Workspace::Failed(message) => failure(message),
        Workspace::Interrupted => interrupted(app),
        Workspace::Unfinished => vec![
            Line::styled("迁移未完成", error()),
            Line::from(""),
            Line::from("存在未完成的迁移运行；status、sync、plan、apply 都会停止。"),
            Line::from("到恢复页（按 5）检查每个步骤，不要重新执行计划。"),
        ],
        _ => return columns(frame, app, area),
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn failure(message: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("无法读取工作区状态", error()), Line::from("")];
    lines.extend(message.lines().map(|l| Line::from(text::clean(l))));
    if let Some(hint) = text::hint(message) {
        lines.extend([Line::from(""), Line::from(format!("建议：{hint}"))]);
    }
    lines.extend([Line::from(""), Line::from("修正后按 R 重新扫描。")]);
    lines
}

fn interrupted(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled("元数据更新中断，需要人工检查", error()),
        Line::from(""),
        Line::from(
            "先备份整个 .organizer 目录，再检查以下内容和 .organizer/sync.log；不要删除标记或直接重跑。",
        ),
    ];
    if let Some(Ok(probe)) = app.snap().map(|s| &s.probe) {
        for file in &probe.recovery_files {
            lines.push(Line::from(format!("  SQLite 恢复文件  .organizer/{file}")));
        }
        if let Some(marker) = &probe.pending_marker {
            lines.push(Line::from("  state/pending.json："));
            lines.extend(
                marker
                    .lines()
                    .take(12)
                    .map(|l| Line::from(format!("    {}", text::tail(&text::clean(l), 200)))),
            );
        }
    }
    lines
}

const CATEGORIES: [&str; 5] = ["新增", "缺失", "移动", "仅内容变化", "未知身份"];

fn columns(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] = super::panes(
        area,
        Constraint::Percentage(45),
        Constraint::Percentage(55),
        app.overview.right,
    );
    let Some(detailed) = app.snap().and_then(|s| s.detailed()) else {
        return;
    };
    let report = &detailed.report;
    let lists: [Vec<String>; 5] = [
        report.added.iter().map(|c| text::clean(&c.path)).collect(),
        report
            .missing
            .iter()
            .map(|c| text::clean(&c.path))
            .collect(),
        report
            .moved
            .iter()
            .map(|c| {
                format!(
                    "{} → {}",
                    text::clean(c.old_path.as_deref().unwrap_or("?")),
                    text::clean(&c.path)
                )
            })
            .collect(),
        report
            .changed
            .iter()
            .map(|c| text::clean(&c.path))
            .collect(),
        report
            .unknown
            .iter()
            .map(|u| {
                format!(
                    "{}  候选 {}  {}",
                    text::clean(&u.path),
                    u.candidate_ids.join(","),
                    text::clean(&u.reason)
                )
            })
            .collect(),
    ];
    let mut lines = vec![
        Line::from(format!("工作区  {}", app.state.label())),
        Line::from(""),
        Line::styled("磁盘变化（相对基线）", bold()),
    ];
    for (i, name) in CATEGORIES.iter().enumerate() {
        let row = format!("  {}{:>4}", text::pad(name, 14), lists[i].len());
        lines.push(if i == app.overview.cursor {
            Line::styled(row, selected())
        } else {
            Line::from(row)
        });
        if app.overview.expanded == Some(i) {
            let shown = lists[i].iter().take(12);
            lines.extend(shown.map(|p| Line::styled(format!("      {p}"), dim())));
            if lists[i].len() > 12 {
                lines.push(Line::styled(
                    format!("      … 另有 {} 项", lists[i].len() - 12),
                    dim(),
                ));
            }
        }
    }
    lines.push(Line::from(""));
    if report.conflicts.is_empty() {
        lines.push(Line::from("冲突  无"));
    } else {
        lines.push(Line::styled(
            format!("冲突（{}）", report.conflicts.len()),
            error(),
        ));
        lines.extend(
            report
                .conflicts
                .iter()
                .flat_map(|c| c.lines())
                .take(10)
                .map(|c| Line::from(format!("  {}", text::clean(c)))),
        );
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), left);
    frame.render_widget(
        Paragraph::new(rounds_and_boundaries(app)).wrap(Wrap { trim: false }),
        right,
    );
}

fn rounds_and_boundaries(app: &App) -> Vec<Line<'static>> {
    let snap = app.snap().unwrap();
    let r = &snap.rounds;
    let blocked = app.state.blocked_reason();
    let mut lines = vec![Line::styled("整理轮次", bold())];
    if r.is_empty() {
        lines.push(Line::styled("  无", dim()));
    }
    for round in &r.rounds {
        let stage = match (blocked, round.stage.done()) {
            (Some(reason), false) => reason.to_owned(),
            _ if round.ack_badge().is_some() => format!("{}  需确认审核", round.stage.label()),
            _ => round.stage.label().to_owned(),
        };
        lines.push(Line::from(format!(
            "  {} {stage}",
            text::pad(&text::clean(&round.name), 18)
        )));
    }
    for plan in &r.unpaired {
        lines.push(Line::from(format!(
            "  {} 未关联计划",
            text::pad(&text::clean(&plan.name), 18)
        )));
    }
    for name in &r.orphans {
        lines.push(Line::from(format!(
            "  {}孤立报告",
            text::pad(&text::clean(name), 16)
        )));
    }
    for (selector, note) in &r.others {
        lines.push(Line::from(format!("  {}  {}", text::clean(selector), note)));
    }
    let warnings = &snap.detailed().unwrap().report.warnings;
    lines.extend([Line::from(""), Line::styled("扫描边界", bold())]);
    if warnings.is_empty() {
        lines.push(Line::styled("  无", dim()));
    }
    for issue in warnings.iter().take(20) {
        let (tag, note) = match issue.kind {
            ScanIssueKind::Protected => ("[P]", "嵌套 Git 项目"),
            ScanIssueKind::Skipped => ("[S]", "符号链接/特殊文件"),
            ScanIssueKind::Excluded => ("[E]", "元数据，已排除"),
            ScanIssueKind::Error => ("[!]", "扫描出错"),
        };
        let style = if issue.kind == ScanIssueKind::Error {
            error()
        } else {
            color(Color::Cyan)
        };
        lines.push(Line::styled(
            format!(
                "  {tag} {}   {note}",
                text::middle(&text::clean(&issue.path), 30)
            ),
            style,
        ));
    }
    lines
}
