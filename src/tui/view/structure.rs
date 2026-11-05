//! 结构 (design §5.2): read-only baseline browser and LLM scope selection.

use super::{bold, color, dim, selected};
use crate::tui::{app::App, text};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Color,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(snap) = app.snapshot.as_ref().filter(|s| s.detailed().is_some()) else {
        return frame.render_widget(Paragraph::new("结构不可用：查看概览页"), area);
    };
    let [left, right] = super::panes(
        area,
        Constraint::Percentage(55),
        Constraint::Percentage(45),
        app.structure.right,
    );
    let index = &snap.baseline;
    let view = &mut app.structure.view;
    let rows = index.rows(&view.collapsed, None);
    let height = left.height.saturating_sub(1) as usize;
    view.fit(rows.len(), height);
    let mut lines = vec![Line::styled("基线结构（只读）", bold())];
    for (at, (id, depth)) in rows.iter().enumerate().skip(view.offset).take(height) {
        let node = &index.nodes[id];
        let dir = index.is_dir(id);
        let marker = index.marker(id, &view.collapsed);
        let mut spans = vec![Span::raw(format!(
            "{}{marker}{}{}",
            "  ".repeat(*depth),
            text::clean(&node.name),
            if dir { "/" } else { "" }
        ))];
        if snap.excluded.contains(id) {
            spans.push(Span::styled("  [X]", color(Color::Red)));
        }
        if app.structure.scope.contains(id) {
            spans.push(Span::styled("  [*]", color(Color::Green)));
        }
        if app.structure.show_ids {
            spans.push(Span::styled(format!("  @{id}"), dim()));
        }
        if let Some(description) = &node.description {
            spans.push(Span::styled(
                format!("   {}", text::clean(description)),
                dim(),
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
        lines.push(Line::styled("（空目录）", dim()));
    }
    frame.render_widget(Paragraph::new(lines), left);
    let mut info = vec![];
    if let Some((id, _)) = rows.get(view.cursor) {
        let node = &index.nodes[id];
        let dir = index.is_dir(id);
        info.push(Line::styled(
            format!(
                "{}{}",
                text::clean(&index.paths[id]),
                if dir { "/" } else { "" }
            ),
            bold(),
        ));
        info.push(Line::from(format!("ID      {id}")));
        info.push(Line::from(format!(
            "类型    {}",
            if dir { "目录" } else { "文件" }
        )));
        info.push(Line::from(format!(
            "说明    {}",
            node.description
                .as_deref()
                .map(text::clean)
                .unwrap_or_else(|| "无".into())
        )));
        if dir {
            let kids = index.kids(Some(id));
            let dirs = kids.iter().filter(|k| index.is_dir(k)).count();
            info.push(Line::from(format!(
                "子项    {dirs} 个目录，{} 个文件",
                kids.len() - dirs
            )));
        }
        let llm = if snap.excluded.contains(id) {
            "敏感或受保护，不发送"
        } else {
            "可发送名称与路径"
        };
        info.push(Line::from(format!("LLM     {llm}")));
    }
    info.push(Line::from(""));
    let scope = &app.structure.scope;
    if scope.is_empty() {
        info.push(Line::from("范围：未选择（整个根目录）"));
    } else {
        info.push(Line::from(format!("范围：已选 {} 个目录", scope.len())));
        info.extend(scope.iter().map(|id| {
            Line::from(format!(
                "  {}",
                text::clean(index.paths.get(id).map_or(id, |p| p))
            ))
        }));
    }
    frame.render_widget(Paragraph::new(info).wrap(Wrap { trim: false }), right);
}
