//! 整理 (design §5.3): round list, step bar and the current view.

use super::{bold, dim, selected, work_detail};
use crate::tui::{
    app::{App, Entry, View},
    rounds::{Round, Stage},
    state::Snapshot,
    text,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let entries = app.entries();
    let Some(snap) = app.snapshot.as_ref().filter(|s| s.detailed().is_some()) else {
        return frame.render_widget(Paragraph::new("整理页不可用：查看概览页"), area);
    };
    let [left, right] = super::panes(
        area,
        Constraint::Length(26),
        Constraint::Min(30),
        app.work.right,
    );
    let blocked = app.state.blocked_reason();
    let mut lines = vec![Line::styled("整理轮次", bold())];
    let mut heading = None;
    for (at, entry) in entries.iter().enumerate() {
        let group = match entry {
            Entry::Round(_) => None,
            Entry::Unpaired(_) => Some("未关联计划"),
            Entry::Orphan(_) | Entry::Other(_) => Some("其他文件"),
        };
        if let Some(title) = group
            && group != heading
        {
            lines.extend([Line::from(""), Line::styled(title, bold())]);
            heading = group;
        }
        let (name, note) = label(snap, *entry, blocked);
        let marker = if at == app.work.cursor { "▶ " } else { "  " };
        let style = if at == app.work.cursor && !app.work.right {
            selected()
        } else {
            Default::default()
        };
        lines.push(Line::styled(
            format!("{marker}{}", text::tail(&name, 22)),
            style,
        ));
        lines.push(Line::styled(format!("    {note}"), dim()));
    }
    if entries.is_empty() {
        lines.extend([
            Line::styled("  无", dim()),
            Line::from(""),
            Line::from("按 n 新建草案"),
        ]);
    }
    frame.render_widget(Paragraph::new(lines), left);
    let [steps, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(right);
    let entry = entries.get(app.work.cursor).copied();
    let round = match entry {
        Some(Entry::Round(i)) => Some(&snap.rounds.rounds[i]),
        _ => None,
    };
    frame.render_widget(Paragraph::new(step_lines(round, app.work.view)), steps);
    match entry {
        None => {}
        Some(Entry::Unpaired(i)) => {
            let plan = &snap.rounds.unpaired[i];
            work_detail::plan(
                frame,
                Some(plan),
                None,
                &app.previewed,
                app.work.scroll,
                body,
                "未关联：计划目标与任何草案都不匹配，只能查看和归档（z）。",
            );
        }
        Some(Entry::Orphan(i)) => {
            let note = format!(
                "drafts/{} 没有对应的草案文件。按 z 归档。",
                text::clean(&snap.rounds.orphans[i])
            );
            frame.render_widget(Paragraph::new(note).wrap(Wrap { trim: false }), body);
        }
        Some(Entry::Other(i)) => {
            let (selector, note) = &snap.rounds.others[i];
            let note = format!(
                "{}：{}。按 z 归档。",
                text::clean(selector),
                text::clean(note)
            );
            frame.render_widget(Paragraph::new(note).wrap(Wrap { trim: false }), body);
        }
        Some(Entry::Round(i)) => {
            let round = &snap.rounds.rounds[i];
            let unresolved = app.work.unresolved.get(&round.path);
            match (&round.draft, app.work.view) {
                (Err(error), _) => {
                    let mut lines = vec![Line::styled("草案无效", super::error())];
                    lines.extend(error.lines().map(|l| Line::from(text::clean(l))));
                    lines.extend([
                        Line::from(""),
                        Line::from("按 e 打开编辑器修正，或按 z 归档。"),
                    ]);
                    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
                }
                (Ok(_), View::Plan) => {
                    let note = "";
                    work_detail::plan(
                        frame,
                        round.active_plan(),
                        unresolved,
                        &app.previewed,
                        app.work.scroll,
                        body,
                        note,
                    );
                }
                (Ok(_), View::Report) => work_detail::report(frame, round, app.work.scroll, body),
                (Ok(draft), View::Review) => {
                    work_detail::review(
                        frame,
                        round,
                        draft,
                        &mut app.work.tree,
                        app.work.changed_only,
                        &mut app.work.focus_node,
                        body,
                    );
                }
            }
        }
    }
}

fn label(snap: &Snapshot, entry: Entry, blocked: Option<&str>) -> (String, String) {
    let r = &snap.rounds;
    match entry {
        Entry::Round(i) => {
            let round = &r.rounds[i];
            let mut note = round.stage.label().to_owned();
            if round.ack_badge().is_some() {
                note.push_str("  需确认审核");
            }
            if let Some(reason) = blocked.filter(|_| !round.stage.done()) {
                note = reason.to_owned();
            }
            (text::clean(&round.name), note)
        }
        Entry::Unpaired(i) => (
            text::clean(&r.unpaired[i].name),
            r.unpaired[i].stage.label().into(),
        ),
        Entry::Orphan(i) => (text::clean(&r.orphans[i]), "孤立报告".into()),
        Entry::Other(i) => (text::clean(&r.others[i].0), text::clean(&r.others[i].1)),
    }
}

fn step_lines(round: Option<&Round>, view: View) -> Vec<Line<'static>> {
    let Some(round) = round else { return vec![] };
    let current = match round.stage {
        Stage::Invalid(_) | Stage::Stale | Stage::NoChanges => 0,
        Stage::NeedsPlan | Stage::Blocked | Stage::PlanStale => 1,
        Stage::Executable => 3,
        Stage::NothingToApply | Stage::Applied => 4,
    };
    let names = ["草案", "审核", "计划", "执行", "归档"];
    let mut spans = vec![];
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(if i == current { " ▶ " } else { " ─ " }));
        } else if current == 0 {
            spans.push(Span::raw("▶ "));
        }
        spans.push(if i == current {
            Span::styled(*name, bold())
        } else {
            Span::raw(*name)
        });
    }
    let tab = |v: View, name: &str| {
        if v == view {
            format!("[{name}]")
        } else {
            format!(" {name} ")
        }
    };
    let tabs = format!(
        "{}{}{}   v 切换",
        tab(View::Review, "审核"),
        tab(View::Plan, "计划"),
        tab(View::Report, "报告")
    );
    vec![Line::from(spans), Line::styled(tabs, dim())]
}
