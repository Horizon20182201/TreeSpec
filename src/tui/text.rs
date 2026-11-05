//! Display helpers. Every string from disk, the LLM or an error message passes
//! through [`clean`] before it reaches the terminal.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Escapes control and bidirectional-override characters so untrusted names,
/// descriptions, reasons and errors cannot inject terminal escape sequences
/// or visually reorder text.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// Display width in terminal columns (CJK characters count as two).
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Truncates at the end with `…` to at most `max` columns.
pub fn tail(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Elides the middle of a path, keeping the start and the file name.
pub fn middle(text: &str, max: usize) -> String {
    if width(text) <= max || max < 5 {
        return tail(text, max);
    }
    let keep = max - 1;
    let right_budget = keep / 2 + keep % 2;
    let mut right: Vec<char> = vec![];
    let mut used = 0;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > right_budget {
            break;
        }
        right.push(c);
        used += w;
    }
    right.reverse();
    let left = tail(text, keep - used + 1);
    let left = left.trim_end_matches('…');
    format!("{left}…{}", right.into_iter().collect::<String>())
}

/// Pads with spaces to exactly `columns` (truncating if needed).
pub fn pad(text: &str, columns: usize) -> String {
    let text = tail(text, columns);
    let fill = columns.saturating_sub(width(&text));
    format!("{text}{}", " ".repeat(fill))
}

/// Escaped last path component, for messages.
pub fn file_name(path: &std::path::Path) -> String {
    clean(
        &path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy()),
    )
}

/// Short Chinese guidance for common library errors. The original English
/// error is always shown as well; this only suggests the next step.
pub fn hint(error: &str) -> Option<&'static str> {
    const HINTS: &[(&str, &str)] = &[
        (
            "busy or cannot be locked",
            "元数据正被另一个 organizer 进程使用，稍后按 R 重试。",
        ),
        ("unfinished migration", "存在未完成的迁移，到恢复页检查。"),
        (
            "incomplete metadata update",
            "元数据更新曾中断：先备份 .organizer，再人工检查 state/pending.json 和 sync.log。",
        ),
        (
            "SQLite recovery files",
            "存在 SQLite 恢复文件：先备份 .organizer，再人工检查。",
        ),
        (
            "baseline is stale",
            "磁盘与基线不一致：先归档进行中的轮次，再同步。",
        ),
        (
            "baseline or disk changed while waiting",
            "等待模型期间磁盘或基线有变化：同步后重试。",
        ),
        (
            "plan is stale or altered",
            "计划已过期：对当前草案重新生成计划。",
        ),
        (
            "source changed",
            "源文件在计划生成后有变化：同步后重新生成计划。",
        ),
        (
            "--from is not the recorded baseline",
            "计划基线不是当前基线：重新生成计划。",
        ),
        (
            "unresolved review items or was edited",
            "草案有待审核项或生成后被编辑：在确认框中勾选“已检查”后再生成计划。",
        ),
        (
            "review report does not match",
            "草案的审核报告与基线或根目录不符：草案已过期，归档后重新生成。",
        ),
        (
            "review marker/report is missing",
            "LLM 草案缺少首行审核标记或配套报告：恢复原文件，或归档后重新生成。",
        ),
        ("pending drafts entry", "drafts/ 中有待处理草案：先归档。"),
        ("pending plans entry", "plans/ 中有待处理计划：先归档。"),
        (
            "identity matches are unresolved",
            "存在未知身份：在同步确认框中勾选“记录为旧节点消失 + 新节点出现”。",
        ),
        (
            "filesystem changed between verification scans",
            "扫描期间目录有变化：等目录稳定后重试。",
        ),
        (
            "unapplied manual edits",
            "structure.tree 被手工修改：先另存为草案，再恢复索引记录的基线。",
        ),
        (
            "Git HEAD changed outside organizer",
            "元数据 Git 仓库被外部修改：人工检查 .organizer/.git。",
        ),
        (
            "LLM is not configured",
            "未配置 LLM：设置 ORGANIZER_LLM_MODEL 等环境变量，或使用手工草案。",
        ),
        ("LLM is disabled", "LLM 已禁用：使用手工草案。"),
        (
            "exceeds 512 nodes",
            "范围超过 512 个节点：在结构页选择更小的目录。",
        ),
        (
            "exceeds 256 files",
            "待分类文件超过 256 个：在结构页选择更小的目录。",
        ),
        (
            "LLM connection/request failed",
            "无法连接模型服务：检查地址、TLS 和网络。",
        ),
        (
            "LLM HTTP status",
            "模型服务返回错误状态：检查模型名、密钥和配额。",
        ),
        (
            "LLM refused, truncated",
            "模型拒答或输出被截断：换一种说法或缩小范围后重试。",
        ),
        ("must both be new", "输出文件已存在：换一个文件名。"),
        ("output must be new", "输出文件已存在：换一个文件名。"),
        (
            "partial migration/publication requires manual recovery",
            "文件步骤未全部完成：人工核对后用 M 标记已处理。",
        ),
        (
            "use recover RUN_ID --finish-git",
            "该运行只差 Git 记录：按 F 补记。",
        ),
        ("Permission denied", "权限不足：检查相关目录的权限。"),
    ];
    HINTS
        .iter()
        .find(|(needle, _)| error.contains(needle))
        .map(|(_, hint)| *hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_control_and_bidi() {
        assert_eq!(
            clean("a\u{1b}[2Jb\nc\u{202e}"),
            "a\\u{1b}[2Jb\\u{a}c\\u{202e}"
        );
        assert_eq!(clean("论文 notes.pdf"), "论文 notes.pdf");
    }

    #[test]
    fn widths_and_truncation_respect_cjk() {
        assert_eq!(width("论文ab"), 6);
        assert_eq!(tail("论文论文", 5), "论文…");
        assert_eq!(pad("论", 4), "论  ");
        let path = middle("Research/Papers/Security/ASLR.pdf", 20);
        assert!(
            width(&path) <= 20 && path.ends_with("ASLR.pdf") && path.contains('…'),
            "{path}"
        );
    }

    #[test]
    fn hints_match_library_messages() {
        assert!(hint("organizer metadata is busy or cannot be locked: x").is_some());
        assert!(hint("something unrelated").is_none());
    }
}
