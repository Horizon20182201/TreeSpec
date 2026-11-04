use std::collections::HashMap;

use serde::Serialize;

use crate::error::TreeError;

use super::validate::paths_validated;
use super::{NodeKind, TreeSpec, validate_tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    AddNode,
    RemoveNode,
    RenameNode,
    ReparentNode,
    UpdateDescription,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TreeChange {
    pub id: String,
    pub node_kind: NodeKind,
    pub change: ChangeKind,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_description: Option<String>,
    pub new_description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PathChange {
    pub id: String,
    pub old_path: String,
    pub new_path: String,
    /// True when only ancestors changed, with no local rename/reparent.
    pub inherited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedKind {
    UnresolvedMissingFile,
    UnresolvedNewFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unresolved {
    pub id: String,
    pub reason: UnresolvedKind,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffReport {
    pub schema_version: u32,
    /// Distinguishes this report from an executable migration plan.
    pub report_type: String,
    pub changes: Vec<TreeChange>,
    pub effective_path_changes: Vec<PathChange>,
    pub unresolved: Vec<Unresolved>,
    pub warnings: Vec<String>,
}

/// Pure ID-based comparison. Removal is a declaration change, never deletion.
pub fn diff(old: &TreeSpec, new: &TreeSpec) -> Result<Vec<TreeChange>, TreeError> {
    Ok(compare_trees(old, new)?.changes)
}

/// Compares validated trees; never consults or changes the managed filesystem.
pub fn compare_trees(old: &TreeSpec, new: &TreeSpec) -> Result<DiffReport, TreeError> {
    validate_tree(old)?;
    validate_tree(new)?;
    let old_nodes: HashMap<_, _> = old
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let new_nodes: HashMap<_, _> = new
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let old_paths = paths_validated(old);
    let new_paths = paths_validated(new);
    let mut ids: Vec<_> = old_nodes.keys().chain(new_nodes.keys()).copied().collect();
    ids.sort_unstable();
    ids.dedup();
    let mut report = DiffReport {
        schema_version: 1,
        report_type: "tree_diff".to_owned(),
        changes: Vec::new(),
        effective_path_changes: Vec::new(),
        unresolved: Vec::new(),
        warnings: vec![
            "Structural report only; disk state is unverified and no operations are executable."
                .to_owned(),
        ],
    };
    for id in ids {
        let before = old_nodes.get(id).copied();
        let after = new_nodes.get(id).copied();
        let mut changes = Vec::new();
        let node = match (before, after) {
            (None, Some(node)) => {
                changes.push(ChangeKind::AddNode);
                if node.kind == NodeKind::File {
                    report.unresolved.push(Unresolved {
                        id: id.to_owned(),
                        reason: UnresolvedKind::UnresolvedNewFile,
                        path: new_paths[id].clone(),
                    });
                }
                node
            }
            (Some(node), None) => {
                changes.push(ChangeKind::RemoveNode);
                if node.kind == NodeKind::File {
                    report.unresolved.push(Unresolved {
                        id: id.to_owned(),
                        reason: UnresolvedKind::UnresolvedMissingFile,
                        path: old_paths[id].clone(),
                    });
                } else {
                    report.warnings.push(format!("Directory {id:?} is omitted; this does not authorize removal. Disk emptiness must be verified by a future planner."));
                }
                node
            }
            (Some(before), Some(after)) => {
                if before.kind != after.kind {
                    return Err(TreeError::Conflict {
                        id: id.to_owned(),
                        message: "a stable ID cannot change between file and directory".to_owned(),
                    });
                }
                let renamed = before.name != after.name;
                let reparented = before.parent_id != after.parent_id;
                if before.kind == NodeKind::Directory
                    && (renamed || reparented)
                    && new_paths[id]
                        .strip_prefix(old_paths[id].as_str())
                        .is_some_and(|suffix| suffix.starts_with('/'))
                {
                    return Err(TreeError::Conflict { id: id.to_owned(), message: "directory target lies inside its old path; self-nesting moves are unsupported".to_owned() });
                }
                if renamed {
                    changes.push(ChangeKind::RenameNode);
                }
                if reparented {
                    changes.push(ChangeKind::ReparentNode);
                }
                if before.description != after.description {
                    changes.push(ChangeKind::UpdateDescription);
                }
                if old_paths[id] != new_paths[id] {
                    report.effective_path_changes.push(PathChange {
                        id: id.to_owned(),
                        old_path: old_paths[id].clone(),
                        new_path: new_paths[id].clone(),
                        inherited: !renamed && !reparented,
                    });
                }
                after
            }
            (None, None) => unreachable!("ID came from the union"),
        };
        for change in changes {
            report.changes.push(TreeChange {
                id: id.to_owned(),
                node_kind: node.kind,
                change,
                old_path: old_paths.get(id).cloned(),
                new_path: new_paths.get(id).cloned(),
                old_description: before.and_then(|node| node.description.clone()),
                new_description: after.and_then(|node| node.description.clone()),
            });
        }
    }
    Ok(report)
}
