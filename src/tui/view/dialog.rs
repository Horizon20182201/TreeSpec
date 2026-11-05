//! Dialog rendering.

use super::{bold, color, dim, error, selected};
use crate::tui::{
    action::Action,
    app::{App, Page, dialog::Dialog},
    text,
};
use organizer::llm::{MAX_CLASSIFY_FILES, MAX_PROMPT_BYTES};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

fn popup(frame: &mut Frame, title: &str, lines: Vec<Line>, scroll: usize) {
    let area = frame.area();
    let width = (area.width * 86 / 100).clamp(40.min(area.width), 100.min(area.width));
    // Height in wrapped rows, so the last lines are never silently cut off.
    // Word wrapping can need one more row than the character count suggests.
    let inner = width.saturating_sub(2).max(1) as usize;
    let rows: usize = lines
        .iter()
        .map(|l| match l.width() {
            w if w <= inner => 1,
            w => w.div_ceil(inner) + 1,
        })
        .sum();
    let height = (rows as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(3)
        .min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    // A wide character just left of the popup would otherwise cover its border.
    let buffer = frame.buffer_mut();
    for y in rect.top()..rect.bottom() {
        if rect.x > area.x && text::width(buffer[(rect.x - 1, y)].symbol()) > 1 {
            buffer[(rect.x - 1, y)].set_symbol(" ");
        }
    }
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(format!(" {title} "), bold()));
    let scroll = scroll.min(rows.saturating_sub(1)) as u16;
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        rect,
    );
}

fn command(lines: &mut Vec<Line>, command: &str) {
    lines.extend([
        Line::from(""),
        Line::styled("等价命令", bold()),
        Line::styled(command.to_owned(), dim()),
    ]);
}

fn button(label: &str, focused: bool) -> Span<'static> {
    let text = format!("[ {label} ]");
    if focused {
        Span::styled(text, selected())
    } else {
        Span::raw(text)
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let Some(dialog) = &app.dialog else { return };
    match dialog {
        Dialog::Confirm(d) => {
            let mut lines: Vec<Line> = d.lines.iter().map(|l| Line::from(l.clone())).collect();
            if let Some(c) = &d.command {
                command(&mut lines, c);
            }
            lines.push(Line::from(""));
            if let Some((label, ticked)) = &d.check {
                let row = format!("[{}] {label}", if *ticked { "x" } else { " " });
                lines.push(if d.focus == 0 {
                    Line::styled(row, selected())
                } else {
                    Line::from(row)
                });
            }
            lines.push(Line::from(vec![
                button("取消", d.focus == 1),
                Span::raw("  "),
                button("确认 y", d.focus == 2),
            ]));
            if let Some(note) = &d.note {
                lines.push(Line::styled(note.clone(), color(Color::Yellow)));
            }
            popup(frame, &d.title, lines, 0);
        }
        Dialog::Apply(d) => {
            let mut lines: Vec<Line> = d.lines.iter().map(|l| Line::from(l.clone())).collect();
            command(&mut lines, &d.command);
            lines.extend([
                Line::from(""),
                Line::styled(
                    format!("输入 apply 确认：{}_", text::clean(&d.typed)),
                    bold(),
                ),
            ]);
            if let Some(note) = &d.note {
                lines.push(Line::styled(note.clone(), color(Color::Yellow)));
            }
            lines.push(Line::styled("Esc 取消", dim()));
            popup(frame, "执行迁移", lines, 0);
        }
        Dialog::Chat(d) => {
            let focus = |i: usize, line: String| {
                if d.focus == i {
                    Line::styled(line, selected())
                } else {
                    Line::from(line)
                }
            };
            let check = |on: bool| if on { "[x]" } else { "[ ]" };
            let mut lines = vec![
                focus(0, format!("请求   {}_", text::clean(&d.prompt))),
                Line::styled(
                    format!("       {} / {MAX_PROMPT_BYTES} 字节", d.prompt.len()),
                    dim(),
                ),
                Line::from(format!("范围   {}", d.scope_label)),
                focus(
                    1,
                    format!(
                        "{} 允许重新分类已有文件        --classify",
                        check(d.classify)
                    ),
                ),
            ];
            let content = if d.llm.no_content {
                Line::styled(
                    "[ ] 发送文本片段（ORGANIZER_LLM_NO_CONTENT=1 已禁止）",
                    dim(),
                )
            } else {
                focus(
                    2,
                    format!(
                        "{} 发送文本片段（单文件 4 KiB，整次 64 KiB）  --read-content",
                        check(d.read_content)
                    ),
                )
            };
            lines.push(content);
            lines.push(focus(3, format!("输出   drafts/{}_", text::clean(&d.out))));
            lines.push(Line::from(""));
            let host = d.llm.host.as_deref().map(text::clean).unwrap_or_default();
            lines.push(Line::from(format!(
                "将发送到  {host}（{}）",
                if d.llm.remote {
                    "远程，HTTPS"
                } else {
                    "本地"
                }
            )));
            match &d.preview {
                Ok(p) => {
                    let body = if d.read_content {
                        "受支持文本文件的有限片段（PDF、二进制只用文件名）"
                    } else {
                        "不含文件正文"
                    };
                    lines.push(Line::from(format!(
                        "发送内容  范围内 {} 个节点的名称、相对路径、ID、说明；",
                        p.visible_nodes
                    )));
                    lines.push(Line::from(format!(
                        "          {body}；{} 个敏感或受保护节点已排除",
                        p.excluded_ids.len()
                    )));
                    if d.classify && p.editable_files > MAX_CLASSIFY_FILES {
                        lines.push(Line::styled(
                            format!(
                                "范围内有 {} 个文件，可能超过单次分类上限 {MAX_CLASSIFY_FILES}",
                                p.editable_files
                            ),
                            color(Color::Yellow),
                        ));
                    }
                }
                Err(e) => lines.push(Line::styled(
                    format!("范围不可用：{}（在结构页选择更小的目录）", text::clean(e)),
                    error(),
                )),
            }
            let action = Action::Chat {
                root: d.root.clone(),
                prompt: d.prompt.clone(),
                scope_ids: d.scope_ids.clone(),
                classify: d.classify,
                read_content: d.read_content,
                out: d.drafts.join(&d.out),
            };
            command(&mut lines, &text::clean(&action.command()));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                button("取消", d.focus == 4),
                Span::raw("  "),
                button("发送", d.focus == 5),
            ]));
            if let Some(e) = &d.error {
                lines.push(Line::styled(e.clone(), error()));
            }
            lines.push(Line::styled(
                "Tab 切换  Space 勾选  Enter 下一项/按钮  Esc 取消",
                dim(),
            ));
            popup(frame, "新建 LLM 草案", lines, 0);
        }
        Dialog::Input(d) => {
            let mut lines = vec![
                Line::from(d.prompt.clone()),
                Line::styled(format!("{}_", text::clean(&d.value)), bold()),
            ];
            if let Some(e) = &d.error {
                lines.push(Line::styled(text::clean(e), error()));
            }
            lines.push(Line::styled("Enter 确定  Esc 取消", dim()));
            popup(frame, &d.title, lines, 0);
        }
        Dialog::Choice(d) => {
            let first = match &d.llm_blocked {
                Some(reason) => Line::styled(
                    format!(
                        "1  用 LLM 生成（不可用：{}）",
                        text::tail(&text::clean(reason), 60)
                    ),
                    dim(),
                ),
                None if d.focus == 0 => Line::styled("1  用 LLM 生成", selected()),
                None => Line::from("1  用 LLM 生成"),
            };
            let second = if d.focus == 1 {
                Line::styled("2  复制当前结构，手工编辑", selected())
            } else {
                Line::from("2  复制当前结构，手工编辑")
            };
            popup(
                frame,
                "新建草案",
                vec![first, second, Line::styled("Enter 选择  Esc 取消", dim())],
                0,
            );
        }
        Dialog::Message(d) => {
            let style = if d.error { error() } else { Default::default() };
            let mut lines: Vec<Line> = d
                .lines
                .iter()
                .map(|l| Line::styled(l.clone(), style))
                .collect();
            lines.push(Line::styled("Enter/Esc 关闭", dim()));
            popup(frame, &d.title, lines, d.scroll);
        }
        Dialog::Help(scroll) => popup(frame, "帮助", help(app.page), *scroll),
        Dialog::Log(scroll) => {
            let mut lines: Vec<Line> = app
                .log
                .iter()
                .flat_map(|e| {
                    let style = if e.failed {
                        error()
                    } else {
                        Default::default()
                    };
                    [
                        Line::styled(format!("+{}s  {}", e.at, text::clean(&e.command)), dim()),
                        Line::styled(format!("      {}", text::clean(&e.result)), style),
                    ]
                })
                .collect();
            if lines.is_empty() {
                lines.push(Line::from("本次会话还没有执行动作。"));
            }
            popup(frame, "命令日志（仅本次会话）", lines, *scroll);
        }
    }
}

fn help(page: Page) -> Vec<Line<'static>> {
    let global = [
        "1–5 切换页面   Tab 切换栏   ↑↓/jk 移动   ←→/hl 折叠展开",
        "Enter 打开/展开   Esc 返回   / 搜索   R 重新扫描   L 命令日志   q 退出",
    ];
    let page_keys: &[&str] = match page {
        Page::Overview => &[
            "i 初始化   s 同步（未知身份需勾选才带 --accept-unknown）",
            "Enter 展开某类变化的路径",
        ],
        Page::Structure => &[
            "Space 选为 LLM 范围（--scope）   c 以所选范围新建 LLM 草案",
            "i 显示 ID   [X] 敏感或受保护，不发送给 LLM",
        ],
        Page::Work => &[
            "n 新建草案   e 外部编辑器   v 切换审核/计划/报告   f 只看变化",
            "p 生成计划   d 预览   A 执行（需输入 apply）   x 导出脚本",
            "z 归档本轮   Z 全部归档（不删除，移到 .organizer/archive）",
            "标注：+ 新建  ~ 改名  > 新父目录  ^ 随上级移动  # 说明  ? 待审核  ! 缺失",
        ],
        Page::History => &["左栏结构提交，右栏迁移运行"],
        Page::Recovery => &[
            "F 补记 Git（仅差 Git 记录时）",
            "M 人工核对后关闭运行；之后归档、同步",
            "Enter 显示完整路径",
        ],
    };
    let mut lines: Vec<Line> = global.iter().map(|l| Line::from(*l)).collect();
    lines.push(Line::from(""));
    lines.extend(page_keys.iter().map(|l| Line::from(*l)));
    lines.extend([
        Line::from(""),
        Line::styled(
            "移动用户文件只发生在 A 执行；所有检查由 organizer 库完成，界面提示不替代检查。",
            dim(),
        ),
    ]);
    lines
}
