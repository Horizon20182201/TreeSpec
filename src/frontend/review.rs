//! Review-view annotations (design §5.3) from `compare_trees` and the LLM report.

use super::{
    rounds::{Draft, Round},
    text,
};
use crate::{
    llm::file_mapper::Assignment,
    tree::{ChangeKind, NodeKind},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Mark {
    pub symbol: char,
    pub text: String,
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("（根目录）", |(p, _)| p)
}

/// Higher wins when a node has several changes.
fn priority(symbol: char) -> u8 {
    match symbol {
        '?' => 6,
        '+' => 5,
        '>' => 4,
        '~' => 3,
        '#' => 2,
        _ => 1,
    }
}

fn put(marks: &mut HashMap<String, Mark>, id: &str, symbol: char, text: String) {
    match marks.get(id) {
        Some(old) if priority(old.symbol) >= priority(symbol) => {}
        _ => {
            marks.insert(id.to_owned(), Mark { symbol, text });
        }
    }
}

/// `+` new, `~` renamed, `>` new parent, `^` moved with an ancestor,
/// `#` description, `?` pending review.
pub fn marks(round: &Round, draft: &Draft) -> HashMap<String, Mark> {
    let mut marks = HashMap::new();
    if let Ok(diff) = &draft.diff {
        for path in diff.effective_path_changes.iter().filter(|p| p.inherited) {
            put(&mut marks, &path.id, '^', "随上级目录移动".into());
        }
        for change in &diff.changes {
            let old = change.old_path.as_deref().unwrap_or("");
            match change.change {
                ChangeKind::AddNode => put(
                    &mut marks,
                    &change.id,
                    '+',
                    if change.node_kind == NodeKind::Directory {
                        "新目录"
                    } else {
                        "新文件"
                    }
                    .into(),
                ),
                ChangeKind::RenameNode => put(
                    &mut marks,
                    &change.id,
                    '~',
                    format!(
                        "原名 {}",
                        text::clean(old.rsplit('/').next().unwrap_or(old))
                    ),
                ),
                ChangeKind::ReparentNode => put(
                    &mut marks,
                    &change.id,
                    '>',
                    format!("自 {}/", text::clean(parent(old))),
                ),
                ChangeKind::UpdateDescription => {
                    put(&mut marks, &change.id, '#', "说明已改".into())
                }
                ChangeKind::RemoveNode => {}
            }
        }
    }
    if let Some(report) = &round.report {
        for item in &round_unresolved(report) {
            put(&mut marks, item, '?', "待审核".into());
        }
    }
    marks
}

fn round_unresolved(report: &crate::llm::ChatReport) -> Vec<String> {
    report
        .unresolved
        .iter()
        .filter_map(|i| i.node_id.clone())
        .collect()
}

/// IDs shown when only changes are displayed: changed nodes and ancestors.
pub fn filter(round: &Round, draft: &Draft) -> HashSet<String> {
    let marks = marks(round, draft);
    draft.index.with_ancestors(marks.keys())
}

/// Old nodes the target no longer declares (never a deletion command).
pub fn missing(draft: &Draft) -> Vec<String> {
    let Ok(diff) = &draft.diff else { return vec![] };
    diff.changes
        .iter()
        .filter(|c| c.change == ChangeKind::RemoveNode)
        .map(|c| text::clean(c.old_path.as_deref().unwrap_or(&c.id)))
        .collect()
}

/// Detail lines for the selected node.
pub fn detail(round: &Round, draft: &Draft, id: &str) -> Vec<String> {
    let new = draft.index.paths.get(id).cloned().unwrap_or_default();
    let old = draft.diff.as_ref().ok().and_then(|d| {
        d.effective_path_changes
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.old_path.clone())
            .or_else(|| {
                d.changes
                    .iter()
                    .find(|c| c.id == id && c.change == ChangeKind::AddNode)
                    .map(|_| String::new())
            })
    });
    let mut lines = vec![format!("{}  {}", text::clean(&new), id)];
    match old.as_deref() {
        Some("") => lines.push("新节点".into()),
        Some(old) => lines.push(format!("原路径 {}", text::clean(old))),
        None => {}
    }
    if let Some(report) = &round.report {
        for assignment in &report.assignments {
            let (file, decision, reason) = match assignment {
                Assignment::Place {
                    file_id,
                    target_dir_id,
                    reason,
                } => {
                    let target = draft
                        .index
                        .paths
                        .get(target_dir_id)
                        .map_or(target_dir_id.as_str(), String::as_str);
                    (file_id, format!("place → {}", text::clean(target)), reason)
                }
                Assignment::Keep { file_id, reason } => (file_id, "keep".into(), reason),
                Assignment::Review { file_id, reason } => (file_id, "review".into(), reason),
            };
            if file == id {
                lines.push(format!("决定 {decision}"));
                lines.push(format!("理由 {}", text::clean(reason)));
            }
        }
        for item in report
            .unresolved
            .iter()
            .filter(|i| i.node_id.as_deref() == Some(id))
        {
            lines.push(format!("待审核：{}", text::clean(&item.reason)));
        }
    }
    lines
}
