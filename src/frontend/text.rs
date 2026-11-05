//! Text shared by every front end: escaping untrusted strings and Chinese
//! hints for common library errors (TUI design §12 decision 6).

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

/// Short Chinese guidance for common library errors. The original English
/// error is always shown as well; this only suggests the next step.
pub fn hint(error: &str) -> Option<&'static str> {
    const HINTS: &[(&str, &str)] = &[
        (
            "busy or cannot be locked",
            "元数据正被另一个 organizer 进程使用，稍后重新扫描。",
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
            "未配置 LLM：配置模型服务（TUI 用 ORGANIZER_LLM_* 环境变量，GUI 用设置），或使用手工草案。",
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
            "文件步骤未全部完成：人工核对后“标记已人工处理”。",
        ),
        (
            "use recover RUN_ID --finish-git",
            "该运行只差 Git 记录：用“补记 Git”补记。",
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
    fn hints_match_library_messages() {
        assert!(hint("organizer metadata is busy or cannot be locked: x").is_some());
        assert!(hint("something unrelated").is_none());
    }
}
