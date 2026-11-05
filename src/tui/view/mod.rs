//! Rendering. Layout follows design §2: header, tabs, page, then three footer
//! lines (status, next step, page keys).

mod dialog;
mod overview;
mod recovery;
mod structure;
mod work;
mod work_detail;

use super::{
    app::{App, Page, View},
    rounds,
    state::Workspace,
    text,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

fn plain() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}
pub fn color(c: Color) -> Style {
    if plain() {
        Style::default()
    } else {
        Style::default().fg(c)
    }
}
pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}
pub fn dim() -> Style {
    if plain() {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}
pub fn error() -> Style {
    color(Color::Red)
}
pub fn rule(width: u16) -> Line<'static> {
    Line::from("─".repeat(width as usize))
}

/// Two panes side by side. Below 80 columns only one is shown and Tab
/// switches (design §9.4); the hidden pane gets an empty area.
pub fn panes(area: Rect, left: Constraint, right: Constraint, show_right: bool) -> [Rect; 2] {
    if area.width >= 80 {
        return Layout::horizontal([left, right]).spacing(2).areas(area);
    }
    let hidden = Rect::new(area.x, area.y, 0, 0);
    if show_right {
        [hidden, area]
    } else {
        [area, hidden]
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, tabs, top, body, bottom, status, next, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let width = frame.area().width;
    frame.render_widget(Paragraph::new(header_line(app, width)), header);
    frame.render_widget(Paragraph::new(tab_line(app)), tabs);
    frame.render_widget(Paragraph::new(rule(width)), top);
    frame.render_widget(Paragraph::new(rule(width)), bottom);
    match app.page {
        Page::Overview => overview::render(frame, app, body),
        Page::Structure => structure::render(frame, app, body),
        Page::Work => work::render(frame, app, body),
        Page::History => recovery::history(frame, app, body),
        Page::Recovery => recovery::render(frame, app, body),
    }
    frame.render_widget(Paragraph::new(status_line(app)), status);
    frame.render_widget(
        Paragraph::new(format!(
            "下一步：{}",
            text::tail(&next_step(app), width as usize)
        )),
        next,
    );
    frame.render_widget(
        Paragraph::new(Line::styled(keys_line(app, width as usize), dim())),
        keys,
    );
    if app.dialog.is_some() {
        dialog::render(frame, app);
    }
}

fn header_line(app: &App, width: u16) -> String {
    let age =
        app.snap()
            .map(|s| s.scanned_at.elapsed().as_secs())
            .map_or("扫描中".to_owned(), |s| match s {
                0..5 => "扫描 刚刚".into(),
                5..60 => format!("扫描 {s} 秒前"),
                _ => format!("扫描 {} 分钟前", s / 60),
            });
    let root = text::clean(&app.root.to_string_lossy());
    let line = format!(
        "TreeSpec Organizer  {}  LLM {}  {age}",
        text::middle(&root, 40),
        app.llm.label
    );
    text::tail(&line, width as usize)
}

fn tab_line(app: &App) -> Line<'static> {
    let mut spans = vec![];
    for (i, page) in Page::ALL.iter().enumerate() {
        let label = format!("{} {}", i + 1, page.title());
        let recovery_idle =
            *page == Page::Recovery && app.snap().is_none_or(|s| s.recoveries.is_empty());
        let span = if *page == app.page {
            Span::styled(format!("[{label}]"), bold())
        } else if recovery_idle {
            Span::styled(format!(" {label} "), dim())
        } else {
            Span::raw(format!(" {label} "))
        };
        spans.push(span);
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn status_line(app: &App) -> Line<'static> {
    if let Some(busy) = &app.busy {
        let progress = busy
            .progress
            .map(|(d, t)| format!(" 第 {d} / {t} 步"))
            .unwrap_or_default();
        let wait = if busy.applying {
            "（不要改动该目录）"
        } else {
            ""
        };
        return Line::styled(
            format!(
                "执行中：{}…{progress}  {} 秒{wait}",
                busy.label,
                busy.started.elapsed().as_secs()
            ),
            color(Color::Yellow),
        );
    }
    match &app.flash {
        Some((message, true)) => Line::styled(message.clone(), color(Color::Yellow)),
        Some((message, false)) => Line::styled(message.clone(), color(Color::Green)),
        None => Line::from(format!("状态：{}", app.state.label())),
    }
}

/// The bottom-bar suggestion computed from the state model.
pub fn next_step(app: &App) -> String {
    let pending = app.snap().is_some_and(|s| s.has_pending());
    match &app.state {
        Workspace::Failed(_) => "无法读取状态：查看概览页的错误，修正后按 R 重新扫描".into(),
        Workspace::Uninitialized => "按 i 初始化这个目录".into(),
        Workspace::Interrupted => "元数据更新曾中断：先备份 .organizer，再人工检查".into(),
        Workspace::Unfinished => "迁移未完成：在恢复页（5）检查".into(),
        Workspace::Conflict => {
            "基线冲突：可在整理页按 n 另存当前 structure.tree，再人工恢复基线".into()
        }
        Workspace::ScanIncomplete => "扫描不完整：修正出错路径的权限后按 R".into(),
        Workspace::GitPending if pending => {
            "Git 待补记：先在整理页按 Z 归档，再在概览页按 s 重试".into()
        }
        Workspace::GitPending => "Git 待补记：在概览页按 s 重试".into(),
        Workspace::Changed if pending => {
            "磁盘有变化：先在整理页按 Z 归档进行中的轮次，再在概览页按 s 同步".into()
        }
        Workspace::Changed => "磁盘有变化：在概览页按 s 同步".into(),
        Workspace::Clean => app
            .snap()
            .map_or_else(String::new, |s| rounds::suggestion(&s.rounds)),
    }
}

fn keys_line(app: &App, width: usize) -> String {
    let page = match app.page {
        Page::Overview if app.state == Workspace::Uninitialized => "i 初始化",
        Page::Overview => "Enter 展开分类   s 同步",
        Page::Structure => "Space 选为范围  c 新建 LLM 草案  i 显示 ID  / 搜索",
        Page::Work if !app.work.right => "n 新建草案   Enter 进入   z 归档本轮   Z 全部归档",
        Page::Work => match app.work.view {
            View::Review => "e 编辑  v 视图  f 只看变化  p 生成计划  z 归档  Esc 返回",
            View::Plan => "d 预览   A 执行   x 导出脚本   v 视图   Esc 返回",
            View::Report => "v 视图   j/k 滚动   Esc 返回",
        },
        Page::History => "j/k 移动",
        Page::Recovery => "Enter 详情   F 补记 Git   M 标记已人工处理",
    };
    // Page keys first; global keys only while they fit (`?` lists them all).
    let mut line = page.to_owned();
    for extra in ["? 帮助", "q 退出", "R 重新扫描", "L 日志"] {
        if text::width(&line) + 3 + text::width(extra) <= width {
            line.push_str("   ");
            line.push_str(extra);
        }
    }
    line
}
